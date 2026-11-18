# Pilot: one Typst file, flagcheck vs Harper (2026-11-18)

> Superseded by [LOG.md](LOG.md) (entries P1-P5), which also records commits and settings.

Not a result. One file, one run each, labelled by Claude, not blind.

File: the owner's DeepRL reading notes (Typst, 54 lines, British spelling,
maths and RL acronyms). Not committed. 19 genuine errors: 15 misspellings,
4 grammar (`stats` for `states`, `a real rewards`, missing `it` in
"is instead up to", `agents` for `agents'`). Two lowercase proper nouns
(`atari`, `markovian`) counted as optional.

| System | Flags | Genuine | Optional | False | Precision | Recall |
|---|---|---|---|---|---|---|
| flagcheck, Haiku 5.5, prompt v1, effort low | 18 | 14 | 0 | 4 | 78 % | 14/19 |
| Harper 3.2.0, default (US) | 40 | 14 | 2 | 24 | 35-40 % | 14/19 |
| Harper 3.2.0, British | 37 | 14 | 2 | 21 | 38-43 % | 14/19 |
| Harper 3.2.0, British + 8-word jargon dictionary | 18 | 14 | 2 | 2 | 78-89 % | 14/19 |

- Harper's false flags were acronyms (DQN x7, DRQN x3, POMDP x3, MDP x2,
  NN, RMSPROP, ADADELTA, DeepRL) and two style rules (long sentence,
  lowercase list item). Nothing inside Typst maths was flagged by either.
- flagcheck's false flags: `optimisers` (British spelling), `million`
  ("unclear wording"), `flickering`, `Stable`. Prompt v2 targets the first
  two and the style categories.
- Misses. flagcheck: approximatior, Reccurent (heading), one Reccurency,
  `a real rewards`, `agents`. Harper: updatind and all 4 grammar errors.
- Same recall, different errors: flagcheck found 2 of 4 grammar errors,
  Harper none; Harper found 3 misspellings flagcheck missed.

Reading: untuned, the LLM raises about half the false flags. With a small
dictionary, Harper matches its precision on spelling. The case for the LLM
is then grammar plus no setup, not raw precision. E1 must report tuned
baselines; this pilot shows why.

## Prompt v2 sweep (same file, 3 runs each, same labelling)

| Config | Found (of 19) | False | Precision |
|---|---|---|---|
| effort low, thinking off | 17 / 17 / 16 | 7 / 7 / 7 | ~70 % |
| low, thinking off, 2-of-3 vote (simulated) | 17 | 5 | 74-78 % |
| low, adaptive thinking | 10 / 11 / 16 | 1 / 1 / 4 | 76-92 % |
| medium, thinking off | 16 / 16 / 17 | 8 / 4 / 6 | 67-81 % |
| medium, thinking off + shape filters (simulated) | 16 / 16 / 17 | 5 / 2 / 4 | 76-89 % |

- Adaptive thinking: in 2 of 3 runs the last batch came back almost empty
  (thinking tokens 0, one placeholder flag). Unstable; kept off.
- Recurring false flags with thinking off: "Frame stacking" as spelling
  (every run), `randomises`/`generalise` as "spelling variant
  inconsistent" (prompt v2 rule ignored), whole-sentence quotes. The shape
  filters (multi-word spelling, variety reasons, quotes over 6 words)
  remove them; cost at most one genuine flag per run.
- Medium found 16 errors in every run (most stable); latency 1.2-2.6 s per
  request, same as low. New default: medium, thinking off, filters on.
- Never found in any config: `agents` for `agents'`. Rarely: the missing
  `it` in "is instead up to".
- Cost of all 15 runs: about 2 cents.

## Real runs of the new default (medium, thinking off, filters)

| Run | Found | Optional | False | Precision |
|---|---|---|---|---|
| 1 | 15 | 2 | 6 | 65-74 % |
| 2 | 15 | 1 | 4 | 71-76 % |

Worse than the simulation (filters applied to earlier outputs). The model
now quoted `Frame` alone, and several false flags carry a category that
contradicts the reason ("spelling: wrong word choice", "spelling:
unnecessary colon"). Run-to-run spread (about 65-80 % precision, 15-17
found) is as large as the differences between configurations, so one file
cannot rank them. Stop tuning on this file; tune on the benchmark.
