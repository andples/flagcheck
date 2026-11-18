#!/usr/bin/env python3
"""Build a blind labelling sheet for flags on the clean papers (E1 precision).

Flags from each tool (after the shared markup filter) are sampled, pooled,
merged where spans overlap, and shuffled. The sheet shows only the paper
and the source around the span; which tools raised each item is kept in a
separate key file the labeller should not open.

Labels:  G genuine error   O optional or style   F false   M markup artefact

Usage: bench/sheet.py RUN_NAME [--per-tool 60] [--seed 1]
Writes bench/runs/<run>/sheet.csv and sheet_key.json.
"""

import argparse
import csv
import json
import random
import re
from pathlib import Path

import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from score import corpus, load, mask_filter, overlaps  # noqa: E402

ROOT = Path(__file__).resolve().parent
CONTEXT = 140


def tool_groups(run: Path) -> dict[str, list[str]]:
    """Repeated flagcheck runs are pooled as one tool."""
    groups: dict[str, list[str]] = {}
    for d in sorted(run.iterdir()):
        if (d / "clean").is_dir():
            name = "flagcheck" if re.fullmatch(r"flagcheck-r\d+", d.name) else d.name
            groups.setdefault(name, []).append(d.name)
    return groups


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("run")
    ap.add_argument("--per-tool", type=int, default=60)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--key-only", action="store_true", help="rewrite only sheet_key.json")
    args = ap.parse_args()
    run = ROOT / "runs" / args.run
    rng = random.Random(args.seed)
    clean_dir, _, papers = corpus(run)

    # Every (tool, paper, flag) after the markup filter.
    pool: dict[str, list[tuple[str, dict]]] = {}
    for tool, dirs in tool_groups(run).items():
        items = []
        for paper in papers:
            for d in dirs:
                for f in mask_filter(load(run / d / "clean" / f"{paper}.jsonl"), clean_dir / paper):
                    items.append((paper, {**f, "run": d}))
        pool[tool] = items

    # Sample per tool, then merge overlapping spans across tools.
    picked: list[tuple[str, dict, str]] = []
    for tool, items in pool.items():
        for paper, f in rng.sample(items, min(args.per_tool, len(items))):
            picked.append((paper, f, tool))
    merged: list[dict] = []
    for paper, f, tool in picked:
        for m in merged:
            if m["paper"] == paper and overlaps(m, f):
                m["start"], m["end"] = min(m["start"], f["start"]), max(m["end"], f["end"])
                m["tools"].add(tool)
                break
        else:
            merged.append({"paper": paper, "start": f["start"], "end": f["end"], "tools": {tool}, "sampled_by": set()})
            m = merged[-1]
        m["sampled_by"].add(tool)
    # Record every tool (sampled or not) whose flags overlap each item, so
    # labels can be reused for all tools.
    for m in merged:
        for tool, items in pool.items():
            runs = {f["run"] for p, f in items if p == m["paper"] and overlaps(f, m)}
            if runs:
                m["tools"].add(tool)
                m.setdefault("runs", {})[tool] = sorted(runs)
    rng.shuffle(merged)

    rows, key = [], {}
    for i, m in enumerate(merged, 1):
        text = (clean_dir / m["paper"]).read_bytes()
        a, b = m["start"], m["end"]
        ctx = (
            text[max(0, a - CONTEXT) : a].decode(errors="replace")
            + " ⟦"
            + text[a:b].decode(errors="replace")
            + "⟧ "
            + text[b : b + CONTEXT].decode(errors="replace")
        ).replace("\n", " ⏎ ")
        iid = f"{i:04d}"
        rows.append({"id": iid, "paper": m["paper"].removesuffix(".tex"), "context": ctx, "label": "", "note": ""})
        key[iid] = {
            "paper": m["paper"],
            "start": a,
            "end": b,
            "tools": sorted(m["tools"]),
            "sampled_by": sorted(m["sampled_by"]),
            "runs": m.get("runs", {}),
        }

    if args.key_only:
        old = list(csv.DictReader(open(run / "sheet.csv")))
        assert [(r["id"], r["context"]) for r in old] == [(r["id"], r["context"]) for r in rows], "sheet would change"
    else:
        with open(run / "sheet.csv", "w", newline="") as fh:
            w = csv.DictWriter(fh, fieldnames=["id", "paper", "context", "label", "note"])
            w.writeheader()
            w.writerows(rows)
    (run / "sheet_key.json").write_text(json.dumps(key, indent=1) + "\n")
    print(f"{len(rows)} items; per tool available: " + ", ".join(f"{t} {len(v)}" for t, v in pool.items()))


if __name__ == "__main__":
    main()
