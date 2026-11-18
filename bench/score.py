#!/usr/bin/env python3
"""Score a run.

E2 (recall on injected errors): an injected error counts as found when any
flag overlaps its byte range. Reported overall and by error type.

E1 (clean papers): flags per 1,000 prose words, before and after the shared
markup filter (a flag is removed when every letter or digit in it is inside
flagcheck's mask). Precision on clean text needs labels: see sheet.py.

Variance: for repeated flagcheck runs, recall per run and the overlap of
flag sets between runs (Jaccard over spans, matched by overlap).

Usage: bench/score.py RUN_NAME   (writes bench/runs/<run>/scores.json)
"""

import collections
import json
import re
import statistics
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
DATA = ROOT / "data"
FLAGCHECK = ROOT.parent / "target" / "release" / "flagcheck"


def load(path: Path) -> list[dict]:
    return [json.loads(l) for l in path.read_text().splitlines()] if path.exists() else []


def overlaps(a: dict, b: dict) -> bool:
    return a["start"] < b["end"] and b["start"] < a["end"]


_masks: dict[Path, bytearray] = {}


def masked(path: Path) -> bytearray:
    if path not in _masks:
        seg = json.loads(subprocess.run([FLAGCHECK, "segment", path], capture_output=True, check=True).stdout)
        m = bytearray(path.stat().st_size)
        for s, e in seg["mask"]:
            m[s:e] = b"\x01" * (e - s)
        _masks[path] = m
    return _masks[path]


def mask_filter(flags: list[dict], path: Path) -> list[dict]:
    data, m = path.read_bytes(), masked(path)
    out = []
    for f in flags:
        idx = [i for i in range(f["start"], min(f["end"], len(data))) if chr(data[i]).isalnum()]
        if idx and all(m[i] for i in idx):
            continue
        out.append(f)
    return out


def jaccard(a: list[dict], b: list[dict]) -> float:
    if not a and not b:
        return 1.0
    matched = sum(1 for f in a if any(overlaps(f, g) for g in b))
    return matched / (len(a) + len(b) - matched)


def main() -> None:
    run = ROOT / "runs" / sys.argv[1]
    manifest = json.loads((ROOT / "corpus" / "manifest.json").read_text())
    papers = manifest["papers"]
    words = sum(p["prose_words"] for p in papers)
    tools = sorted(d.name for d in run.iterdir() if (d / "clean").is_dir())
    gold = {p["file"]: load(DATA / "injected" / (p["file"] + ".gold.jsonl")) for p in papers}
    n_gold = sum(len(g) for g in gold.values())

    scores: dict = {"run": run.name, "papers": len(papers), "prose_words": words, "injected": n_gold, "tools": {}}
    for tool in tools:
        t: dict = {}
        for filt in ("raw", "masked"):
            found = collections.Counter()
            by_type = collections.Counter(g["type"] for gs in gold.values() for g in gs)
            clean_flags = 0
            inj_other = 0
            for p in papers:
                f = p["file"]
                clean = load(run / tool / "clean" / f"{f}.jsonl")
                inj = load(run / tool / "injected" / f"{f}.jsonl")
                if filt == "masked":
                    clean = mask_filter(clean, DATA / "papers" / f)
                    inj = mask_filter(inj, DATA / "injected" / f)
                clean_flags += len(clean)
                for g in gold[f]:
                    if any(overlaps(g, x) for x in inj):
                        found[g["type"]] += 1
                inj_other += sum(1 for x in inj if not any(overlaps(x, g) for g in gold[f]))
            t[filt] = {
                "recall": round(sum(found.values()) / n_gold, 3),
                "recall_by_type": {k: f"{found[k]}/{v}" for k, v in sorted(by_type.items())},
                "clean_flags": clean_flags,
                "clean_flags_per_1k_words": round(1000 * clean_flags / words, 2),
                "injected_other_flags": inj_other,
            }
        scores["tools"][tool] = t

    reps = [t for t in tools if re.fullmatch(r"flagcheck-r\d+", t)]
    if len(reps) > 1:
        rec = [scores["tools"][t]["masked"]["recall"] for t in reps]
        jac = []
        for i in range(len(reps)):
            for j in range(i + 1, len(reps)):
                for p in papers:
                    a = load(run / reps[i] / "clean" / f"{p['file']}.jsonl")
                    b = load(run / reps[j] / "clean" / f"{p['file']}.jsonl")
                    jac.append(jaccard(a, b))
        scores["flagcheck_variance"] = {
            "recall_mean": round(statistics.mean(rec), 3),
            "recall_min": min(rec),
            "recall_max": max(rec),
            "clean_jaccard_mean": round(statistics.mean(jac), 3),
        }

    (run / "scores.json").write_text(json.dumps(scores, indent=1) + "\n")
    w = max(len(t) for t in tools)
    print(f"{'tool':{w}}  filter  recall  clean/1k  inj-other")
    for tool in tools:
        for filt in ("raw", "masked"):
            s = scores["tools"][tool][filt]
            print(f"{tool:{w}}  {filt:6}  {s['recall']:.3f}  {s['clean_flags_per_1k_words']:8}  {s['injected_other_flags']:9}")
    if "flagcheck_variance" in scores:
        print("flagcheck variance:", scores["flagcheck_variance"])


if __name__ == "__main__":
    main()
