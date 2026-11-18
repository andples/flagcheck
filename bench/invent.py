#!/usr/bin/env python3
"""Rewrite the corpus into an invented markup language (E6).

No model has seen this syntax, so it tests what happens when a checker meets
a format it has no parser for. The rewrite is mechanical and keeps every
prose byte unchanged; only markup is replaced:

  \\name     -> ◆<nonsense word>   (fixed per name, same in every paper)
  {  }      -> ⟨  ⟩
  [  ]      -> ⟦  ⟧   (only directly after a command, i.e. optional args)
  $         -> ¤
  %comment  -> ;;comment
  \\\\        -> ◆◆
  &         -> ¦      (table cells)

Gold offsets are carried over through a byte-offset map.

Usage: bench/invent.py   -> bench/data/invented/{clean,injected}/*.glyph
"""

import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent
DATA = ROOT / "data"
OUT = DATA / "invented"
SYLLABLES = ["ka", "zo", "ri", "vu", "mel", "tor", "ix", "ba", "nup", "qe", "lo", "dra", "fen", "gu", "sha", "wim"]


def nonsense(name: str) -> str:
    h = hashlib.sha256(f"glyph:{name}".encode()).digest()
    n = 2 + h[0] % 2
    return "".join(SYLLABLES[b % len(SYLLABLES)] for b in h[1 : 1 + n])


def rewrite(text: str) -> tuple[str, list[int]]:
    """Return the new text and, for every byte offset of the old text (plus
    the end), the corresponding byte offset in the new text."""
    out: list[str] = []
    new_len = 0
    omap = [0] * (len(text.encode()) + 1)
    i = 0
    b = 0  # byte offset in old text
    after_command = False
    opt_depth = 0

    def emit(old: str, new: str):
        nonlocal new_len, b
        ob = len(old.encode())
        nb = len(new.encode())
        for k in range(ob):
            # Bytes inside replaced markup map to its start; prose maps 1:1.
            omap[b + k] = new_len + (k if old == new else 0)
        out.append(new)
        new_len += nb
        b += ob

    while i < len(text):
        c = text[i]
        if c == "\\":
            m = re.match(r"\\([A-Za-z@]+\*?)", text[i:])
            if m:
                emit(m.group(0), "◆" + nonsense(m.group(1)))
                i += len(m.group(0))
                after_command = True
                continue
            nxt = text[i + 1 : i + 2]
            if nxt == "\\":
                emit("\\\\", "◆◆")
            elif nxt:
                emit("\\" + nxt, "◆" + nxt)
            else:
                emit("\\", "◆")
            i += 2
            after_command = False
            continue
        if c == "%":
            j = text.find("\n", i)
            j = len(text) if j < 0 else j
            emit(text[i : i + 1], ";;")
            i += 1
            emit(text[i:j], text[i:j])
            i = j
            continue
        rep = {"{": "⟨", "}": "⟩", "$": "¤", "&": "¦"}.get(c)
        if c == "[" and after_command:
            rep = "⟦"
            opt_depth += 1
        elif c == "]" and opt_depth:
            rep = "⟧"
            opt_depth -= 1
        emit(c, rep or c)
        # An option bracket may follow a command name or a closing brace.
        after_command = c in "}]"
        i += 1
    omap[b] = new_len
    return "".join(out), omap


def main():
    manifest = json.loads((ROOT / "corpus" / "manifest.json").read_text())
    for variant, src_dir in (("clean", DATA / "papers"), ("injected", DATA / "injected")):
        (OUT / variant).mkdir(parents=True, exist_ok=True)
        for p in manifest["papers"]:
            src = src_dir / p["file"]
            new, omap = rewrite(src.read_text())
            dst = OUT / variant / p["file"].replace(".tex", ".glyph")
            dst.write_text(new)
            if variant == "injected":
                gold = [json.loads(l) for l in (src_dir / (p["file"] + ".gold.jsonl")).read_text().splitlines()]
                data = new.encode()
                old = src.read_bytes()
                moved = []
                for g in gold:
                    s, e = omap[g["start"]], omap[g["end"]]
                    assert data[s:e] == old[g["start"] : g["end"]], (p["file"], g)
                    moved.append({**g, "start": s, "end": e})
                (dst.parent / (dst.name + ".gold.jsonl")).write_text("".join(json.dumps(g) + "\n" for g in moved))
    print("names:", {n: nonsense(n) for n in ["cite", "section", "begin", "end", "ref", "label", "emph", "textbf"]})


if __name__ == "__main__":
    main()
