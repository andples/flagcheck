//! Claude via the Messages API (raw HTTP; there is no official Rust SDK).
//!
//! Layout for prompt caching, most stable first: system prompt, document
//! snapshot (cache breakpoint), then the paragraphs to check. Haiku 5.5
//! rejects sampling parameters, so none are sent.

use super::{Backend, Reply, http, parse};
use crate::config::{Settings, Thinking};
use crate::prompt::{CheckRequest, SYSTEM, schema};
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

const API_VERSION: &str = "2026-10-21";

pub struct Anthropic {
    client: reqwest::Client,
    url: String,
    key: String,
    model: String,
    effort: Option<String>,
    thinking: Thinking,
    max_tokens: u32,
}

impl Anthropic {
    pub fn new(s: &Settings, key: String) -> Self {
        let base = s
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.anthropic.com".into());
        Anthropic {
            client: http(),
            url: format!("{}/v1/messages", base.trim_end_matches('/')),
            key,
            model: s.model(),
            effort: s.effort(),
            thinking: s.thinking,
            max_tokens: s.max_tokens,
        }
    }

    pub fn body(&self, req: &CheckRequest) -> Value {
        let mut content = vec![];
        if let Some(doc) = req.document_block() {
            content.push(json!({
                "type": "text",
                "text": doc,
                "cache_control": {"type": "ephemeral"}
            }));
        }
        content.push(json!({"type": "text", "text": req.check_block()}));

        let mut output_config = json!({
            "format": {"type": "json_schema", "schema": schema()}
        });
        if let Some(e) = &self.effort {
            output_config["effort"] = json!(e);
        }
        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": [{"type": "text", "text": SYSTEM}],
            "messages": [{"role": "user", "content": content}],
            "output_config": output_config,
        });
        if self.thinking == Thinking::Disabled {
            body["thinking"] = json!({"type": "disabled"});
        }
        body
    }
}

#[async_trait::async_trait]
impl Backend for Anthropic {
    async fn check(&self, req: &CheckRequest) -> Result<Reply> {
        let mut attempt = 0;
        let resp: Value = loop {
            let r = self
                .client
                .post(&self.url)
                .header("x-api-key", &self.key)
                .header("anthropic-version", API_VERSION)
                .json(&self.body(req))
                .send()
                .await?;
            let status = r.status();
            if status.is_success() {
                break r.json().await?;
            }
            let retryable =
                status.as_u16() == 429 || status.as_u16() == 529 || status.is_server_error();
            let text = r.text().await.unwrap_or_default();
            if retryable && attempt < 3 {
                attempt += 1;
                tokio::time::sleep(std::time::Duration::from_millis(500 << attempt)).await;
                continue;
            }
            bail!("anthropic {status}: {}", error_message(&text));
        };

        match resp["stop_reason"].as_str() {
            Some("refusal") => bail!("model declined the request"),
            Some("max_tokens") => bail!("reply hit max_tokens"),
            _ => {}
        }
        let raw: String = resp["content"]
            .as_array()
            .ok_or_else(|| anyhow!("no content in reply"))?
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect();
        let flags = parse(&raw)?;
        Ok(Reply {
            flags,
            raw,
            usage: Some(resp["usage"].clone()),
        })
    }
}

fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(String::from))
        .unwrap_or_else(|| body.chars().take(200).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markup::Lang;

    #[test]
    fn body_shape() {
        let s = Settings::default();
        let a = Anthropic::new(&s, "k".into());
        let req = CheckRequest {
            lang: Lang::Markdown,
            document: Some("doc".into()),
            paragraphs: vec![(0, "Teh cat.".into())],
        };
        let b = a.body(&req);
        assert_eq!(b["model"], "claude-haiku-5-5");
        assert_eq!(b["output_config"]["effort"], "low");
        assert_eq!(b["output_config"]["format"]["type"], "json_schema");
        assert!(b.get("temperature").is_none());
        assert!(b.get("thinking").is_none());
        let content = b["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content[0]["cache_control"]["type"], "ephemeral");
        assert!(content[1]["text"].as_str().unwrap().contains("Teh cat."));
        assert!(!b.to_string().contains("\"k\""));
    }
}
