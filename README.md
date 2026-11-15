# flagcheck (working name)

An LLM spelling and grammar checker for Markdown, LaTeX and Typst that runs as
a language server. It underlines; it never suggests replacement text.

Status: early, not released. See [DESIGN.md](DESIGN.md).

Checked text is sent to the configured model provider. Use a local model
(`backend: "openai"` with an Ollama `base_url`) to keep it on your machine,
or put `flagcheck: off` in the first five lines of a file to skip it.

## Install

```sh
cargo install --path server
```

## Zed

1. Install the LaTeX and Typst extensions if you need those languages.
2. Extensions → Install Dev Extension → choose the `zed/` folder
   (needs Rust from rustup).
3. Make the API key visible to the shell Zed starts from:
   `export ANTHROPIC_API_KEY=...`
4. Optional settings (`~/.config/zed/settings.json`):

```json
{
  "lsp": {
    "flagcheck": {
      "initialization_options": {
        "backend": "anthropic",
        "effort": "low",
        "reasons": "short"
      }
    }
  }
}
```

`"backend": "mock"` runs without a key or network and flags a few fixed
misspellings, for checking the setup.

Local model through Ollama:

```json
"initialization_options": {
  "backend": "openai",
  "base_url": "http://localhost:11434/v1",
  "model": "qwen3:8b"
}
```

All settings are listed in [DESIGN.md](DESIGN.md#settings).

## Command line

```sh
flagcheck check paper.tex
flagcheck check --backend mock --json notes.md
```

## License

MIT
