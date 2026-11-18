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

## C2 - Error injection, version 2 (2026-11-18, Claude)

- `bench/inject.py`, VERSION 2. Rule-based, no LLM. Prose only (outside
  flagcheck's mask), sites at least 40 words apart, one error per ~250
  prose words, RNG seeded per paper.
- Operators: spell-swap, spell-drop, spell-double (non-words only, checked
  against Harper 3.2.0's 138k-word list), agreement (is/are, was/were,
  has/have, does/do), article (a/an), repetition (short function word),
  homophone (its/it's, than->then, their->there).
- Version 1 picked a site first and then an operator: 90 % of errors were
  misspellings because grammar operators rarely matched the site. Version
  2 picks the operator first, then a matching site.
- Result: 278 errors in 10 papers. spell-swap 59, spell-drop 58,
  spell-double 30, agreement 55, homophone 26, article 25, repetition 25.
- Known validity problem: some agreement swaps stay acceptable (collective
  nouns, e.g. "The egregious set were constructed"; "a population of
  players are"). Needs a validity check; report the invalid rate and
  exclude invalid items from recall.
- Uses flagcheck's own mask to place errors. Bias: errors are never placed
  where flagcheck thinks there is markup. Same for every tool scored.

## R1 - First benchmark run: flagcheck x3 and Harper on 10 papers (2026-11-18, Claude)

- Commit 02b600a (server unchanged since 8f700af). Corpus C1 (10 papers,
  68,201 prose words), injection C2 (278 errors). Outputs:
  `bench/runs/r1/` (local), scores in `bench/runs/r1/scores.json`.
- flagcheck: defaults (Haiku 5.5, prompt v2, effort medium, thinking off,
  shape filters, document context, batches of 16), 3 independent runs, each
  on the clean and the injected copy of every paper. 564 requests,
  1.45M input + 16.7M cache-read + 0.55M cache-write + 0.20M output
  tokens, 0 thinking tokens. Cost $0.48. Wall time 9 min (4 papers in
  parallel).
- Harper 3.2.0: `harper-us` = defaults; `harper-tuned` = dialect with fewer
  lints on the clean paper (British for 2 papers) plus a per-paper
  dictionary of every word Harper flags as a misspelling at least twice in
  the clean paper (1-73 words per paper). Harper reads .tex as plain text
  (no LaTeX support).
- "masked" = the shared markup filter: a flag is removed when all its
  letters and digits are inside flagcheck's mask. Applied to every tool.

  E2 recall (injected errors found, any overlapping flag):

  | tool | recall | agreement | article | homophone | repetition | spelling (all) |
  |---|---|---|---|---|---|---|
  | flagcheck r1 / r2 / r3 | 0.72 / 0.78 / 0.75 | 30-36 /55 | 13-17 /25 | 18-22 /26 | 15-19 /25 | 119-128 /147 |
  | harper-us | 0.42 | 8/55 | 16/25 | 7/26 | 10/25 | 76/147 |
  | harper-tuned | 0.42 | 8/55 | 16/25 | 7/26 | 10/25 | 76/147 |

  E1 flags per 1,000 prose words on the clean papers (not yet labelled):

  | tool | raw | masked |
  |---|---|---|
  | flagcheck (mean of 3) | 3.9 | 3.9 |
  | harper-tuned | 22.0 | 16.0 |
  | harper-us | 40.5 | 29.1 |

- Harper stops partway on 4 papers (last lint at 0 %, 40 %, 60 %, 89 % of
  the file); 54 of the 278 injected errors lie after that point. Example:
  2609.39222, nothing after line 19 of 898, where author lines contain
  `\texorpdfstring{$^{1*}$}{}`. Cause not confirmed (likely an unbalanced
  `$` read as maths). Restricted to the text Harper checks, its recall is
  about 117/224 = 0.52. Report both.
- Run-to-run stability of flagcheck on the clean papers is low: mean
  Jaccard overlap of flag sets between runs 0.105; per paper, runs share
  2 of 10, 9 of 19-27, 4 of 22-29, 16 of 42-46 flags. Recall varies less
  (0.72-0.78). Unchanged paragraphs are cached in the editor, so users see
  stable flags; the benchmark sees the variance. Motivates voting (E4).
- flagcheck raised fewer non-gold flags on the injected copies
  (115-165) than on the clean copies (230-290). Not explained yet.
- Labelling sheet: `bench/runs/r1/sheet.csv`, 176 items (60 sampled per
  tool from masked clean flags, overlapping spans merged, shuffled,
  seed 1). Pool sizes: flagcheck 794 (3 runs), harper-tuned 1,089,
  harper-us 1,983. Key in `sheet_key.json` (labeller must not open).

## R1b - LTeX+ added to R1 (2026-11-18, Claude)

- Commit 8345871. ltex-ls-plus 18.7.0 (bundled Java), driven through its
  language server (`bench/ltex.py`) for exact ranges; the CLI prints only
  start positions. Check: language-server diagnostic counts match the CLI.
- `ltex-default` = en-US, no dictionary. `ltex-tuned` = en-US or en-GB,
  whichever gives fewer diagnostics on the clean paper (en-GB for 2
  papers, the same 2 as Harper), plus a dictionary of words flagged as
  misspellings at least twice in the clean paper (8-37 words). Cost $0.

  | tool | recall | clean flags / 1k words (masked) |
  |---|---|---|
  | flagcheck r1 / r2 / r3 | 0.72 / 0.78 / 0.75 | 4.2 / 3.4 / 4.0 |
  | ltex-default | 0.76 | 22.7 |
  | ltex-tuned | 0.76 | 10.4 |
  | harper-tuned | 0.42 | 16.0 |
  | harper-us | 0.42 | 29.1 |

  Recall by type, flagcheck (3 runs) vs LTeX+:

  | type | flagcheck | LTeX+ |
  |---|---|---|
  | agreement | 29-36 /55 | 12/55 |
  | homophone | 18-22 /26 | 16/26 |
  | article | 13-17 /25 | 25/25 |
  | repetition | 15-19 /25 | 22/25 |
  | spelling (3 operators) | 119-128 /147 | 135/147 |

- Overall recall is the same as LTeX+. LTeX+ wins on mechanical errors
  (a/an, doubled words, non-words); flagcheck wins on agreement. The tools
  miss different errors, so a union would beat both (not yet computed).
- flagcheck missing a/an errors that a rule catches 25/25 is unexpected;
  inspect the misses before the next run.
- The recall advantage over rule-based checking shown against Harper does
  not hold against LTeX+, the proper LaTeX baseline. What remains to test:
  precision on clean text (sheet), and setup cost (default vs tuned).
- Sheet rebuilt with LTeX+: `bench/runs/r1/sheet.csv`, 239 items, 50
  sampled per tool (flagcheck pooled over 3 runs), seed 1. Pool sizes:
  flagcheck 794, ltex-tuned 712, harper-tuned 1,089, ltex-default 1,548,
  harper-us 1,983.
- a/an misses (flagcheck r1, 12 of 25): genuine misses, not anchoring
  failures. No flag near 10 of them ("a explicit", "an code-hosting",
  "an policy model"); for 2 the model flagged the next word instead
  (`single`, `fair outcome`), which the overlap rule counts as a miss.
  Hypothesis: 16 paragraphs per request makes the model skim. Test batch
  size in E4; consider a lenient "adjacent token" match as a secondary
  score.
