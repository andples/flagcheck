# Experiment log

Every experiment, in order, including failures and bugs found by them.
Each entry: date, who ran it, code commit, data, settings, result, cost,
where the raw outputs are. Raw outputs under `bench/runs/` are kept locally
(not committed) until the data release is decided.

Labelling so far is by Claude, not blind, one labeller. Treat P-entries as
pilots that shaped the tool, not as results.

---

## V1 - Pipeline checks without a model (2026-11-15, Claude)

- Commit c8facd2. Mock backend; fake local endpoint returning
  Messages-API-shaped replies.
- LSP over stdio: UTF-16 positions correct on a line with an emoji; flags
  carried over while editing; "Ignore" / "Add to word list" work.
- Fake Anthropic reply: a leaked correction (`should be "receive"`) was cut
  to "misspelled"; a flag inside `\cite{}` dropped (mask); an invented quote
  dropped (anchoring).
- Engineering checks only; no numbers for the paper.

## P1 - First real Haiku run, owner's notes (2026-11-18, owner, Mac)

- Commit 34d55dc. Prompt v1. Haiku 5.5, effort low, adaptive thinking (then
  the default), document context, batches of 16.
- Data: owner's DeepRL reading notes, Typst, 54 lines, British spelling,
  maths and RL acronyms. Not committed (owner's text). Gold: 19 errors
  (15 misspellings, 4 grammar), 2 optional (lowercase proper nouns),
  labelled by Claude.
- Result: 18 flags, 14 genuine, 4 false (78 % precision), 14/19 found.
  False: `optimisers` (British), `million` ("unclear wording", category
  other), `flickering`, `Stable` (word choice).
- 3 requests, 1.8-3.0 s each; thinking tokens 0 on all three. Prompt cache:
  first request wrote 2,420 tokens, the next two read them.
- Led to prompt v2 (7c8c47e): both spelling varieties correct, word choice
  only for clearly wrong words, category `other` removed.

## P2 - Live editing in Zed (2026-11-18, owner, Mac)

- Commits 590b516 / 363b202 / 57c0149. Prompt v2, effort low, adaptive
  thinking. Same notes file plus 6-8 repeated junk test lines.
- Before 363b202: a misspelling repeated across identical lines was
  dropped as "quote ambiguous" (8 drops in one check). Fixed: misspellings
  flag every occurrence; other categories every occurrence with the same
  preceding words.
- Latency by thinking tokens (one request each, 1-16 paragraphs):

  | thinking tokens | latency |
  |---|---|
  | 0 | 1.6-3.2 s |
  | 505-1,107 | 4.0-8.8 s |

  Haiku chose to think on some requests and not others at effort low.
  Owner's impression: "at least 5 seconds" from typing to underline
  (1.5 s debounce + request).
- Opening the file sent three batches in parallel: all three show
  `cached 0`, i.e. each wrote the cache. Fixed in 57c0149 (first batch per
  context window runs alone).
- Led to: thinking off by default (57c0149).

## P3 - Harper 3.2.0 on the same notes (2026-11-18, Claude)

- harper-cli 3.2.0 (GitHub release binary), `lint --format compact`,
  empty user dictionary.
- Configurations and results (same gold, same labeller):

  | config | flags | genuine | optional | false | recall |
  |---|---|---|---|---|---|
  | default (US) | 40 | 14 | 2 | 24 | 14/19 |
  | British | 37 | 14 | 2 | 21 | 14/19 |
  | British + 8-word dictionary | 18 | 14 | 2 | 2 | 14/19 |

  Dictionary: DeepRL, DQN, DRQN, NN, POMDP, MDP, RMSPROP, ADADELTA.
- Harper false flags: acronyms (19 of 21 in the British run) and two
  style rules (long sentence, lowercase list item). Nothing in Typst maths.
- Harper found 0 of 4 grammar errors; P1 found 2. Harper found 3
  misspellings P1 missed; P1 found `updatind`, which Harper missed.
- Takeaway for the paper: precision advantage disappears against a tuned
  baseline; the defensible claim is no-setup precision close to tuned
  baselines plus grammar coverage. E1 must report default and tuned.

## P4 - Prompt v2 configuration sweep (2026-11-18, Claude)

- Commit 7c8c47e..ada4ea5 binary (prompt v2, before the shape filters).
  Same notes file (lines 1-54, no junk lines). 3 runs per config, CLI
  `flagcheck check --json`, scored with `score_pilot.py` (overlap with
  gold token on the same line).

  | config | found | false | precision |
  |---|---|---|---|
  | low, thinking off | 17 / 17 / 16 | 7 / 7 / 7 | ~70 % |
  | low, adaptive thinking | 10 / 11 / 16 | 1 / 1 / 4 | 76-92 % |
  | medium, thinking off | 16 / 16 / 17 | 8 / 4 / 6 | 67-81 % |

- Adaptive thinking: in 2 of 3 runs the last batch (11 paragraphs) came
  back with no flags or one empty placeholder, with 0 thinking tokens.
  Recall therefore swings from 10 to 16. Kept off.
- Offline simulations on the saved outputs:
  - 2-of-3 vote over the low runs: 17 found, 5 false (74-78 %).
  - Shape filters (spelling quote with more than one word; reason
    mentions spelling variety; quote over 6 words): low 73-81 %,
    medium 76-89 %, at most one genuine flag lost per run.
- Recurring false flags with thinking off: "Frame stacking" as spelling
  (every run), `randomises` / `generalise` as "spelling variant
  inconsistent" (the v2 rule did not stop it), whole-sentence quotes.
- Never found in any run: `agents` for `agents'`.
- Led to: shape filters (67548e8), default effort medium (0f35fb8).
- Cost: about 2 cents for 15 runs.

## P5 - New default, real runs (2026-11-18, Claude)

- Commit 0f35fb8. Medium, thinking off, shape filters. 2 runs.

  | run | found | optional | false | precision |
  |---|---|---|---|---|
  | 1 | 15 | 2 | 6 | 65-74 % |
  | 2 | 15 | 1 | 4 | 71-76 % |

- Worse than the P4 simulation: the model quoted `Frame` alone, and
  several false flags had a category contradicting the reason
  ("spelling: wrong word choice", "spelling: unnecessary colon").
- Conclusion: run-to-run spread on one file (about 65-80 % precision,
  15-17 found) is as large as differences between configurations. Stop
  tuning on this file.
- Raw outputs for P3-P5: `bench/runs/pilot-2026-11-18/` (local).

## C1 - Benchmark corpus, first 10 papers (2026-11-18, Claude)

- Commit ff7ec89. `bench/corpus.py`. arXiv OAI-PMH listing, set cs,
  datestamps 2026-10-01 to 2026-10-02 (records updated in that window).
- Eligibility: primary category cs.*, licence CC BY 4.0, CC BY-SA 4.0 or
  CC0, first submitted July 2026 or later, version 1 source under 5 MB,
  main .tex flattened (`\input`, `\include`), 2,000-15,000 prose words
  (counted outside flagcheck's mask).
- Bug found: the OAI `created` field is not the version-1 date (a 2017
  paper showed created 2026-09-29), so two January 2026 papers
  (2601.16294, 2601.08710) first passed. Fixed by reading the month from
  the identifier; both rejected. Eligible pool: 463 papers.
- Rejected while filling: 2609.39666 (16,666 words), 2609.38340 (7.0 MB
  source).
- Selected: 10 papers, 68,201 prose words, 9 CC BY 4.0 and 1 CC BY-SA 4.0,
  all first submitted 2026-08 or 2026-09. List, hashes and licences in
  `bench/corpus/manifest.json`.
- Note: all LaTeX. arXiv does not accept Typst sources; Typst and Markdown
  need another source.
- Download: about 19 MB.
