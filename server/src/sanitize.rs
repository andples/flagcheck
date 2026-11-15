//! Keep corrections out of diagnostic messages.
//!
//! The prompt tells the model never to give the correct form, but the prompt
//! is not a guarantee. This module is: a reason is shortened, cut before any
//! phrase that introduces a fix, stripped of quoted text, and rejected
//! entirely if it still contains a word that looks like an edit of the
//! flagged text and does not occur in the paragraph. When in doubt the reason
//! is dropped and only the category is shown.

/// Phrases after which a reason would start describing the fix.
const CUT_MARKERS: &[&str] = &[
    "should", "instead", "replace", "use ", "try ", "consider", "did you mean", "perhaps",
    "maybe", "change to", "change it", "correct form", "correct spelling", "correctly",
    "rather than", "write ", "spelled", "spelt", "meant", "probably", "likely", "e.g.",
    "i.e.", "such as", "like ", "->", "→", "=>", "⇒", ":", "(", "would be", "needs",
    "need ", "must", "requires", "add ", "remove ", "delete ", "insert ",
];

/// Generic grammar vocabulary that may appear in reasons even if absent
/// from the paragraph and close to a flagged word.
const VOCAB: &[&str] = &[
    "verb", "verbs", "noun", "nouns", "subject", "object", "tense", "plural", "singular",
    "article", "articles", "word", "words", "missing", "extra", "repeated", "agreement",
    "spelling", "misspelled", "misspelling", "typo", "preposition", "punctuation", "comma",
    "commas", "period", "hyphen", "apostrophe", "pronoun", "adjective", "adverb", "form",
    "wrong", "incorrect", "duplicate", "doubled", "unclear", "number", "person", "case",
    "past", "present", "future", "possessive", "contraction", "capitalisation",
    "capitalization", "capital", "letter", "order", "mismatch", "sentence", "clause",
    "fragment", "run-on", "usage", "choice", "confused", "homophone", "the", "a", "an",
    "and", "or", "of", "in", "on", "to", "is", "are", "was", "were", "it", "its", "this",
    "that", "with", "not", "no", "too", "here", "after", "before", "has", "have", "had",
];

const MAX_CHARS: usize = 48;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReasonMode {
    /// A short sanitized reason.
    #[default]
    Short,
    /// Category label only.
    Category,
    /// Nothing but the underline.
    None,
}

pub fn clean_reason(reason: &str, quote: &str, paragraph: &str) -> Option<String> {
    let mut r = format!("{reason} ");
    let lower = r.to_lowercase();
    let cut = CUT_MARKERS
        .iter()
        .filter_map(|m| find_word_start(&lower, m))
        .min()
        .unwrap_or(r.len());
    r.truncate(floor(&r, cut));

    let r = strip_quoted(&r);
    let r = r
        .trim()
        .trim_end_matches(|c: char| !c.is_alphanumeric())
        .trim()
        .to_string();
    if r.is_empty() {
        return None;
    }

    // Reject reasons that carry a plausible replacement word.
    let par_words: std::collections::HashSet<String> = words(paragraph).collect();
    let quote_words: Vec<String> = words(quote).collect();
    for w in words(&r) {
        if par_words.contains(&w) || VOCAB.contains(&w.as_str()) {
            continue;
        }
        if quote_words.iter().any(|q| close_edit(&w, q)) {
            return None;
        }
    }
    // A reason that is mostly words from the quote is restating the text,
    // which is harmless, but one that introduces many new content words
    // is likely rephrasing it; keep reasons generic.
    let new_words = words(&r)
        .filter(|w| !par_words.contains(w) && !VOCAB.contains(&w.as_str()))
        .count();
    if new_words > 4 {
        return None;
    }

    Some(shorten(&r))
}

fn shorten(r: &str) -> String {
    if r.chars().count() <= MAX_CHARS {
        return r.to_string();
    }
    let mut out = String::new();
    for w in r.split_whitespace() {
        if out.chars().count() + w.chars().count() + 1 > MAX_CHARS {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(w);
    }
    out
}

/// Remove "...", “...”, `...`, and '...' (when the quotes sit on word
/// boundaries, so apostrophes in "it's" survive).
fn strip_quoted(s: &str) -> String {
    let pairs = [('"', '"'), ('“', '”'), ('‘', '’'), ('`', '`'), ('«', '»'), ('\'', '\'')];
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    'outer: while i < chars.len() {
        for (o, c) in pairs {
            if chars[i] == o {
                let opens = o != '\'' || i == 0 || !chars[i - 1].is_alphanumeric();
                if opens {
                    if let Some(k) = chars[i + 1..].iter().position(|&x| x == c) {
                        let close = i + 1 + k;
                        let closes =
                            c != '\'' || close + 1 >= chars.len() || !chars[close + 1].is_alphanumeric();
                        if closes {
                            i = close + 1;
                            continue 'outer;
                        }
                    }
                    if o != '\'' {
                        // Unbalanced quote: drop the rest.
                        break 'outer;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn words(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '-'))
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
}

/// True if `a` is within a small edit distance of `b` (a likely correction).
fn close_edit(a: &str, b: &str) -> bool {
    if a == b {
        return false;
    }
    let (la, lb) = (a.chars().count(), b.chars().count());
    if la < 2 || lb < 2 {
        return false;
    }
    // Generous on purpose: dropping a harmless reason costs little.
    let limit = ((la.max(lb) + 2) / 3).max(1);
    if la.abs_diff(lb) > limit {
        // Also catch inflections: "go" -> "goes", "child" -> "children".
        return a.starts_with(b) || b.starts_with(a);
    }
    levenshtein(a, b) <= limit || a.starts_with(b) || b.starts_with(a)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, &cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            cur.push(sub.min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// First occurrence of `m` in `s` that does not start mid-word.
fn find_word_start(s: &str, m: &str) -> Option<usize> {
    s.match_indices(m)
        .map(|(i, _)| i)
        .find(|&i| {
            !m.starts_with(char::is_alphanumeric)
                || !s[..i].chars().next_back().is_some_and(char::is_alphanumeric)
        })
}

fn floor(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAR: &str = "The results was recieve by the the team.";

    #[test]
    fn keeps_generic_reasons() {
        assert_eq!(
            clean_reason("subject-verb agreement", "results was", PAR).as_deref(),
            Some("subject-verb agreement")
        );
        assert_eq!(
            clean_reason("Misspelled word.", "recieve", PAR).as_deref(),
            Some("Misspelled word")
        );
        assert_eq!(clean_reason("repeated word", "the the", PAR).as_deref(), Some("repeated word"));
    }

    #[test]
    fn cuts_fix_phrases() {
        assert_eq!(
            clean_reason("agreement; should be 'were'", "results was", PAR).as_deref(),
            Some("agreement")
        );
        assert_eq!(
            clean_reason("misspelled: receive", "recieve", PAR).as_deref(),
            Some("misspelled")
        );
        assert_eq!(clean_reason("wrong verb -> were", "was", PAR).as_deref(), Some("wrong verb"));
        assert_eq!(clean_reason("Use \"received\"", "recieve", PAR), None);
    }

    #[test]
    fn rejects_hidden_corrections() {
        // "receive" is not in the paragraph and is one edit from the quote.
        assert_eq!(clean_reason("misspelling of receive", "recieve", PAR), None);
        assert_eq!(clean_reason("plural needs were not was", "was", PAR), Some("plural".into()));
        assert_eq!(clean_reason("received is the past tense", "recieve", PAR), None);
    }

    #[test]
    fn strips_quotes_keeps_apostrophes() {
        assert_eq!(strip_quoted("it's 'wrong' here"), "it's here");
        assert_eq!(strip_quoted("the “fix” now"), "the now");
    }

    #[test]
    fn shortens() {
        let long = "this is a very long reason that goes on and on about the problem at hand";
        let out = clean_reason(long, "x", long).unwrap();
        assert!(out.chars().count() <= MAX_CHARS, "{out}");
    }
}
