//! The prompt and output schema. Changing anything here must bump
//! `PROMPT_VERSION`: it is part of the cache key and of every logged run.

use crate::flag::Category;
use crate::markup::Lang;
use serde_json::{Value, json};

pub const PROMPT_VERSION: u32 = 2;

pub const SYSTEM: &str = "\
You locate spelling and grammar errors. You never correct them.

You receive the source of a document (Markdown, LaTeX or Typst) and a list of \
paragraphs to check, each with an id. Report errors only in those paragraphs. \
The rest of the document is context: use it to learn the author's terms, \
names, notation and spelling variety (for example British or American).

Report only clear errors that a careful copy editor would fix: misspellings, \
wrong word forms, subject-verb agreement, tense, articles, prepositions, \
missing, doubled or extra words, and clearly wrong punctuation. Do not report \
style, tone, wording preferences, sentence length, passive voice, or anything \
that is acceptable as written. Technical terms, names, acronyms, code \
identifiers and notation are not errors. British and American spellings are \
both correct; never report one as a misspelling of the other. Report a word \
choice only when the word is clearly wrong (for example a confused homophone), \
never because another word would be better. When unsure, do not report.

Markup is not prose. Never report anything inside commands, maths, code, \
citations, labels, references, URLs or comments.

For each error:
- paragraph: the id of the paragraph it is in.
- quote: the erroneous text copied exactly from the paragraph source, \
character for character, including any markup inside it. Use the shortest \
span that shows the error, usually one to three words.
- before: the one to four words that come immediately before the quote in \
the paragraph, copied exactly; empty if the quote starts the paragraph.
- category: one of the listed categories.
- reason: at most five words naming the kind of problem, such as \
\"subject-verb agreement\" or \"misspelled word\".

Never state, quote or hint at the correct form anywhere in your answer. \
Do not rewrite text. If the paragraphs have no errors, return an empty list.";

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "flags": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "paragraph": {"type": "integer"},
                        "quote": {"type": "string"},
                        "before": {"type": "string"},
                        "category": {"type": "string", "enum": Category::ALL},
                        "reason": {"type": "string"}
                    },
                    "required": ["paragraph", "quote", "before", "category", "reason"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["flags"],
        "additionalProperties": false
    })
}

/// One request: optional document context plus the paragraphs to check.
#[derive(Clone, Debug)]
pub struct CheckRequest {
    pub lang: Lang,
    /// Whole-document context (a snapshot), or `None` for paragraph-only.
    pub document: Option<String>,
    /// (id, paragraph source).
    pub paragraphs: Vec<(usize, String)>,
}

impl CheckRequest {
    pub fn document_block(&self) -> Option<String> {
        self.document.as_ref().map(|d| {
            format!(
                "<document format=\"{}\">\n{}\n</document>",
                self.lang.name(),
                d
            )
        })
    }

    pub fn check_block(&self) -> String {
        let mut s = format!("Check these {} paragraphs:\n", self.lang.name());
        for (id, text) in &self.paragraphs {
            s.push_str(&format!("<paragraph id=\"{id}\">\n{text}\n</paragraph>\n"));
        }
        s
    }
}

/// Pull the JSON object out of a model reply, tolerating code fences and
/// surrounding chatter from backends without structured output.
pub fn extract_json(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then(|| &text[start..=end])
}
