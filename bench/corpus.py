#!/usr/bin/env python3
"""Build the benchmark corpus from arXiv.

Picks computer-science papers first posted after the models' training cutoff
(June 2026) under licences that allow redistribution with attribution
(CC BY, CC BY-SA, CC0), downloads version 1 of each LaTeX source, flattens
\\input/\\include into one file, and records everything in
bench/corpus/manifest.json.

Usage: bench/corpus.py [--n 10] [--from 2026-10-01] [--until 2026-10-02]

Sources go to bench/data/ (not committed). arXiv asks for at most one
request every three seconds; this script waits that long between requests.
"""

import argparse
import gzip
import hashlib
import io
import json
import os
import re
import subprocess
import sys
import tarfile
import time
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parent
DATA = ROOT / "data"
MANIFEST = ROOT / "corpus" / "manifest.json"
FLAGCHECK = ROOT.parent / "target" / "release" / "flagcheck"
UA = "flagcheck-bench/0.1 (research benchmark)"
CUTOFF = "2026-07-01"
LICENCES = {
    "http://creativecommons.org/licenses/by/4.0/": "CC BY 4.0",
    "http://creativecommons.org/licenses/by-sa/4.0/": "CC BY-SA 4.0",
    "http://creativecommons.org/publicdomain/zero/1.0/": "CC0 1.0",
}
MAX_SOURCE_BYTES = 5_000_000
MIN_WORDS, MAX_WORDS = 2000, 15000
NS = {"o": "http://www.openarchives.org/OAI/2.0/", "a": "http://arxiv.org/OAI/arXiv/"}

_last = 0.0


def get(url: str) -> tuple[bytes, str]:
    global _last
    wait = 3.0 - (time.time() - _last)
    if wait > 0:
        time.sleep(wait)
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=60) as r:
        n = int(r.headers.get("Content-Length") or 0)
        if n > MAX_SOURCE_BYTES:
            _last = time.time()
            raise ValueError(f"too large ({n} bytes)")
        body = r.read(MAX_SOURCE_BYTES + 1)
        ctype = r.headers.get("Content-Type", "")
    _last = time.time()
    if len(body) > MAX_SOURCE_BYTES:
        raise ValueError("too large")
    return body, ctype


def listing(frm: str, until: str) -> list[dict]:
    cache = DATA / "oai" / f"cs-{frm}-{until}.xml"
    if not cache.exists():
        cache.parent.mkdir(parents=True, exist_ok=True)
        url = (
            "https://oaipmh.arxiv.org/oai?verb=ListRecords&metadataPrefix=arXiv"
            f"&set=cs&from={frm}&until={until}"
        )
        body, _ = get(url)
        cache.write_bytes(body)
    root = ET.fromstring(cache.read_bytes())
    out = []
    for rec in root.iterfind(".//o:record", NS):
        m = rec.find(".//a:arXiv", NS)
        if m is None:
            continue
        t = lambda k: (m.findtext(f"a:{k}", default="", namespaces=NS) or "").strip()
        out.append(
            {
                "id": t("id"),
                "created": t("created"),
                "title": " ".join(t("title").split()),
                "categories": t("categories").split(),
                "license": t("license"),
            }
        )
    return out


def first_month(pid: str) -> str:
    """YYMM of first submission, from the identifier. The OAI `created`
    field is not the version-1 date, so it cannot be used for the cutoff."""
    m = re.match(r"^(\d{2})(\d{2})\.\d{4,5}$", pid)
    return f"20{m.group(1)}-{m.group(2)}" if m else "0000-00"


def eligible(p: dict) -> bool:
    return (
        first_month(p["id"]) >= CUTOFF[:7]
        and p["license"] in LICENCES
        and p["categories"]
        and p["categories"][0].startswith("cs.")
    )


def unpack(body: bytes, ctype: str, dest: Path) -> None:
    """Unpack an arXiv e-print into `dest`, refusing anything outside it."""
    dest.mkdir(parents=True, exist_ok=True)
    if "pdf" in ctype:
        raise ValueError("PDF only")
    try:
        raw = gzip.decompress(body)
    except OSError:
        raw = body
    try:
        with tarfile.open(fileobj=io.BytesIO(raw)) as tf:
            tf.extractall(dest, filter="data")
        return
    except tarfile.ReadError:
        pass
    if raw.lstrip()[:1] in (b"\\", b"%"):
        (dest / "main.tex").write_bytes(raw)
        return
    raise ValueError("not a LaTeX source")


def find_main(src: Path) -> Path:
    cands = []
    for f in src.rglob("*.tex"):
        t = f.read_text(errors="replace")
        if "\\documentclass" in t and "\\begin{document}" in t:
            cands.append((len(t), f))
    if not cands:
        raise ValueError("no main .tex")
    return max(cands)[1]


INPUT = re.compile(r"\\(?:input|include|subfile)\s*\{([^}]+)\}")


def flatten(path: Path, root: Path, depth: int = 0) -> str:
    text = path.read_text(errors="replace")
    if depth > 8:
        return text

    def sub(m: re.Match) -> str:
        # Leave commented-out inputs alone.
        line_start = text.rfind("\n", 0, m.start()) + 1
        if "%" in text[line_start : m.start()].replace("\\%", ""):
            return m.group(0)
        name = m.group(1).strip()
        for cand in (root / name, root / f"{name}.tex", path.parent / name, path.parent / f"{name}.tex"):
            if cand.is_file() and root in cand.resolve().parents:
                return flatten(cand, root, depth + 1)
        return m.group(0)

    return INPUT.sub(sub, text)


def prose_words(path: Path) -> int:
    seg = json.loads(subprocess.run([FLAGCHECK, "segment", path], capture_output=True, check=True).stdout)
    text = path.read_bytes()
    mask = seg["mask"]
    words = 0
    for s, e in seg["paragraphs"]:
        keep = bytearray(text[s:e])
        for ms, me in mask:
            if me <= s or ms >= e:
                continue
            for i in range(max(ms, s), min(me, e)):
                keep[i - s] = 32
        words += len(re.findall(rb"[A-Za-z]{2,}", bytes(keep)))
    return words


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--n", type=int, default=10)
    ap.add_argument("--from", dest="frm", default="2026-10-01")
    ap.add_argument("--until", default="2026-10-02")
    args = ap.parse_args()

    if not FLAGCHECK.exists():
        sys.exit("build first: cargo build --release")
    manifest = json.loads(MANIFEST.read_text()) if MANIFEST.exists() else {"papers": [], "rejected": {}}
    have = {p["id"] for p in manifest["papers"]}
    pool = [p for p in listing(args.frm, args.until) if eligible(p)]
    # Deterministic shuffle so the selection is reproducible.
    pool.sort(key=lambda p: hashlib.sha256(p["id"].encode()).hexdigest())
    print(f"{len(pool)} eligible papers", file=sys.stderr)

    for p in pool:
        if len(manifest["papers"]) >= args.n:
            break
        if p["id"] in have or p["id"] in manifest["rejected"]:
            continue
        pid = p["id"]
        try:
            body, ctype = get(f"https://arxiv.org/e-print/{pid}v1")
            src = DATA / "src" / pid.replace("/", "_")
            unpack(body, ctype, src)
            main_tex = find_main(src)
            flat = flatten(main_tex, src.resolve())
            out = DATA / "papers" / f"{pid.replace('/', '_')}.tex"
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_text(flat)
            words = prose_words(out)
            if not MIN_WORDS <= words <= MAX_WORDS:
                raise ValueError(f"{words} prose words")
        except Exception as e:  # noqa: BLE001 - record and move on
            manifest["rejected"][pid] = str(e)[:120]
            print(f"skip {pid}: {e}", file=sys.stderr)
            continue
        manifest["papers"].append(
            {
                "id": pid,
                "version": 1,
                "title": p["title"],
                "first_submitted": first_month(pid),
                "categories": p["categories"],
                "license": LICENCES[p["license"]],
                "file": out.name,
                "sha256": hashlib.sha256(out.read_bytes()).hexdigest(),
                "prose_words": words,
            }
        )
        print(f"ok   {pid}: {words} words, {LICENCES[p['license']]}", file=sys.stderr)
        MANIFEST.parent.mkdir(parents=True, exist_ok=True)
        MANIFEST.write_text(json.dumps(manifest, indent=1) + "\n")
    MANIFEST.write_text(json.dumps(manifest, indent=1) + "\n")


if __name__ == "__main__":
    main()
