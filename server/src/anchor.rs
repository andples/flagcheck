//! Map a model's quoted span back to a byte range in the paragraph.
//!
//! Models are poor at character offsets but good at copying text, so the
//! model returns the erroneous text verbatim plus a few words before it. We
//! search for it ourselves:
//!
//! 1. exact matches on word boundaries, then any exact match;
//! 2. if more than one, keep those preceded by the model's `before` text;
//! 3. if none, repeat on whitespace-collapsed text (models often reflow
//!    line breaks into spaces).
//!
//! Anything still unfound or ambiguous is reported as such and dropped by
//! the caller. Those drops are logged; in evaluation they count against the
//! system.

use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anchor {
    Found(Range<usize>),
    NotFound,
    Ambiguous(usize),
}

pub fn anchor(par: &str, quote: &str, before: &str) -> Anchor {
    let quote = quote.trim();
    if quote.is_empty() {
        return Anchor::NotFound;
    }
    match pick(par, quote, before) {
        Anchor::NotFound => {}
        a => return a,
    }
    // Whitespace-insensitive pass.
    let (norm, map) = collapse(par);
    let (q, _) = collapse(quote);
    let (bf, _) = collapse(before);
    match pick(&norm, q.trim(), &bf) {
        Anchor::Found(r) => {
            // Non-space bytes map one to one, so the last byte maps exactly.
            Anchor::Found(map[r.start]..map[r.end - 1] + 1)
        }
        a => a,
    }
}

/// For a quote that matches several places: every occurrence (on word
/// boundaries where possible). With `before`, only occurrences preceded by
/// it. Used when the same error is repeated, e.g. a misspelling that occurs
/// twice in one paragraph.
pub fn anchor_all(par: &str, quote: &str, before: &str) -> Vec<Range<usize>> {
    let quote = quote.trim();
    if quote.is_empty() {
        return vec![];
    }
    let all: Vec<usize> = par.match_indices(quote).map(|(i, _)| i).collect();
    let bounded: Vec<usize> = all
        .iter()
        .copied()
        .filter(|&i| on_boundary(par, i, i + quote.len()))
        .collect();
    let cands = if bounded.is_empty() { all } else { bounded };
    let b = squash(before.trim());
    let cands: Vec<usize> = if b.is_empty() {
        cands
    } else {
        cands
            .into_iter()
            .filter(|&i| squash(&par[..i]).ends_with(&b))
            .collect()
    };
    cands.into_iter().map(|i| i..i + quote.len()).collect()
}

fn pick(par: &str, quote: &str, before: &str) -> Anchor {
    let all: Vec<usize> = par.match_indices(quote).map(|(i, _)| i).collect();
    if all.is_empty() {
        return Anchor::NotFound;
    }
    let bounded: Vec<usize> = all
        .iter()
        .copied()
        .filter(|&i| on_boundary(par, i, i + quote.len()))
        .collect();
    let cands = if bounded.is_empty() { all } else { bounded };
    if cands.len() == 1 {
        return found(cands[0], quote);
    }
    let before = before.trim();
    if !before.is_empty() {
        let b = squash(before);
        let hits: Vec<usize> = cands
            .iter()
            .copied()
            .filter(|&i| squash(&par[..i]).ends_with(&b))
            .collect();
        if hits.len() == 1 {
            return found(hits[0], quote);
        }
    } else if cands[0] == 0 || squash(&par[..cands[0]]).is_empty() {
        // Empty `before` means "at the start of the paragraph".
        return found(cands[0], quote);
    }
    Anchor::Ambiguous(cands.len())
}

fn found(i: usize, quote: &str) -> Anchor {
    Anchor::Found(i..i + quote.len())
}

/// The match does not start or end in the middle of a word.
pub fn on_boundary(text: &str, start: usize, end: usize) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '\'';
    let first = text[start..end].chars().next();
    let last = text[start..end].chars().next_back();
    let prev = text[..start].chars().next_back();
    let next = text[end..].chars().next();
    let left = !(first.is_some_and(word) && prev.is_some_and(word));
    let right = !(last.is_some_and(word) && next.is_some_and(word));
    left && right
}

/// Remove all whitespace, for comparing `before` text loosely.
fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Collapse whitespace runs to one space; return the new string and, for
/// each byte of it, the byte offset in the original.
fn collapse(s: &str) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(s.len());
    let mut map = Vec::with_capacity(s.len());
    let mut in_ws = false;
    for (i, c) in s.char_indices() {
        if c.is_whitespace() {
            if !in_ws {
                out.push(' ');
                map.push(i);
            }
            in_ws = true;
        } else {
            in_ws = false;
            let n = c.len_utf8();
            out.push(c);
            for k in 0..n {
                map.push(i + k);
            }
        }
    }
    (out, map)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(par: &str, a: Anchor) -> String {
        match a {
            Anchor::Found(r) => par[r].to_string(),
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn unique_exact() {
        let p = "We recieve the data.";
        assert_eq!(text(p, anchor(p, "recieve", "")), "recieve");
    }

    #[test]
    fn prefers_word_boundary() {
        let p = "The theory of the cat.";
        assert_eq!(anchor(p, "the", "of"), Anchor::Found(14..17));
    }

    #[test]
    fn disambiguates_with_before() {
        let p = "it are big and it are small";
        assert_eq!(anchor(p, "it are", "big and"), Anchor::Found(15..21));
        assert_eq!(anchor(p, "it are", "nothing"), Anchor::Ambiguous(2));
        assert_eq!(anchor(p, "it are", ""), Anchor::Found(0..6));
    }

    #[test]
    fn whitespace_insensitive() {
        let p = "this is a\n  sentense with a break";
        let a = anchor(p, "a sentense", "is");
        assert_eq!(text(p, a), "a\n  sentense");
    }

    #[test]
    fn all_occurrences() {
        let p = "a teh b. a teh b. c teh d.";
        assert_eq!(anchor_all(p, "teh", "").len(), 3);
        assert_eq!(anchor_all(p, "teh", "a").len(), 2);
    }

    #[test]
    fn not_found() {
        assert_eq!(anchor("abc", "xyz", ""), Anchor::NotFound);
        assert_eq!(anchor("abc", "  ", ""), Anchor::NotFound);
    }

    #[test]
    fn unicode_quote() {
        let p = "Le café est trés bon.";
        assert_eq!(text(p, anchor(p, "trés", "est")), "trés");
    }
}
