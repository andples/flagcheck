//! Settings, from LSP `initializationOptions` or the CLI.
//!
//! API keys are never part of the settings: only the *name* of the
//! environment variable holding the key is configurable.

use crate::sanitize::ReasonMode;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    #[default]
    Anthropic,
    Openai,
    Mock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Context {
    /// Whole document as context (default).
    #[default]
    Document,
    /// Only the paragraphs being checked.
    Paragraph,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Thinking {
    Adaptive,
    /// Default: thinking made live checks 2-3x slower (4-9 s vs 1.6-3 s)
    /// without catching more in the first real runs.
    #[default]
    Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default]
    Information,
    Hint,
    Warning,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub backend: BackendKind,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
    /// Anthropic `output_config.effort`; OpenAI `reasoning_effort` if set.
    pub effort: Option<String>,
    pub thinking: Thinking,
    pub debounce_ms: u64,
    pub reasons: ReasonMode,
    pub severity: Severity,
    pub votes: usize,
    pub vote_min: usize,
    pub context: Context,
    /// Max paragraphs per request.
    pub batch: usize,
    /// Parallel requests per document.
    pub concurrency: usize,
    /// JSONL log of requests and raw responses, for evaluation.
    pub log_path: Option<String>,
    /// OpenAI-compatible only: "schema" (json_schema), "object", "none".
    pub json_mode: String,
    pub max_tokens: u32,
    /// Document context above this size is split into windows, keeping
    /// every request below Haiku's 100k-token price step.
    pub max_context_tokens: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            backend: BackendKind::Anthropic,
            model: None,
            base_url: None,
            api_key_env: None,
            effort: None,
            thinking: Thinking::Disabled,
            debounce_ms: 1500,
            reasons: ReasonMode::Short,
            severity: Severity::Information,
            votes: 1,
            vote_min: 1,
            context: Context::Document,
            batch: 16,
            concurrency: 4,
            log_path: None,
            json_mode: "schema".into(),
            max_tokens: 8000,
            max_context_tokens: 60_000,
        }
    }
}

impl Settings {
    pub fn model(&self) -> String {
        self.model.clone().unwrap_or_else(|| match self.backend {
            BackendKind::Anthropic => "claude-haiku-5-5".into(),
            BackendKind::Openai => "gpt-5-mini".into(),
            BackendKind::Mock => "mock".into(),
        })
    }

    pub fn effort(&self) -> Option<String> {
        match (&self.effort, self.backend) {
            (Some(e), _) => Some(e.clone()),
            // Pilot (docs/experiments): medium with thinking off was the most
            // stable configuration at the same latency as low.
            (None, BackendKind::Anthropic) => Some("medium".into()),
            _ => None,
        }
    }

    pub fn api_key_env(&self) -> String {
        self.api_key_env
            .clone()
            .unwrap_or_else(|| match self.backend {
                BackendKind::Openai => "OPENAI_API_KEY".into(),
                _ => "ANTHROPIC_API_KEY".into(),
            })
    }

    /// Everything that can change a paragraph's flags. Part of the cache key,
    /// so changing a setting never serves stale flags.
    pub fn fingerprint(&self) -> String {
        format!(
            "p{}|{:?}|{}|{:?}|{:?}|{:?}|{}/{}|w{}",
            crate::prompt::PROMPT_VERSION,
            self.backend,
            self.model(),
            self.effort(),
            self.thinking,
            self.context,
            self.votes,
            self.vote_min,
            self.max_context_tokens,
        )
    }
}
