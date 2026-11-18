#!/usr/bin/env python3
"""Precision from labelled sheets, and agreement between labellers.

Per tool, precision is computed over the items that tool's own random
sample contributed (`sampled_by` in sheet_key.json), so every tool is
judged on an unbiased sample of its flags.

  strict  = G / n
  lenient = (G + O) / n
  95 % Wilson interval for each.

Agreement: Cohen's kappa on the four labels and on the binary split
{G, O} vs {F, M}, over ids labelled in both files.

Usage: bench/precision.py RUN LABELS.csv [OTHER_LABELS.csv]
"""

import collections
import csv
import json
import math
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def wilson(k: int, n: int, z: float = 1.96) -> tuple[float, float]:
    if n == 0:
        return (0.0, 0.0)
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (max(0.0, c - h), min(1.0, c + h))


def kappa(a: list[str], b: list[str]) -> float:
    n = len(a)
    if n == 0:
        return float("nan")
    po = sum(x == y for x, y in zip(a, b)) / n
    ca, cb = collections.Counter(a), collections.Counter(b)
    pe = sum(ca[k] * cb[k] for k in set(a) | set(b)) / (n * n)
    return (po - pe) / (1 - pe) if pe < 1 else 1.0


def read_labels(path: Path) -> dict[str, str]:
    return {r["id"]: r["label"].strip().upper() for r in csv.DictReader(open(path))}


def main() -> None:
    run = ROOT / "runs" / sys.argv[1]
    labels = read_labels(Path(sys.argv[2]))
    key = json.loads((run / "sheet_key.json").read_text())
    out = {"labels": sys.argv[2], "tools": {}}
    tools = sorted({t for v in key.values() for t in v["sampled_by"]})
    print(f"{'tool':16} {'n':>3}  G  O  F  M   strict (95% CI)      lenient (95% CI)")
    for t in tools:
        ids = [i for i, v in key.items() if t in v["sampled_by"] and i in labels]
        c = collections.Counter(labels[i] for i in ids)
        n = len(ids)
        g, o = c["G"], c["O"]
        s_lo, s_hi = wilson(g, n)
        l_lo, l_hi = wilson(g + o, n)
        out["tools"][t] = {"n": n, "counts": dict(c), "strict": g / n if n else None,
                           "strict_ci": [s_lo, s_hi], "lenient": (g + o) / n if n else None,
                           "lenient_ci": [l_lo, l_hi]}
        print(f"{t:16} {n:3} {g:2} {o:2} {c['F']:2} {c['M']:2}   {g/n:.2f} ({s_lo:.2f}-{s_hi:.2f})"
              f"     {(g+o)/n:.2f} ({l_lo:.2f}-{l_hi:.2f})")
    if len(sys.argv) > 3:
        other = read_labels(Path(sys.argv[3]))
        both = sorted(set(labels) & set(other))
        a = [labels[i] for i in both]
        b = [other[i] for i in both]
        binary = lambda xs: ["keep" if x in "GO" else "bad" for x in xs]
        out["agreement"] = {
            "n": len(both),
            "raw": sum(x == y for x, y in zip(a, b)) / len(both),
            "kappa_4": kappa(a, b),
            "kappa_binary": kappa(binary(a), binary(b)),
        }
        print(f"\nagreement on {len(both)} items: raw {out['agreement']['raw']:.2f}, "
              f"kappa (4 labels) {out['agreement']['kappa_4']:.2f}, kappa (G+O vs F+M) {out['agreement']['kappa_binary']:.2f}")
    name = Path(sys.argv[2]).stem
    (run / f"precision-{name}.json").write_text(json.dumps(out, indent=1) + "\n")


if __name__ == "__main__":
    main()
