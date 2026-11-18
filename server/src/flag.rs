//! Flag types shared by backends, the checker and the server.

use serde::{Deserialize, Serialize};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Spelling,
    Grammar,
    Agreement,
    Tense,
    Article,
    Preposition,
    Punctuation,
    WordChoice,
    Repetition,
    MissingWord,
    ExtraWord,
    #[serde(other)]
    Other,
}

impl Category {
    /// Categories the model may use. `Other` is deliberately absent: it
    /// invited style comments. Replies that still use an unknown category
    /// parse as `Other` and are dropped.
    pub const ALL: &[&str] = &[
        "spelling",
        "grammar",
        "agreement",
        "tense",
        "article",
        "preposition",
        "punctuation",
        "word-choice",
        "repetition",
        "missing-word",
        "extra-word",
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Spelling => "spelling",
            Category::Grammar => "grammar",
            Category::Agreement => "agreement",
            Category::Tense => "tense",
            Category::Article => "article",
            Category::Preposition => "preposition",
            Category::Punctuation => "punctuation",
            Category::WordChoice => "word choice",
            Category::Repetition => "repetition",
            Category::MissingWord => "missing word",
            Category::ExtraWord => "extra word",
            Category::Other => "other",
        }
    }
}

/// One flag as the model reports it, before anchoring.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawFlag {
    pub paragraph: usize,
    pub quote: String,
    #[serde(default)]
    pub before: String,
    pub category: Category,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct RawResponse {
    #[serde(default)]
    pub flags: Vec<RawFlag>,
}

/// A flag anchored inside its paragraph. Offsets are relative to the
/// paragraph start, so the flag survives edits elsewhere in the document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Flag {
    pub range: Range<usize>,
    pub quote: String,
    pub category: Category,
    /// Already sanitized. `None` when no safe reason remained.
    pub reason: Option<String>,
}
