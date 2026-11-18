# Benchmark

Experiments for the paper. Scripts are committed; downloaded sources
(`data/`) and tool outputs (`runs/`) are not.

| Step | Script | Output |
|---|---|---|
| Corpus | `corpus.py` | `data/papers/*.tex`, `corpus/manifest.json` |

Corpus rules: arXiv cs papers first submitted July 2026 or later (after the
models' training cutoff, read from the arXiv identifier), licensed CC BY,
CC BY-SA or CC0, version 1, 2,000-15,000 words of prose, source under 5 MB.
Selection is a fixed hash order, so reruns pick the same papers.

Requires `cargo build --release` (uses `flagcheck segment`).
