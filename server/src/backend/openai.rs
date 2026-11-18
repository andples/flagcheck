//! OpenAI-compatible chat completions: OpenAI itself, most hosted
//! open-model providers, and local servers such as Ollama or llama.cpp.

use super::{Backend, Reply, http, parse};
use crate::config::Settings;
use crate::prompt::{CheckRequest, SYSTEM, schema};
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

pub struct OpenAi {
    client: reqwest::Client,
    url: String,
    key: Option<String>,
    model: String,
    effort: Option<String>,
    json_mode: String,
}

impl OpenAi {
    pub fn new(s: &Settings, key: Option<String>) -> Self {
        let base = s
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".into());
        OpenAi {
            client: http(),
            url: format!("{}/chat/completions", base.trim_end_matches('/')),
            key,
            model: s.model(),
            effort: s.effort(),
            json_mode: s.json_mode.clone(),
        }
    }

    fn body(&self, req: &CheckRequest) -> Value {
        let mut user = String::new();
        if let Some(doc) = req.document_block() {
            user.push_str(&doc);
            user.push_str("\n\n");
        }
        user.push_str(&req.check_block());
        let mut body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": SYSTEM},
                {"role": "user", "content": user}
            ]
        });
        match self.json_mode.as_str() {
            "schema" => {
                body["response_format"] = json!({
                    "type": "json_schema",
                    "json_schema": {"name": "flags", "strict": true, "schema": schema()}
                })
            }
            "object" => body["response_format"] = json!({"type": "json_object"}),
            _ => {}
        }
        if let Some(e) = &self.effort {
            body["reasoning_effort"] = json!(e);
        }
        body
    }
}

#[async_trait::async_trait]
impl Backend for OpenAi {
    async fn check(&self, req: &CheckRequest) -> Result<Reply> {
        let mut attempt = 0;
        let resp: Value = loop {
            let mut r = self.client.post(&self.url).json(&self.body(req));
            if let Some(k) = &self.key {
                r = r.bearer_auth(k);
            }
            let r = r.send().await?;
            let status = r.status();
            if status.is_success() {
                break r.json().await?;
            }
            let text = r.text().await.unwrap_or_default();
            if (status.as_u16() == 429 || status.is_server_error()) && attempt < 3 {
                attempt += 1;
                tokio::time::sleep(std::time::Duration::from_millis(500 << attempt)).await;
                continue;
            }
            bail!(
                "openai-compatible {status}: {}",
                text.chars().take(200).collect::<String>()
            );
        };
        let choice = &resp["choices"][0];
        if choice["finish_reason"] == "length" {
            bail!("reply hit the token limit");
        }
        let raw = choice["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("no content in reply"))?
            .to_string();
        let flags = parse(&raw)?;
        Ok(Reply {
            flags,
            raw,
            usage: Some(resp["usage"].clone()),
        })
    }
}
