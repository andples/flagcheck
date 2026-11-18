# Labelling guidelines (E1 precision)

Each row of a sheet is one flagged span in a published paper's source,
shown as `... ⟦span⟧ ...` with about 140 bytes of context on each side.
Line breaks are shown as `⏎`. The tools that raised it are hidden. Label
the span, not the tool's message (messages are not shown).

| label | meaning | examples |
|---|---|---|
| G | Genuine error: a careful copy editor would change it. | `the RIS can transforms`, `a explicit`, misspelled word, doubled word, missing word |
| O | Optional: defensible either way, or style only. | Oxford comma, `data is`, long sentence, `e.g.` without comma, British vs American spelling, capitalisation in headings |
| F | False: the text is correct as written. | correct technical term, name, acronym, correct grammar flagged |
| M | Markup artefact: the span is (mostly) markup, not prose. | macro names, citation keys, maths, `\\`, table `&`, preamble options |

Rules:

- Judge the prose as the author's final text would read when typeset:
  `\emph{word}` reads as `word`, `~` is a space, `--` is a dash.
- If the span covers an error but is much wider than it, it is still G.
- If the span is next to an error but does not include it, label by what
  it does include (usually F), and note "adjacent error".
- Errors in the context outside the span do not count.
- When unsure between G and O, choose O. When unsure between O and F,
  choose O.
- The `note` column is free text; use it for anything ambiguous.

Precision is reported two ways: strict = G / all, lenient = (G + O) / all.
M counts against the tool in both.
