# Pilot: one Typst file, flagcheck vs Harper (2026-11-18)

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
