//! `flagcheck`            run the language server on stdio
//! `flagcheck check FILE` check files once and print flags (for testing and
//!                        for the benchmark harness)

use anyhow::{Context as _, Result, bail};
use flagcheck::config::Settings;
use flagcheck::doc::Doc;
use flagcheck::markup::Lang;
use flagcheck::position::Encoding;
use flagcheck::{backend, checker};
use futures::StreamExt;

const USAGE: &str = "\
usage:
  flagcheck [--stdio]
  flagcheck check [options] FILE...
  flagcheck segment FILE     paragraphs and mask as JSON byte ranges

options:
  --backend anthropic|openai|mock
  --model NAME
  --effort low|medium|high
  --context document|paragraph
  --votes K --vote-min M
  --settings JSON      any setting, e.g. '{\"base_url\":\"http://localhost:11434/v1\"}'
  --format NAME        markdown|latex|typst|unknown, instead of the file extension
  --json               JSON lines with byte offsets
  --verbose            also print dropped quotes and usage
  --log PATH           append raw model replies, usage and drops as JSON lines";

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("--stdio") => {
            serve().await;
            Ok(())
        }
        Some("check") => check(&args[1..]).await,
        Some("segment") => segment(&args[1..]),
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

/// Print the segmentation used by the checker, for the benchmark (errors are
/// injected only into prose) and for debugging the scanners.
fn segment(args: &[String]) -> Result<()> {
    let file = args.first().with_context(|| USAGE.to_string())?;
    let text = std::fs::read_to_string(file).with_context(|| format!("reading {file}"))?;
    let lang = Lang::from_path(file).with_context(|| format!("unknown file type: {file}"))?;
    let seg = flagcheck::markup::segment(lang, &text);
    let r = |v: &[std::ops::Range<usize>]| v.iter().map(|r| [r.start, r.end]).collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::json!({
            "lang": lang.name(),
            "paragraphs": r(&seg.paragraphs),
            "mask": r(&seg.mask),
            "includes": seg.includes,
        })
    );
    Ok(())
}

async fn check(args: &[String]) -> Result<()> {
    let mut settings = serde_json::Map::new();
    let mut files = vec![];
    let (mut json, mut verbose) = (false, false);
    let mut log: Option<std::fs::File> = None;
    let mut format: Option<Lang> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = |name: &str| -> Result<String> {
            it.next()
                .cloned()
                .with_context(|| format!("{name} needs a value"))
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
            "--format" => {
                let v = val(a)?;
                format = Some(Lang::from_language_id(&v).with_context(|| format!("unknown format {v}"))?);
            }
            "--verbose" => verbose = true,
            "--log" => {
                let p = val(a)?;
                log = Some(
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&p)
                        .with_context(|| format!("opening {p}"))?,
                );
            }
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
        let lang = match format {
            Some(l) => l,
            None => Lang::from_path(&file).with_context(|| format!("unknown file type: {file}"))?,
        };
        let path = std::fs::canonicalize(&file).ok();
        let mut doc = Doc::new(lang, path, text, 0);
        let ids: Vec<usize> = (0..doc.seg.paragraphs.len()).collect();
        let groups: Vec<(Option<String>, Vec<usize>)> = match settings.context {
            flagcheck::config::Context::Document => doc
                .contexts(&ids, settings.max_context_tokens)
                .into_iter()
                .map(|(c, g)| (Some(c), g))
                .collect(),
            flagcheck::config::Context::Paragraph => vec![(None, ids)],
        };
        let batches: Vec<(Option<String>, Vec<usize>)> = groups
            .into_iter()
            .flat_map(|(c, g)| {
                g.chunks(settings.batch.max(1))
                    .map(|ch| (c.clone(), ch.to_vec()))
                    .collect::<Vec<_>>()
            })
            .collect();
        // As in the server: the first batch for each context runs alone so
        // it writes the prompt cache, then the rest run in parallel.
        let run_one = |context: Option<String>, chunk: Vec<usize>| {
            let (backend, settings, doc) = (&backend, &settings, &doc);
            async move {
                checker::check(backend.as_ref(), settings, lang, context, &doc.text, &doc.seg, &chunk).await
            }
        };
        let mut results = Vec::with_capacity(batches.len());
        let mut k = 0;
        while k < batches.len() {
            let ctx = &batches[k].0;
            let end = (k..batches.len()).find(|&j| &batches[j].0 != ctx).unwrap_or(batches.len());
            results.push(run_one(batches[k].0.clone(), batches[k].1.clone()).await?);
            let rest: Vec<_> = futures::stream::iter(
                batches[k + 1..end].iter().map(|(c, ch)| run_one(c.clone(), ch.clone())),
            )
            .buffered(settings.concurrency.max(1))
            .collect()
            .await;
            for r in rest {
                results.push(r?);
            }
            k = end;
        }
        for ((context, chunk), out) in batches.iter().zip(results) {
            let chunk = chunk.as_slice();
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
            if let Some(f) = log.as_mut() {
                use std::io::Write;
                writeln!(
                    f,
                    "{}",
                    serde_json::json!({
                        "file": file,
                        "settings": settings.fingerprint(),
                        "paragraphs": chunk,
                        "context_bytes": context.as_ref().map(|c| c.len()),
                        "latency_ms": out.latency_ms,
                        "usage": out.replies.iter().map(|r| r.usage.clone()).collect::<Vec<_>>(),
                        "raw": out.replies.iter().map(|r| r.raw.clone()).collect::<Vec<_>>(),
                        "dropped": out.dropped,
                    })
                )?;
            }
            if verbose {
                for d in &out.dropped {
                    eprintln!(
                        "dropped ({}): paragraph {} {:?}",
                        d.why, d.paragraph, d.quote
                    );
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
