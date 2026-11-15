//! Deterministic offline backend for tests and for trying the editor
//! integration without an API key. Flags a few common misspellings and
//! doubled words.

use super::{Backend, Reply};
use crate::flag::{Category, RawFlag};
use crate::prompt::CheckRequest;
use anyhow::Result;

const MISSPELLINGS: &[&str] = &[
    "teh", "recieve", "recieved", "seperate", "occured", "definately", "wich", "untill",
    "accomodate", "acheive", "beleive", "enviroment", "goverment", "neccessary", "occurence",
    "thier", "tommorow", "wierd", "arguement", "existance", "independant", "refered",
];

pub struct Mock;

#[async_trait::async_trait]
impl Backend for Mock {
    async fn check(&self, req: &CheckRequest) -> Result<Reply> {
        let mut flags = vec![];
        for (id, text) in &req.paragraphs {
            let words: Vec<(usize, &str)> = text
                .split_whitespace()
                .map(|w| (w.as_ptr() as usize - text.as_ptr() as usize, w))
                .collect();
            for (k, &(_, w)) in words.iter().enumerate() {
                let bare = w.trim_matches(|c: char| !c.is_alphanumeric());
                let before = if k > 0 { words[k - 1].1.to_string() } else { String::new() };
                if MISSPELLINGS.contains(&bare.to_lowercase().as_str()) {
                    flags.push(RawFlag {
                        paragraph: *id,
                        quote: bare.to_string(),
                        before: before.clone(),
                        category: Category::Spelling,
                        reason: "misspelled word".into(),
                    });
                }
                if k > 0 {
                    let prev = words[k - 1].1.trim_matches(|c: char| !c.is_alphanumeric());
                    if !bare.is_empty() && prev.eq_ignore_ascii_case(bare) && words[k - 1].1 == prev {
                        flags.push(RawFlag {
                            paragraph: *id,
                            quote: format!("{prev} {bare}"),
                            before: if k > 1 { words[k - 2].1.to_string() } else { String::new() },
                            category: Category::Repetition,
                            reason: "repeated word".into(),
                        });
                    }
                }
            }
        }
        let raw = serde_json::json!({ "flags": flags }).to_string();
        Ok(Reply { flags, raw, usage: None })
    }
}
