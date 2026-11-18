# Design

`flagcheck` is a working name.

A language server that underlines likely spelling and grammar errors in
Markdown, LaTeX and Typst using an LLM. It never proposes replacement text.
It points; the writer fixes.

## Layout

```
server/   the language server (Rust crate, binary `flagcheck`)
zed/      Zed extension (Rust -> WASM); starts `flagcheck` from PATH
bench/    benchmark harness for the paper experiments (later)
```

## Flag-only contract

These rules are the point of the tool. Each is enforced in code, not only in
the prompt.

1. A diagnostic carries a span, a category and, optionally, a short reason.
   Never a correction.
2. `sanitize.rs` removes anything from a reason that could carry the fix:
   quoted text, text after phrases such as "should be", "instead", "replace",
   arrows, and any word that is not in the paragraph but is a close edit of
   the flagged quote. If a reason cannot be cleaned it is dropped and only the
   category is shown. Setting `reasons = "none"` removes reasons entirely.
3. No code action edits the document. The only code actions are
   "ignore this flag" and "add word to word list", both of which run server
   commands that change the server's own state files.
4. The prompt asks for detection only. The model is never asked to rewrite.

## Pipeline

```
didChange -> debounce (1.5 s) -> segment -> hash paragraphs
  -> cached? reuse flags : queue paragraph
  -> backend.check(snapshot, queued paragraphs)
  -> anchor quotes -> mask filter -> sanitize -> cache by hash
  -> publish merged diagnostics for all paragraphs
```

**Segmentation** (`markup/`). Each language has a small hand-written scanner
that yields paragraphs (byte ranges of prose in the source) and a mask (byte
ranges that are never prose: maths, code, citations, labels, refs, macros
names, URLs, front matter, comments). Raw source is sent to the model; the
mask is applied afterwards, so a flag that overlaps masked bytes is dropped.
LaTeX `\input`/`\include` are followed to build whole-document context.

**Anchoring** (`anchor.rs`). The model returns quotes, not offsets, because
models are bad at counting characters. The server finds each quote inside the
paragraph it was reported for: exact match, then disambiguation with the
model's `before` text, then whitespace-insensitive match. Quotes that are not
found, or found more than once with no way to choose, are dropped and logged.
In evaluation they count as false positives.

**Positions** (`position.rs`). Internal offsets are UTF-8 bytes. LSP
positions are converted to the negotiated encoding (UTF-16 by default, UTF-8
if the client offers it).

**Caching.** Flags are cached per paragraph by a hash of
(paragraph text, backend, model, prompt version, settings). Unchanged
paragraphs never go back to the model. Every publish sends the merged set for
the whole document, because each `publishDiagnostics` replaces the last.

**Staleness.** Each check records the document version. When results arrive
for an older version, each paragraph's flags are kept only if that
paragraph's text is unchanged (they are re-anchored by hash); the rest are
discarded and the paragraph is re-queued.

**Prompt caching.** Request layout, stable first:

1. system prompt (fixed per prompt version)
2. document snapshot, with a cache breakpoint
3. the paragraphs to check, with ids

The snapshot is the whole document at some recent time, not at every
keystroke: it is refreshed when more than 20 % of its paragraphs have
changed, or after 30 minutes. Reads keep the prompt cache alive while
typing, so a rewrite (a full-price cache write) is rarely needed.

Above `max_context_tokens` (default 60k, estimated at 3 bytes per token)
the snapshot is cut into fixed overlapping windows, one starting every half
window and cut at blank lines; each paragraph is checked with the window it
starts in. Requests stay under Haiku's 100k-token price step, and each
window is its own stable cached prefix. Changed paragraphs are always sent in
their current form in part 3. Haiku 5.5 does not cache prefixes below 512
tokens; short documents simply go uncached.

**First batch alone.** With document context, the first request of a full
check runs on its own so it writes the prompt cache; the other batches then
run in parallel and read it.

**Voting.** Optional: run k samples, keep flags whose anchored spans overlap
in at least m of them. Default k = 1.

## Backends

`Backend` trait, one request -> list of raw flags.

| backend     | endpoint                      | notes                              |
|-------------|-------------------------------|------------------------------------|
| `anthropic` | `POST /v1/messages`           | default, `claude-haiku-5-5`        |
| `openai`    | `POST {base}/chat/completions`| OpenAI, compatible hosts, Ollama   |
| `mock`      | none                          | deterministic, for tests and demos |

Anthropic request: `output_config.format` JSON schema, `output_config.effort`
(default `low`), thinking disabled by default (allowed at effort `high` and
below; with thinking, live checks took 4-9 s instead of 1.6-3 s), no sampling parameters (Haiku 5.5 rejects them). API keys are read
from environment variables only and never written to logs.

## Model output schema

```json
{"flags": [{"paragraph": 3, "quote": "teh", "before": "and ",
            "category": "spelling", "reason": "misspelled word"}]}
```

Categories: `spelling`, `grammar`, `agreement`, `tense`, `article`,
`preposition`, `punctuation`, `word-choice`, `repetition`, `missing-word`,
`extra-word`. There is no `other`: it invited style comments, and replies
that use an unknown category are dropped.

## Settings

Passed as LSP `initializationOptions` (Zed: `lsp.flagcheck.initialization_options`).

| key            | default             |
|----------------|---------------------|
| `backend`      | `"anthropic"`       |
| `model`        | `"claude-haiku-5-5"`|
| `base_url`     | backend default     |
| `api_key_env`  | `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` |
| `effort`       | `"low"`             |
| `thinking`     | `"disabled"` (`"adaptive"`) |
| `debounce_ms`  | `1500`              |
| `reasons`      | `"short"` (`"category"`, `"none"`) |
| `severity`     | `"information"` (`"hint"`) |
| `votes`        | `1`                 |
| `vote_min`     | `1`                 |
| `context`      | `"document"` (`"paragraph"`; `"sentence"` planned for E4) |
| `batch`        | `16` paragraphs per request |
| `concurrency`  | `4` requests in flight |
| `max_tokens`   | `8000`              |
| `max_context_tokens` | `60000`       |
| `json_mode`    | `"schema"` (`"object"`, `"none"`; OpenAI-compatible only) |
| `log_path`     | unset (JSONL of requests/responses for evaluation) |

Per-project state lives in `.flagcheck/` at the workspace root:
`words.txt` (word list) and `ignored.txt` (ignored flags, by paragraph hash
and quote). Opt-out per file with a `flagcheck: off` comment in the first
lines.

## Benchmark layout (later)

```
bench/
  corpus/      manifest of source files (arXiv id, version, licence, hash)
  inject/      rule-based error injection with gold offsets in source
  run/         runs any tool over the corpus, writes flags as JSONL
  score/       span overlap scoring, P/R/F0.5, bootstrap CIs, kappa
```

Raw model responses are kept so results can be re-scored after a model
retires.
