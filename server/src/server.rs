//! The language server.
//!
//! Flow per document: every change re-segments the text and immediately
//! republishes cached flags (so unchanged paragraphs keep their
//! underlines). After a typing pause the paragraphs with no cached result
//! are sent to the model in batches. Results are cached by paragraph text,
//! which also handles staleness: a result for text that has since changed
//! simply matches no current paragraph. Every publish sends the full merged
//! set, because each publishDiagnostics replaces the previous one.

use crate::backend::{self, Backend};
use crate::checker;
use crate::config::{Context, Settings, Severity};
use crate::doc::{Doc, fnv};
use crate::flag::{Category, Flag};
use crate::markup::Lang;
use crate::position::Encoding;
use crate::sanitize::ReasonMode;
use crate::store::Store;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Semaphore};
use tower_lsp_server::jsonrpc::Result as RpcResult;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer};

const CMD_IGNORE: &str = "flagcheck.ignore";
const CMD_ADD_WORD: &str = "flagcheck.addWord";
const SOURCE: &str = "flagcheck";
const CACHE_LIMIT: usize = 100_000;

pub struct Server {
    client: Client,
    state: Arc<Mutex<State>>,
}

struct State {
    settings: Settings,
    backend: Option<Arc<dyn Backend>>,
    backend_error: Option<String>,
    shown_error: Option<String>,
    enc: Encoding,
    root: Option<PathBuf>,
    store: Store,
    docs: HashMap<Uri, Doc>,
    /// Cache key -> flags. The key covers paragraph text, language and every
    /// setting that affects results.
    cache: HashMap<u64, Vec<Flag>>,
    inflight: HashSet<u64>,
    limit: Arc<Semaphore>,
}

fn cache_key(fingerprint: &str, lang: Lang, paragraph: &str) -> u64 {
    fnv(&[
        fingerprint.as_bytes(),
        lang.name().as_bytes(),
        paragraph.as_bytes(),
    ])
}

impl State {
    fn key(&self, lang: Lang, paragraph: &str) -> u64 {
        cache_key(&self.settings.fingerprint(), lang, paragraph)
    }

    fn configure(&mut self, settings: Settings) {
        self.limit = Arc::new(Semaphore::new(settings.concurrency.max(1)));
        match backend::build(&settings) {
            Ok(b) => {
                self.backend = Some(b);
                self.backend_error = None;
            }
            Err(e) => {
                self.backend = None;
                self.backend_error = Some(e.to_string());
            }
        }
        self.settings = settings;
    }

    fn diagnostics(&self, uri: &Uri) -> (Vec<Diagnostic>, Option<i32>) {
        let Some(d) = self.docs.get(uri) else {
            return (vec![], None);
        };
        if d.disabled {
            return (vec![], Some(d.version));
        }
        let severity = match self.settings.severity {
            Severity::Information => DiagnosticSeverity::INFORMATION,
            Severity::Hint => DiagnosticSeverity::HINT,
            Severity::Warning => DiagnosticSeverity::WARNING,
        };
        let mut out = vec![];
        for (i, r) in d.seg.paragraphs.iter().enumerate() {
            let par = &d.text[r.clone()];
            let flags = self
                .cache
                .get(&self.key(d.lang, par))
                .or_else(|| d.provisional.get(&i));
            let Some(flags) = flags else { continue };
            for f in flags {
                if self.store.is_ignored(d.hashes[i], &f.quote)
                    || (f.category == Category::Spelling && self.store.has_word(&f.quote))
                {
                    continue;
                }
                let abs = r.start + f.range.start..r.start + f.range.end;
                let message = match self.settings.reasons {
                    ReasonMode::Short => f
                        .reason
                        .clone()
                        .unwrap_or_else(|| f.category.label().into()),
                    ReasonMode::Category => f.category.label().into(),
                    ReasonMode::None => String::new(),
                };
                out.push(Diagnostic {
                    range: d.index.range(&d.text, abs, self.enc),
                    severity: Some(severity),
                    code: Some(NumberOrString::String(f.category.label().into())),
                    source: Some(SOURCE.into()),
                    message,
                    data: Some(json!({
                        "hash": format!("{:016x}", d.hashes[i]),
                        "quote": f.quote,
                        "category": f.category,
                    })),
                    ..Default::default()
                });
            }
        }
        (out, Some(d.version))
    }

    fn log(&self, record: Value) {
        let Some(p) = &self.settings.log_path else {
            return;
        };
        let path = match &self.root {
            Some(root) if !PathBuf::from(p).is_absolute() => root.join(p),
            _ => PathBuf::from(p),
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{record}");
        }
    }
}

impl Server {
    pub fn new(client: Client) -> Self {
        Server {
            client,
            state: Arc::new(Mutex::new(State {
                settings: Settings::default(),
                backend: None,
                backend_error: None,
                shown_error: None,
                enc: Encoding::Utf16,
                root: None,
                store: Store::default(),
                docs: HashMap::new(),
                cache: HashMap::new(),
                inflight: HashSet::new(),
                limit: Arc::new(Semaphore::new(4)),
            })),
        }
    }

    async fn publish(&self, uri: &Uri) {
        publish(&self.client, &self.state, uri).await;
    }

    /// Schedule a check after the debounce delay, unless the document
    /// changes again in the meantime.
    async fn schedule(&self, uri: Uri) {
        let (generation, delay) = {
            let st = self.state.lock().await;
            let Some(d) = st.docs.get(&uri) else { return };
            (d.generation, st.settings.debounce_ms)
        };
        let client = self.client.clone();
        let state = self.state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(delay)).await;
            let current = state.lock().await.docs.get(&uri).map(|d| d.generation);
            if current == Some(generation) {
                run(client, state, uri).await;
            }
        });
    }
}

async fn publish(client: &Client, state: &Arc<Mutex<State>>, uri: &Uri) {
    let (diags, version) = state.lock().await.diagnostics(uri);
    client
        .publish_diagnostics(uri.clone(), diags, version)
        .await;
}

/// Send every unchecked paragraph of `uri` to the backend, in batches.
async fn run(client: Client, state: Arc<Mutex<State>>, uri: Uri) {
    let batches = {
        let mut st = state.lock().await;
        let Some(backend) = st.backend.clone() else {
            let err = st.backend_error.clone().unwrap_or_default();
            if st.shown_error.as_ref() != Some(&err) {
                st.shown_error = Some(err.clone());
                drop(st);
                client
                    .show_message(MessageType::WARNING, format!("flagcheck: {err}"))
                    .await;
            }
            return;
        };
        let settings = st.settings.clone();
        let Some(d) = st.docs.get(&uri) else { return };
        if d.disabled {
            return;
        }
        let lang = d.lang;
        let pending: Vec<(usize, u64)> = (0..d.seg.paragraphs.len())
            .map(|i| (i, st.key(lang, d.paragraph(i))))
            .filter(|(_, k)| !st.cache.contains_key(k) && !st.inflight.contains(k))
            .collect();
        if pending.is_empty() {
            return;
        }
        for (_, k) in &pending {
            st.inflight.insert(*k);
        }
        let d = st.docs.get_mut(&uri).unwrap();
        let context = match settings.context {
            Context::Document => Some(d.context()),
            Context::Paragraph => None,
        };
        let job = Arc::new((
            d.text.clone(),
            d.seg.clone(),
            context,
            lang,
            settings.clone(),
            backend,
        ));
        pending
            .chunks(settings.batch.max(1))
            .map(|c| (job.clone(), c.to_vec()))
            .collect::<Vec<_>>()
    };

    // With document context, the first request writes the prompt cache.
    // Run it alone so the other batches read the cache instead of each
    // paying to write it again.
    let warm_first = batches.len() > 1 && batches[0].0.2.is_some();
    let mut all_ok = true;
    let mut tasks = vec![];
    for (n, (job, chunk)) in batches.into_iter().enumerate() {
        let client = client.clone();
        let state = state.clone();
        let uri = uri.clone();
        let handle = tokio::spawn(async move {
            let limit = state.lock().await.limit.clone();
            let _permit = limit.acquire_owned().await;
            let (text, seg, context, lang, settings, backend) = &*job;
            let ids: Vec<usize> = chunk.iter().map(|(i, _)| *i).collect();
            let result = checker::check(
                backend.as_ref(),
                settings,
                *lang,
                context.clone(),
                text,
                seg,
                &ids,
            )
            .await;
            let mut st = state.lock().await;
            for (_, k) in &chunk {
                st.inflight.remove(k);
            }
            let ok = match result {
                Ok(out) => {
                    if st.cache.len() > CACHE_LIMIT {
                        st.cache.clear();
                    }
                    for (i, k) in &chunk {
                        let flags = out.flags.get(i).cloned().unwrap_or_default();
                        st.cache.insert(*k, flags);
                    }
                    st.log(json!({
                        "time": unix_time(),
                        "uri": uri.as_str(),
                        "settings": settings.fingerprint(),
                        "paragraphs": ids,
                        "latency_ms": out.latency_ms,
                        "usage": out.replies.iter().map(|r| r.usage.clone()).collect::<Vec<_>>(),
                        "raw": out.replies.iter().map(|r| r.raw.clone()).collect::<Vec<_>>(),
                        "dropped": out.dropped,
                    }));
                    st.shown_error = None;
                    drop(st);
                    client
                        .log_message(MessageType::INFO, summary(&settings, &out, ids.len()))
                        .await;
                    true
                }
                Err(e) => {
                    let msg = e.to_string();
                    client
                        .log_message(MessageType::ERROR, format!("error: {msg}"))
                        .await;
                    st.log(json!({"time": unix_time(), "uri": uri.as_str(), "error": msg}));
                    let show = st.shown_error.as_ref() != Some(&msg);
                    st.shown_error = Some(msg.clone());
                    drop(st);
                    if show {
                        client
                            .show_message(MessageType::WARNING, format!("flagcheck: {msg}"))
                            .await;
                    }
                    false
                }
            };
            publish(&client, &state, &uri).await;
            ok
        });
        if n == 0 && warm_first {
            all_ok &= handle.await.unwrap_or(false);
        } else {
            tasks.push(handle);
        }
    }
    for t in tasks {
        all_ok &= t.await.unwrap_or(false);
    }
    // Paragraphs edited while we waited are still unchecked; go again.
    // After an error, wait for the next edit instead of retrying in a loop.
    if all_ok {
        Box::pin(run(client, state, uri)).await;
    }
}

/// One line per request for the editor's language server log, e.g.
/// `claude-haiku-5-5 low | 16 par | 5 flags | 2 dropped | 2786 ms | in 712 cached 2420 out 57`
fn summary(settings: &Settings, out: &checker::Checked, paragraphs: usize) -> String {
    let flags: usize = out.flags.values().map(Vec::len).sum();
    let mut line = format!(
        "{} {} | {paragraphs} par | {flags} flags | {} dropped | {} ms",
        settings.model(),
        settings.effort().unwrap_or_default(),
        out.dropped.len(),
        out.latency_ms,
    );
    let sum = |k: &str| -> u64 {
        out.replies
            .iter()
            .filter_map(|r| r.usage.as_ref()?.get(k)?.as_u64())
            .sum()
    };
    if out.replies.iter().any(|r| r.usage.is_some()) {
        let thinking: u64 = out
            .replies
            .iter()
            .filter_map(|r| {
                r.usage
                    .as_ref()?
                    .get("output_tokens_details")?
                    .get("thinking_tokens")?
                    .as_u64()
            })
            .sum();
        line.push_str(&format!(
            " | in {} cached {} out {} thinking {thinking}",
            sum("input_tokens") + sum("prompt_tokens"),
            sum("cache_read_input_tokens"),
            sum("output_tokens") + sum("completion_tokens"),
        ));
    }
    for d in &out.dropped {
        line.push_str(&format!("\n  dropped ({}): {:?}", d.why, d.quote));
    }
    line
}

fn unix_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn parse_settings(v: &Value) -> Option<Settings> {
    let v = v.get("flagcheck").unwrap_or(v);
    if v.is_null() {
        return None;
    }
    serde_json::from_value(v.clone()).ok()
}

impl LanguageServer for Server {
    async fn initialize(&self, params: InitializeParams) -> RpcResult<InitializeResult> {
        let mut st = self.state.lock().await;
        st.enc = Encoding::negotiate(
            params
                .capabilities
                .general
                .as_ref()
                .and_then(|g| g.position_encodings.as_deref()),
        );
        #[allow(deprecated)]
        let root = params
            .workspace_folders
            .as_ref()
            .and_then(|f| f.first())
            .map(|f| f.uri.clone())
            .or(params.root_uri.clone())
            .and_then(|u| u.to_file_path().map(|p| p.into_owned()));
        st.store = Store::load(root.as_deref());
        st.root = root;
        let settings = params
            .initialization_options
            .as_ref()
            .and_then(parse_settings)
            .unwrap_or_default();
        st.configure(settings);

        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "flagcheck".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            capabilities: ServerCapabilities {
                position_encoding: Some(st.enc.kind()),
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::INCREMENTAL),
                        ..Default::default()
                    },
                )),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![CMD_IGNORE.into(), CMD_ADD_WORD.into()],
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn shutdown(&self) -> RpcResult<()> {
        Ok(())
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        let Some(s) = parse_settings(&params.settings) else {
            return;
        };
        let uris: Vec<Uri> = {
            let mut st = self.state.lock().await;
            st.configure(s);
            st.shown_error = None;
            st.docs.keys().cloned().collect()
        };
        for u in uris {
            self.publish(&u).await;
            self.schedule(u).await;
        }
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let td = params.text_document;
        let path = td.uri.to_file_path().map(|p| p.into_owned());
        let lang = Lang::from_language_id(&td.language_id).or_else(|| {
            path.as_ref()
                .and_then(|p| Lang::from_path(&p.to_string_lossy()))
        });
        let Some(lang) = lang else { return };
        {
            let mut st = self.state.lock().await;
            st.docs
                .insert(td.uri.clone(), Doc::new(lang, path, td.text, td.version));
        }
        self.publish(&td.uri).await;
        // First check runs without waiting for the debounce.
        run(self.client.clone(), self.state.clone(), td.uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        {
            let mut st = self.state.lock().await;
            let enc = st.enc;
            let Some(d) = st.docs.get(&uri) else { return };
            let mut text = d.text.clone();
            let lang = d.lang;
            for ch in params.content_changes {
                match ch.range {
                    Some(r) => {
                        let idx = crate::position::LineIndex::new(&text);
                        let s = idx.offset(&text, r.start, enc);
                        let e = idx.offset(&text, r.end, enc).max(s);
                        text.replace_range(s..e, &ch.text);
                    }
                    None => text = ch.text,
                }
            }
            let fp = st.settings.fingerprint();
            let mut d = st.docs.remove(&uri).unwrap();
            let cache = &st.cache;
            d.set_text(text, params.text_document.version, |p| {
                cache.get(&cache_key(&fp, lang, p)).cloned()
            });
            st.docs.insert(uri.clone(), d);
        }
        self.publish(&uri).await;
        self.schedule(uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.state.lock().await.docs.remove(&uri);
        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn code_action(&self, params: CodeActionParams) -> RpcResult<Option<CodeActionResponse>> {
        let mut actions = vec![];
        for d in &params.context.diagnostics {
            if d.source.as_deref() != Some(SOURCE) {
                continue;
            }
            let Some(data) = &d.data else { continue };
            let (Some(hash), Some(quote)) = (data["hash"].as_str(), data["quote"].as_str()) else {
                continue;
            };
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Ignore".into(),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![d.clone()]),
                command: Some(Command::new(
                    "Ignore".into(),
                    CMD_IGNORE.into(),
                    Some(vec![json!(hash), json!(quote)]),
                )),
                ..Default::default()
            }));
            if data["category"] == "spelling" && !quote.contains(char::is_whitespace) {
                actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: "Add to word list".into(),
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![d.clone()]),
                    command: Some(Command::new(
                        "Add to word list".into(),
                        CMD_ADD_WORD.into(),
                        Some(vec![json!(quote)]),
                    )),
                    ..Default::default()
                }));
            }
        }
        Ok(Some(actions))
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> RpcResult<Option<LSPAny>> {
        let args = params.arguments;
        let uris: Vec<Uri> = {
            let mut st = self.state.lock().await;
            let res = match params.command.as_str() {
                CMD_IGNORE => match (
                    args.first().and_then(|v| v.as_str()),
                    args.get(1).and_then(|v| v.as_str()),
                ) {
                    (Some(h), Some(q)) => match u64::from_str_radix(h, 16) {
                        Ok(h) => st.store.ignore(h, q),
                        Err(_) => Ok(()),
                    },
                    _ => Ok(()),
                },
                CMD_ADD_WORD => match args.first().and_then(|v| v.as_str()) {
                    Some(w) => st.store.add_word(w),
                    None => Ok(()),
                },
                _ => Ok(()),
            };
            if let Err(e) = res {
                drop(st);
                self.client
                    .show_message(MessageType::ERROR, format!("flagcheck: {e}"))
                    .await;
                return Ok(None);
            }
            st.docs.keys().cloned().collect()
        };
        for u in uris {
            self.publish(&u).await;
        }
        Ok(None)
    }
}
