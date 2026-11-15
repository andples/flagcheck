//! `flagcheck`            run the language server on stdio
//! `flagcheck check FILE` check files once and print flags (for testing and
//!                        for the benchmark harness)

use anyhow::{Context as _, Result, bail};
use flagcheck::config::Settings;
use flagcheck::doc::Doc;
use flagcheck::markup::Lang;
use flagcheck::position::Encoding;
use flagcheck::{backend, checker};

const USAGE: &str = "\
usage:
  flagcheck [--stdio]
  flagcheck check [options] FILE...

options:
  --backend anthropic|openai|mock
  --model NAME
  --effort low|medium|high
  --context document|paragraph
  --votes K --vote-min M
  --settings JSON      any setting, e.g. '{\"base_url\":\"http://localhost:11434/v1\"}'
  --json               JSON lines with byte offsets
  --verbose            also print dropped quotes and usage";

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("--stdio") => {
            serve().await;
            Ok(())
        }
        Some("check") => check(&args[1..]).await,
        Some("--version") => {
            println!("flagcheck {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}

async fn serve() {
    // Logs go to stderr; stdout is the LSP channel.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
    let (service, socket) = tower_lsp_server::LspService::new(flagcheck::server::Server::new);
    tower_lsp_server::Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;
}

async fn check(args: &[String]) -> Result<()> {
    let mut settings = serde_json::Map::new();
    let mut files = vec![];
    let (mut json, mut verbose) = (false, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = |name: &str| -> Result<String> {
            it.next().cloned().with_context(|| format!("{name} needs a value"))
        };
        match a.as_str() {
            "--backend" => _ = settings.insert("backend".into(), val(a)?.into()),
            "--model" => _ = settings.insert("model".into(), val(a)?.into()),
            "--effort" => _ = settings.insert("effort".into(), val(a)?.into()),
            "--context" => _ = settings.insert("context".into(), val(a)?.into()),
            "--votes" => _ = settings.insert("votes".into(), val(a)?.parse::<u64>()?.into()),
            "--vote-min" => _ = settings.insert("vote_min".into(), val(a)?.parse::<u64>()?.into()),
            "--settings" => {
                let v: serde_json::Value = serde_json::from_str(&val(a)?)?;
                for (k, v) in v.as_object().context("--settings must be an object")? {
                    settings.insert(k.clone(), v.clone());
                }
            }
            "--json" => json = true,
            "--verbose" => verbose = true,
            f if f.starts_with("--") => bail!("unknown option {f}\n{USAGE}"),
            f => files.push(f.to_string()),
        }
    }
    if files.is_empty() {
        bail!("{USAGE}");
    }
    let settings: Settings = serde_json::from_value(settings.into()).context("bad settings")?;
    let backend = backend::build(&settings)?;

    for file in files {
        let text = std::fs::read_to_string(&file).with_context(|| format!("reading {file}"))?;
        let lang = Lang::from_path(&file).with_context(|| format!("unknown file type: {file}"))?;
        let path = std::fs::canonicalize(&file).ok();
        let mut doc = Doc::new(lang, path, text, 0);
        let context = match settings.context {
            flagcheck::config::Context::Document => Some(doc.context()),
            flagcheck::config::Context::Paragraph => None,
        };
        let ids: Vec<usize> = (0..doc.seg.paragraphs.len()).collect();
        for chunk in ids.chunks(settings.batch.max(1)) {
            let out = checker::check(
                backend.as_ref(),
                &settings,
                lang,
                context.clone(),
                &doc.text,
                &doc.seg,
                chunk,
            )
            .await?;
            for (i, flags) in &out.flags {
                let base = doc.seg.paragraphs[*i].start;
                for f in flags {
                    let (s, e) = (base + f.range.start, base + f.range.end);
                    let p = doc.index.position(&doc.text, s, Encoding::Utf32);
                    if json {
                        println!(
                            "{}",
                            serde_json::json!({
                                "file": file, "start": s, "end": e,
                                "line": p.line + 1, "col": p.character + 1,
                                "quote": f.quote, "category": f.category, "reason": f.reason,
                            })
                        );
                    } else {
                        println!(
                            "{file}:{}:{}: {}: {}  [{}]",
                            p.line + 1,
                            p.character + 1,
                            f.category.label(),
                            f.reason.as_deref().unwrap_or("-"),
                            f.quote.replace('\n', " ")
                        );
                    }
                }
            }
            if verbose {
                for d in &out.dropped {
                    eprintln!("dropped ({}): paragraph {} {:?}", d.why, d.paragraph, d.quote);
                }
                for r in &out.replies {
                    if let Some(u) = &r.usage {
                        eprintln!("usage: {u}  latency: {} ms", out.latency_ms);
                    }
                }
            }
        }
    }
    Ok(())
}
