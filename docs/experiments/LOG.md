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

## S1 - Batch-size sweep (2026-11-18, Claude)

- Commit efe8d12 (CLI now sends a file's batches in parallel after the
  first per context; same requests and settings otherwise, checked
  identical output with the mock backend). Defaults except `batch`. One
  run each. Batch 16 = R1 (3 runs). Batch 1 on the injected copies only,
  to limit cost.

  | paragraphs per request | recall | clean flags / 1k | non-gold flags on injected | requests | cost |
  |---|---|---|---|---|---|
  | 16 (R1, 3 runs) | 0.72-0.78 | 3.4-4.2 | 115-165 | 564 (3 runs) | $0.48 (3 runs) |
  | 8 | 0.80 | 5.2 | 190 | 366 | $0.27 |
  | 4 | 0.89 | 6.8 | 288 | 722 | $0.32 |
  | 1 (injected only) | 0.93 | - | 439 | 1,423 | $0.50 |
  | LTeX+ tuned, for reference | 0.76 | 10.4 | 708 | - | - |

  Recall by type:

  | type | b16 (3 runs) | b8 | b4 | b1 | LTeX+ |
  |---|---|---|---|---|---|
  | agreement | 29-36 | 40 | 45 | 48 | 12 /55 |
  | article | 13-17 | 13 | 18 | 20 | 25 /25 |
  | homophone | 18-22 | 17 | 20 | 22 | 16 /26 |
  | repetition | 15-19 | 19 | 23 | 25 | 22 /25 |
  | spelling | 119-128 | 133 | 140 | 144 | 135 /147 |

- Hypothesis from R1b confirmed: with many paragraphs per request the
  model skims. Recall rises steadily as batches shrink; at 4 or fewer it is
  well above LTeX+ overall and on agreement (45-48 vs 12), level or close
  on everything else except a/an (18-20 vs 25).
- The cost: more flags. Non-gold flags on the injected copies rise from
  ~130 to 439; clean flags from ~4 to 6.8 per 1k words at batch 4. Whether
  the extra flags are genuine is unknown until labelled.
- Cache writes were 0 for b4 and b1 because the previous run had just
  warmed the same prefixes; a cold run costs slightly more.
- Live editing already sends only the changed paragraph (batch 1); only a
  file's first check uses batch 16. So editor behaviour is closer to b1
  than to the R1 default.
- Single runs; R1's run-to-run spread at b16 was 0.72-0.78, far smaller
  than the trend.
- Batch 1 on the clean copies, run afterwards (same commit): 560 flags,
  8.2 per 1k words; 1,423 requests, $0.53 (both batch-1 runs together
  $1.03). Full row: batch 1 = recall 0.93, 8.2 clean flags per 1k words,
  against LTeX+ tuned 0.76 and 10.4. Clean flag rate by batch size:
  16: 3.4-4.2, 8: 5.2, 4: 6.8, 1: 8.2.

## E6a - Invented markup format (2026-11-18, Claude)

- Commits 6ffc2c5 (`bench/invent.py`), 2c7948c (runner). Run
  `bench/runs/r2-invented/`, corpus `invented`.
- The 10 papers (clean and injected) rewritten mechanically into a markup
  no model has seen: every LaTeX command name becomes a fixed nonsense
  word (`\cite` -> `◆vutor`, `\section` -> `◆rimelka`, `\emph` ->
  `◆wimgu`), braces -> `⟨⟩`, option brackets -> `⟦⟧`, `$` -> `¤`, `&` ->
  `¦`, `%` -> `;;`. Prose bytes unchanged; all 278 gold spans mapped and
  verified byte-for-byte.
- flagcheck: new `unknown` format (paragraphs at blank lines, no mask,
  format named "unknown-format" in the prompt), batch 4, otherwise
  defaults, 1 run. $0.47 (680 requests incl. cache writes). Harper and
  LTeX+ in plain-text mode (Harper: "could not detect language ID; falling
  back to PlainEnglish"; LTeX+: languageId plaintext), default and tuned
  as in R1.

  | tool | corpus | recall | clean flags / 1k words | clean flags at markup |
  |---|---|---|---|---|
  | flagcheck b4 | LaTeX (mask) | 0.89 | 6.8 | - |
  | flagcheck b4 | invented (no mask) | 0.78 | 7.6 | 15 % |
  | LTeX+ tuned | LaTeX | 0.76 | 10.4 (masked) | - |
  | LTeX+ tuned | invented | 0.79 | 41.8 | 65 % |
  | LTeX+ default | invented | 0.79 | 178.2 | 81 % |
  | Harper tuned | invented | 0.83 | 63.4 | 58 % |
  | Harper default | invented | 0.83 | 171.6 | 65 % |

  "At markup" = an invented-markup character within 3 bytes of the span
  (a proxy for markup artefacts until labelled).

- On a format nobody has a parser for, rule-based checkers flag the markup:
  LTeX+ goes from 10 to 42-178 flags per 1k words, Harper to 63-172, most
  of them at markup. flagcheck goes from 6.8 to 7.6, 15 % at markup.
- flagcheck's recall drops from 0.89 to 0.78 (agreement 45 -> 36, a/an
  18 -> 14, spelling 140 -> 126 of 147). Baseline recall rises (Harper
  0.42 -> 0.83) because in plain-text mode they check every line,
  including the parts Harper skipped in LaTeX; their findings are buried
  in 4-17x more flags.
- Single runs.

## A1 - Mask ablation on LaTeX (2026-11-18, Claude)

- Commit 2c7948c. flagcheck batch 4 on the real LaTeX corpus with
  `--format unknown` (no scanner, no mask), 1 run, $0.39. Run
  `bench/runs/r1/fc-b4-nomask-r1`.

  | flagcheck b4 on LaTeX | recall | clean flags / 1k (raw) | after applying the mask |
  |---|---|---|---|
  | with scanner and mask | 0.89 | 6.8 | 6.8 |
  | no scanner, no mask | 0.84 | 6.55 | 6.1 |

- The mask removes only about 0.45 flags per 1k words: the model itself
  ignores LaTeX markup almost entirely. Most of flagcheck's markup
  robustness comes from the model, not the hand-written scanner.
- Recall falls 0.89 -> 0.84 without the scanner, probably because
  paragraphs are then split only at blank lines (headings, list items,
  environments merge into bigger units). Not tested further.
- Together with E6a: on LaTeX the model alone loses 5 points of recall; on
  an unseen format, 11 points, while flag volume stays flat.

## L1 - Provisional precision labels from blind Claude labellers (2026-11-18, Claude)

- Commit c6673d4. Sheet `bench/runs/r1/sheet.csv` rebuilt with 30 random
  clean-paper flags per tool (9 tools: flagcheck b16 pooled over 3 runs,
  b8, b4, b1, b4 no-mask, LTeX+ default/tuned, Harper default/tuned),
  merged where spans overlap: 265 items. Guidelines:
  `docs/experiments/labelling.md`.
- Labeller A: a Claude subagent (Claude Code, subscription, not the API
  key) that read only the guidelines and the sheet, labelled all 265.
  Labeller B: a second, independent subagent, same rules, labelled a
  random 79-item subset (seed 7). Neither saw tool names. Cost $0 API.
- Agreement A vs B on 79 items: raw 0.89, Cohen's kappa 0.77 (four
  labels), 0.82 (G+O vs F+M).
- Precision per tool from its own 30-flag sample (labeller A):

  | tool | G | O | F | M | strict | lenient |
  |---|---|---|---|---|---|---|
  | flagcheck b16 (3 runs) | 0 | 1 | 25 | 4 | 0.00 | 0.03 |
  | flagcheck b8 | 1 | 4 | 24 | 1 | 0.03 | 0.17 |
  | flagcheck b4 | 1 | 2 | 26 | 1 | 0.03 | 0.10 |
  | flagcheck b1 | 1 | 6 | 22 | 1 | 0.03 | 0.23 |
  | flagcheck b4 no mask | 3 | 1 | 24 | 2 | 0.10 | 0.13 |
  | LTeX+ tuned | 1 | 9 | 14 | 6 | 0.03 | 0.33 |
  | LTeX+ default | 0 | 9 | 17 | 4 | 0.00 | 0.30 |
  | Harper tuned | 0 | 3 | 9 | 18 | 0.00 | 0.10 |
  | Harper default | 0 | 5 | 14 | 11 | 0.00 | 0.17 |

  95 % Wilson intervals are wide (n = 30; e.g. 0.10 -> 0.03-0.26). Full
  output: `bench/runs/r1/precision-labels-claude-a.json`.
- Audit (Claude, not blind, 14 flagcheck items labelled F): the labels
  hold. Typical false flags are correct sentences: "which obtains",
  "it increases", "correctly identify", "decouples". This is LLM
  over-correction appearing in detection.
- Clean published papers contain few real errors, so precision is low for
  every tool; the comparable number is non-genuine flags per 1k words
  (rate x (1 - lenient)): flagcheck b16 ~3.8, b4 ~6.1, b1 ~6.3;
  LTeX+ tuned ~7.0; Harper tuned ~14.4.
- Caveat: LLM labellers judging an LLM tool; to be checked against the
  owner's labels on the same sheet.

## V1 - Voting, simulated on R1's three batch-16 runs (2026-11-18, Claude)

- No new API calls. A flag is kept if overlapping flags appear in at
  least k of the 3 runs (mask filter applied to clean flags).

  | k of 3 | recall | clean flags / 1k | labelled flagcheck sample kept |
  |---|---|---|---|
  | 1 (union) | 0.88 | 9.7 | 30/30 (25 F, 4 M, 1 O) |
  | 2 | 0.80 | 1.55 | 4/30 (3 F, 1 O) |
  | 3 | 0.58 | 0.38 | 0/30 |

- 2-of-3 voting keeps recall (0.80, above every single run's 0.72-0.78)
  while cutting clean flags by about 60 % and removing 26 of the 30
  labelled flags, nearly all false. False flags are mostly one-off; real
  errors are flagged consistently (matches the R1 Jaccard of 0.105).
- Next: batch 4 with 3 runs, to measure voting where single-run recall is
  highest.
