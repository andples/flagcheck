//! Model backends. Each turns a `CheckRequest` into raw flags.

mod anthropic;
mod mock;
mod openai;

use crate::config::{BackendKind, Settings};
use crate::flag::{RawFlag, RawResponse};
use crate::prompt::{CheckRequest, extract_json};
use anyhow::{Context as _, Result, anyhow};
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct Reply {
    pub flags: Vec<RawFlag>,
    /// The model's text, kept verbatim for logs and re-scoring.
    pub raw: String,
    /// Provider usage block (token counts, cache hits), if any.
    pub usage: Option<serde_json::Value>,
}

#[async_trait::async_trait]
pub trait Backend: Send + Sync {
    async fn check(&self, req: &CheckRequest) -> Result<Reply>;
}

pub fn build(s: &Settings) -> Result<Arc<dyn Backend>> {
    Ok(match s.backend {
        BackendKind::Mock => Arc::new(mock::Mock),
        BackendKind::Anthropic => Arc::new(anthropic::Anthropic::new(s, key(s)?)),
        BackendKind::Openai => {
            // Local servers such as Ollama need no key.
            let key = std::env::var(s.api_key_env()).ok().filter(|k| !k.is_empty());
            if key.is_none() && s.base_url.is_none() {
                return Err(anyhow!("{} is not set", s.api_key_env()));
            }
            Arc::new(openai::OpenAi::new(s, key))
        }
    })
}

fn key(s: &Settings) -> Result<String> {
    std::env::var(s.api_key_env())
        .ok()
        .filter(|k| !k.is_empty())
        .ok_or_else(|| anyhow!("{} is not set", s.api_key_env()))
}

pub(crate) fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .expect("http client")
}

pub(crate) fn parse(text: &str) -> Result<Vec<RawFlag>> {
    let json = extract_json(text).context("no JSON object in reply")?;
    // Parse leniently: one malformed flag must not discard the others.
    let v: serde_json::Value = serde_json::from_str(json).context("reply is not valid JSON")?;
    if let Ok(r) = serde_json::from_value::<RawResponse>(v.clone()) {
        return Ok(r.flags);
    }
    let items = v
        .get("flags")
        .and_then(|f| f.as_array())
        .context("reply has no flags array")?;
    Ok(items
        .iter()
        .filter_map(|i| serde_json::from_value::<RawFlag>(i.clone()).ok())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_and_partial() {
        let t = "```json\n{\"flags\":[{\"paragraph\":0,\"quote\":\"teh\",\"before\":\"\",\"category\":\"spelling\",\"reason\":\"x\"},{\"bad\":1}]}\n```";
        let f = parse(t).unwrap();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].quote, "teh");
    }

    #[test]
    fn unknown_category_is_other() {
        let t = r#"{"flags":[{"paragraph":1,"quote":"a","category":"style","reason":""}]}"#;
        let f = parse(t).unwrap();
        assert_eq!(f[0].category, crate::flag::Category::Other);
    }
}
