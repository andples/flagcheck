#!/usr/bin/env python3
"""Run LTeX+ (LanguageTool for LaTeX) over the corpus through its language
server, to get exact diagnostic ranges (the CLI prints only start points).

Tools written into bench/runs/<run>/:
  ltex-default   LTeX+ defaults (en-US, no dictionary)
  ltex-tuned     language picked like harper-tuned (variant with fewer
                 diagnostics on the clean paper, en-US or en-GB), plus a
                 dictionary of every word flagged as a misspelling at least
                 twice in the clean paper

Usage: bench/ltex.py RUN_NAME
"""

import collections
import json
import re
import subprocess
import sys
import threading
import queue
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent
DATA = ROOT / "data"
LTEX = Path.home() / ".local/opt/ltex-ls-plus-18.7.0/ltex-ls-plus-18.7.0/bin/ltex-ls-plus"
TIMEOUT = 300


class Client:
    def __init__(self, settings: dict):
        self.settings = settings
        self.p = subprocess.Popen([LTEX], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        self.q: queue.Queue = queue.Queue()
        threading.Thread(target=self._read, daemon=True).start()
        self.id = 0

    def _read(self):
        while True:
            h = b""
            while not h.endswith(b"\r\n\r\n"):
                c = self.p.stdout.read(1)
                if not c:
                    return
                h += c
            n = int(re.search(rb"Content-Length: *(\d+)", h, re.I).group(1))
            self.q.put(json.loads(self.p.stdout.read(n)))

    def send(self, m: dict):
        b = json.dumps(m).encode()
        self.p.stdin.write(b"Content-Length: %d\r\n\r\n" % len(b) + b)
        self.p.stdin.flush()

    def request(self, method: str, params: dict):
        self.id += 1
        self.send({"jsonrpc": "2.0", "id": self.id, "method": method, "params": params})
        return self.id

    def pump(self, until, timeout=TIMEOUT):
        """Answer server requests until `until(msg)` is true."""
        end = time.time() + timeout
        while time.time() < end:
            try:
                m = self.q.get(timeout=0.5)
            except queue.Empty:
                continue
            if "method" in m and "id" in m:
                # Server-to-client request: configuration, progress, etc.
                if m["method"] == "workspace/configuration":
                    items = m["params"]["items"]
                    self.send({"jsonrpc": "2.0", "id": m["id"], "result": [self.settings for _ in items]})
                else:
                    self.send({"jsonrpc": "2.0", "id": m["id"], "result": None})
                continue
            if until(m):
                return m
        raise TimeoutError("ltex-ls-plus did not answer")

    def close(self):
        try:
            rid = self.request("shutdown", None)
            self.pump(lambda m: m.get("id") == rid, timeout=20)
            self.send({"jsonrpc": "2.0", "method": "exit"})
        finally:
            self.p.kill()


def diagnostics(path: Path, settings: dict) -> list[dict]:
    c = Client(settings)
    try:
        rid = c.request(
            "initialize",
            {"processId": None, "rootUri": None, "capabilities": {"workspace": {"configuration": True}},
             "initializationOptions": {}},
        )
        c.pump(lambda m: m.get("id") == rid)
        c.send({"jsonrpc": "2.0", "method": "initialized", "params": {}})
        uri = path.resolve().as_uri()
        text = path.read_text()
        c.send(
            {"jsonrpc": "2.0", "method": "textDocument/didOpen",
             "params": {"textDocument": {"uri": uri, "languageId": "latex", "version": 1, "text": text}}}
        )
        m = c.pump(lambda m: m.get("method") == "textDocument/publishDiagnostics" and m["params"]["uri"] == uri
                   and m["params"].get("diagnostics") is not None and (m["params"]["diagnostics"] or True))
        # LTeX publishes an empty set first in some versions; wait for a
        # non-empty one briefly before accepting empty.
        diags = m["params"]["diagnostics"]
        if not diags:
            try:
                m = c.pump(lambda m: m.get("method") == "textDocument/publishDiagnostics" and m["params"]["uri"] == uri,
                           timeout=60)
                diags = m["params"]["diagnostics"]
            except TimeoutError:
                pass
    finally:
        c.close()
    return to_bytes(text, diags)


def to_bytes(text: str, diags: list[dict]) -> list[dict]:
    """LSP UTF-16 positions -> byte offsets."""
    lines = text.split("\n")
    starts, pos = [], 0
    for ln in lines:
        starts.append(pos)
        pos += len(ln.encode()) + 1

    def off(p):
        line = lines[p["line"]] if p["line"] < len(lines) else ""
        units, b = 0, 0
        for ch in line:
            if units >= p["character"]:
                break
            units += len(ch.encode("utf-16-le")) // 2
            b += len(ch.encode())
        return starts[min(p["line"], len(starts) - 1)] + b

    out = []
    for d in diags:
        s, e = off(d["range"]["start"]), off(d["range"]["end"])
        raw = text.encode()[s:e].decode(errors="replace")
        out.append({"start": s, "end": e, "quote": raw, "category": str(d.get("code", ""))})
    return out


def settings(lang: str, words: list[str]) -> dict:
    return {"ltex": {"language": lang, "dictionary": {lang: words}, "checkFrequency": "edit"}}


def main():
    run = ROOT / "runs" / sys.argv[1]
    papers = [p["file"] for p in json.loads((ROOT / "corpus" / "manifest.json").read_text())["papers"]]
    meta_path = run / "meta.json"
    meta = json.loads(meta_path.read_text())
    meta["ltex"] = {"version": "ltex-ls-plus 18.7.0", "papers": {}}
    for paper in papers:
        clean, inj = DATA / "papers" / paper, DATA / "injected" / paper
        default = diagnostics(clean, settings("en-US", []))
        write(run / "ltex-default" / "clean" / f"{paper}.jsonl", default)
        write(run / "ltex-default" / "injected" / f"{paper}.jsonl", diagnostics(inj, settings("en-US", [])))
        gb = diagnostics(clean, settings("en-GB", []))
        lang, base = ("en-GB", gb) if len(gb) < len(default) else ("en-US", default)
        spelled = collections.Counter(f["quote"] for f in base if "MORFOLOGIK" in f["category"])
        words = sorted(w for w, n in spelled.items() if n >= 2)
        write(run / "ltex-tuned" / "clean" / f"{paper}.jsonl", diagnostics(clean, settings(lang, words)))
        write(run / "ltex-tuned" / "injected" / f"{paper}.jsonl", diagnostics(inj, settings(lang, words)))
        meta["ltex"]["papers"][paper] = {"language": lang, "dictionary_words": len(words),
                                         "counts": {"en-US": len(default), "en-GB": len(gb)}}
        print(f"ltex {paper}: {lang}, {len(words)} dictionary words, {len(default)} default diagnostics", flush=True)
        meta_path.write_text(json.dumps(meta, indent=1) + "\n")


def write(path: Path, flags: list[dict]):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("".join(json.dumps(f) + "\n" for f in flags))


if __name__ == "__main__":
    main()
