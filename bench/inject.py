#!/usr/bin/env python3
"""Inject errors into the corpus with fixed rules (no LLM), for recall (E2).

Errors go only into prose (outside flagcheck's mask), at least MIN_GAP
words apart, about one per WORDS_PER_ERROR prose words. Each paper uses its
own seeded RNG, so the output is reproducible. Gold offsets are byte ranges
in the injected file.

Operators (weights in OPERATORS):
  spell-swap    swap two adjacent interior letters       method -> mehtod
  spell-drop    delete one interior letter               method -> mthod
  spell-double  double an interior consonant             method -> metthod
  agreement     is<->are, was<->were, has<->have, does<->do
  article       a<->an
  repetition    duplicate a short function word          of -> of of
  homophone     its<->it's, than->then, their->there

Misspellings that produce a dictionary word (Harper's word list) are
rejected, so every injected spelling error is a non-word.

Usage: bench/inject.py   (reads bench/corpus/manifest.json)
"""

import hashlib
import json
import random
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent
DATA = ROOT / "data"
FLAGCHECK = ROOT.parent / "target" / "release" / "flagcheck"
HARPER = Path.home() / ".local/opt/harper-cli-3.2.0/harper-cli"
OUT = DATA / "injected"

WORDS_PER_ERROR = 250
MIN_GAP = 40
VERSION = 2

OPERATORS = {
    "spell-swap": 2,
    "spell-drop": 2,
    "spell-double": 1,
    "agreement": 2,
    "article": 1,
    "repetition": 1,
    "homophone": 1,
}
SWAPS = {
    "agreement": {"is": "are", "are": "is", "was": "were", "were": "was", "has": "have", "have": "has", "does": "do"},
    "homophone": {"its": "it's", "it's": "its", "than": "then", "their": "there"},
}
REPEATABLE = {"the", "of", "to", "a", "in", "and", "is", "for", "that", "with"}
VOWELS = "aeiou"


def dictionary() -> set[str]:
    out = subprocess.run([HARPER, "words"], capture_output=True, text=True, check=True).stdout
    return {w.strip().strip('"').lower() for w in out.splitlines() if w.strip()}


def prose_words(path: Path) -> list[tuple[int, int, str]]:
    """(start, end, word) for every word whose bytes are all unmasked prose."""
    seg = json.loads(subprocess.run([FLAGCHECK, "segment", path], capture_output=True, check=True).stdout)
    data = path.read_bytes()
    masked = bytearray(len(data))
    for s, e in seg["mask"]:
        masked[s:e] = b"\x01" * (e - s)
    words = []
    for s, e in seg["paragraphs"]:
        for m in re.finditer(rb"[A-Za-z]+(?:'[a-z]+)?", data[s:e]):
            a, b = s + m.start(), s + m.end()
            if not any(masked[a:b]):
                # Neighbouring bytes must not be masked either (e.g. \emph{x}).
                words.append((a, b, data[a:b].decode()))
    return words


def plain(w: str) -> bool:
    """Ordinary lowercase or capitalised word, not an acronym or CamelCase."""
    return w.isalpha() and (w.islower() or (w[0].isupper() and w[1:].islower()))


def mutate(op: str, w: str, nxt: str, rng: random.Random, dic: set[str]) -> str | None:
    lw = w.lower()
    if op.startswith("spell"):
        if not plain(w) or len(w) < 5 or lw not in dic:
            return None
        inner = list(range(1, len(w) - 1))
        rng.shuffle(inner)
        for i in inner:
            if op == "spell-swap" and i + 1 < len(w) - 1 and w[i] != w[i + 1]:
                cand = w[:i] + w[i + 1] + w[i] + w[i + 2 :]
            elif op == "spell-drop" and len(w) >= 6:
                cand = w[:i] + w[i + 1 :]
            elif op == "spell-double" and w[i].lower() not in VOWELS and w[i] != w[i - 1] and w[i] != w[i + 1]:
                cand = w[:i] + w[i] + w[i:]
            else:
                continue
            if cand.lower() not in dic:
                return cand
        return None
    if op in SWAPS:
        rep = SWAPS[op].get(w)
        return rep
    if op == "article":
        if w == "a" and nxt[:1].lower() in "bcdfgjklmnpqrstvwxyz":
            return "an"
        if w == "an" and nxt[:1].lower() in VOWELS:
            return "a"
        return None
    if op == "repetition":
        return f"{w} {w}" if w in REPEATABLE else None
    return None


def inject(path: Path, pid: str, dic: set[str]) -> tuple[bytes, list[dict]]:
    rng = random.Random(int(hashlib.sha256(f"{pid}:{VERSION}".encode()).hexdigest(), 16))
    data = path.read_bytes()
    words = prose_words(path)
    target = max(1, len(words) // WORDS_PER_ERROR)
    ops = list(OPERATORS)
    weights = [OPERATORS[o] for o in ops]
    # Pick the operator first, then a site where it applies, so the
    # operator weights hold even for rare words such as "an" or "were".
    cands: dict[str, list[int]] = {o: [] for o in ops}
    for idx, (_, _, w) in enumerate(words):
        nxt = words[idx + 1][2] if idx + 1 < len(words) else ""
        for o in ops:
            if o.startswith("spell"):
                if plain(w) and len(w) >= 5 and w.lower() in dic:
                    cands[o].append(idx)
            elif mutate(o, w, nxt, rng, dic) is not None:
                cands[o].append(idx)
    chosen: list[tuple[int, int, str, str, str]] = []
    used: list[int] = []
    attempts = 0
    while len(chosen) < target and attempts < target * 50:
        attempts += 1
        op = rng.choices(ops, weights)[0]
        free = [i for i in cands[op] if all(abs(i - u) >= MIN_GAP for u in used)]
        if not free:
            continue
        idx = rng.choice(free)
        s, e, w = words[idx]
        nxt = words[idx + 1][2] if idx + 1 < len(words) else ""
        new = mutate(op, w, nxt, rng, dic)
        if new is None or new == w:
            continue
        chosen.append((s, e, op, w, new))
        used.append(idx)
    chosen.sort()
    out = bytearray()
    gold = []
    pos = 0
    for s, e, op, w, new in chosen:
        out += data[pos:s]
        start = len(out)
        out += new.encode()
        gold.append({"start": start, "end": len(out), "type": op, "original": w, "injected": new})
        pos = e
    out += data[pos:]
    return bytes(out), gold


def main() -> None:
    manifest = json.loads((ROOT / "corpus" / "manifest.json").read_text())
    dic = dictionary()
    OUT.mkdir(parents=True, exist_ok=True)
    total = 0
    summary = {}
    for p in manifest["papers"]:
        src = DATA / "papers" / p["file"]
        text, gold = inject(src, p["id"], dic)
        (OUT / p["file"]).write_bytes(text)
        (OUT / (p["file"] + ".gold.jsonl")).write_text("".join(json.dumps(g) + "\n" for g in gold))
        for g in gold:
            summary[g["type"]] = summary.get(g["type"], 0) + 1
        total += len(gold)
        print(f"{p['id']}: {len(gold)} errors")
    print(f"total {total}: " + ", ".join(f"{k} {v}" for k, v in sorted(summary.items())))


if __name__ == "__main__":
    main()
