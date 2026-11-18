#!/usr/bin/env python3
"""Run every tool on the clean and injected corpus.

Writes bench/runs/<run>/<tool>/<clean|injected>/<paper>.jsonl with flags as
{"start", "end", "quote", "category"} (byte offsets into the file the tool
saw), plus bench/runs/<run>/meta.json recording the commit, tool versions,
settings, timings, token usage and estimated cost.

Tools:
  flagcheck-rN   flagcheck CLI, default settings, repeat N (--repeats)
  harper-us      Harper defaults (US dialect, no dictionary)
  harper-tuned   Harper with the dialect that raises fewer lints on the clean
                 paper, and a dictionary of every word Harper flags as a
                 misspelling at least twice in the clean paper (what a user
                 adding recurring jargon would do)

Usage: bench/run.py RUN_NAME [--repeats 3] [--only flagcheck|harper]
Needs ANTHROPIC_API_KEY for flagcheck.
"""

import argparse
import collections
import concurrent.futures as cf
import datetime
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent
DATA = ROOT / "data"
RUNS = ROOT / "runs"
FLAGCHECK = ROOT.parent / "target" / "release" / "flagcheck"
HARPER = Path.home() / ".local/opt/harper-cli-3.2.0/harper-cli"

# Haiku 5.5 list prices, USD per million tokens (prompts up to 100k tokens).
PRICE = {"input": 0.10, "output": 0.50, "cache_read": 0.01, "cache_write_5m": 0.125}


def sh(*args, **kw) -> str:
    return subprocess.run([str(a) for a in args], capture_output=True, text=True, **kw).stdout


def commit() -> str:
    dirty = sh("git", "-C", ROOT.parent, "status", "--porcelain", "--", "server").strip()
    return sh("git", "-C", ROOT.parent, "rev-parse", "--short", "HEAD").strip() + ("-dirty" if dirty else "")


def write(path: Path, flags: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("".join(json.dumps(f) + "\n" for f in flags))


# --- flagcheck -------------------------------------------------------------


def run_flagcheck(src: Path, out: Path, log: Path) -> dict:
    t0 = time.time()
    env = dict(os.environ)
    p = subprocess.run(
        [FLAGCHECK, "check", "--json", "--log", log, src], capture_output=True, text=True, env=env
    )
    if p.returncode != 0:
        raise RuntimeError(f"flagcheck failed on {src.name}: {p.stderr[-300:]}")
    flags = [
        {"start": f["start"], "end": f["end"], "quote": f["quote"], "category": f["category"]}
        for f in map(json.loads, p.stdout.splitlines())
    ]
    write(out, flags)
    return {"seconds": round(time.time() - t0, 1), "flags": len(flags)}


def usage_totals(logs: list[Path]) -> dict:
    tot = collections.Counter()
    requests = 0
    for log in logs:
        if not log.exists():
            continue
        for line in log.read_text().splitlines():
            rec = json.loads(line)
            for u in rec.get("usage") or []:
                if not u:
                    continue
                requests += 1
                tot["input"] += u.get("input_tokens", 0)
                tot["output"] += u.get("output_tokens", 0)
                tot["cache_read"] += u.get("cache_read_input_tokens", 0)
                tot["cache_write_5m"] += u.get("cache_creation_input_tokens", 0)
                tot["thinking"] += (u.get("output_tokens_details") or {}).get("thinking_tokens", 0)
    cost = sum(tot[k] * PRICE[k] / 1e6 for k in PRICE)
    return {"requests": requests, "tokens": dict(tot), "usd": round(cost, 4)}


# --- Harper ----------------------------------------------------------------


def harper_lints(src: Path, dialect: str, dictionary: Path) -> list[dict]:
    out = sh(HARPER, "lint", "--quiet", "--no-color", "--format", "json", "-d", dialect, "-u", dictionary, src)
    files = json.loads(out) if out.strip() else []
    text = src.read_text()
    # Harper spans count characters; convert to byte offsets.
    byte_at = [0]
    for ch in text:
        byte_at.append(byte_at[-1] + len(ch.encode()))
    flags = []
    for f in files:
        for l in f["lints"]:
            s, e = l["span"]["char_start"], l["span"]["char_end"]
            flags.append(
                {
                    "start": byte_at[s],
                    "end": byte_at[e],
                    "quote": l.get("matched_text", text[s:e]),
                    "category": f"{l['kind']}::{l['rule']}",
                }
            )
    return flags


def run_harper(paper: str, run_dir: Path) -> dict:
    clean, injected = DATA / "papers" / paper, DATA / "injected" / paper
    empty = run_dir / "harper-empty.dict"
    empty.write_text("")
    info = {}
    # Defaults.
    for variant, src in (("clean", clean), ("injected", injected)):
        write(run_dir / "harper-us" / variant / f"{paper}.jsonl", harper_lints(src, "us", empty))
    # Tuned: dialect with fewer lints on the clean paper, then a dictionary
    # of words flagged as misspellings at least twice in the clean paper.
    counts = {d: len(harper_lints(clean, d, empty)) for d in ("us", "british")}
    dialect = min(counts, key=counts.get)
    spelled = collections.Counter(
        f["quote"] for f in harper_lints(clean, dialect, empty) if f["category"].startswith("Spelling")
    )
    words = sorted(w for w, n in spelled.items() if n >= 2)
    dic = run_dir / "harper-tuned" / "dict" / f"{paper}.txt"
    dic.parent.mkdir(parents=True, exist_ok=True)
    dic.write_text("".join(w + "\n" for w in words))
    for variant, src in (("clean", clean), ("injected", injected)):
        write(run_dir / "harper-tuned" / variant / f"{paper}.jsonl", harper_lints(src, dialect, dic))
    info[paper] = {"dialect": dialect, "dictionary_words": len(words), "lint_counts": counts}
    return info


# --- main ------------------------------------------------------------------


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("run")
    ap.add_argument("--repeats", type=int, default=3)
    ap.add_argument("--only", choices=["flagcheck", "harper"])
    ap.add_argument("--jobs", type=int, default=4)
    args = ap.parse_args()

    manifest = json.loads((ROOT / "corpus" / "manifest.json").read_text())
    papers = [p["file"] for p in manifest["papers"]]
    run_dir = RUNS / args.run
    run_dir.mkdir(parents=True, exist_ok=True)
    meta_path = run_dir / "meta.json"
    meta = json.loads(meta_path.read_text()) if meta_path.exists() else {}
    meta.update(
        {
            "run": args.run,
            "started": meta.get("started") or datetime.datetime.now().isoformat(timespec="seconds"),
            "commit": commit(),
            "papers": papers,
            "injection_version": 2,
        }
    )

    if args.only != "harper":
        if not os.environ.get("ANTHROPIC_API_KEY"):
            sys.exit("ANTHROPIC_API_KEY is not set")
        meta["flagcheck"] = {
            "version": sh(FLAGCHECK, "--version").strip(),
            "settings": "defaults (see DESIGN.md at the commit)",
            "repeats": args.repeats,
            "jobs": {},
        }
        jobs = []
        for r in range(1, args.repeats + 1):
            tool = f"flagcheck-r{r}"
            for paper in papers:
                for variant in ("clean", "injected"):
                    src = DATA / ("papers" if variant == "clean" else "injected") / paper
                    out = run_dir / tool / variant / f"{paper}.jsonl"
                    log = run_dir / tool / "logs" / variant / f"{paper}.jsonl"
                    if out.exists():
                        continue
                    log.parent.mkdir(parents=True, exist_ok=True)
                    log.unlink(missing_ok=True)
                    jobs.append((tool, variant, paper, src, out, log))
        with cf.ThreadPoolExecutor(args.jobs) as ex:
            futs = {ex.submit(run_flagcheck, j[3], j[4], j[5]): j for j in jobs}
            for fut in cf.as_completed(futs):
                tool, variant, paper, *_ = futs[fut]
                res = fut.result()
                meta["flagcheck"]["jobs"][f"{tool}/{variant}/{paper}"] = res
                print(f"{tool} {variant} {paper}: {res['flags']} flags, {res['seconds']} s", flush=True)
        logs = list(run_dir.glob("flagcheck-r*/logs/*/*.jsonl"))
        meta["flagcheck"]["usage"] = usage_totals(logs)

    if args.only != "flagcheck":
        meta["harper"] = {"version": sh(HARPER, "--version").strip(), "papers": {}}
        for paper in papers:
            meta["harper"]["papers"].update(run_harper(paper, run_dir))
            print(f"harper {paper}: done", flush=True)

    meta["finished"] = datetime.datetime.now().isoformat(timespec="seconds")
    meta_path.write_text(json.dumps(meta, indent=1) + "\n")
    print(json.dumps(meta.get("flagcheck", {}).get("usage", {})))


if __name__ == "__main__":
    main()
