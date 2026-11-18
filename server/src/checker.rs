//! One check: send paragraphs to the backend (k times when voting), anchor
//! the quotes, apply the markup mask and the flag-only sanitizer.
//!
//! This is shared by the language server and the CLI/benchmark, so both see
//! exactly the same pipeline.

use crate::anchor::{Anchor, anchor, anchor_all};
use crate::backend::{Backend, Reply};
use crate::config::Settings;
use crate::flag::{Category, Flag, RawFlag};
use crate::markup::{Lang, Segmented};
use crate::prompt::CheckRequest;
use crate::sanitize::{ReasonMode, clean_reason};
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::Instant;

#[derive(Debug, Clone, Serialize)]
pub struct Dropped {
    pub paragraph: usize,
    pub quote: String,
    pub why: &'static str,
}

#[derive(Debug, Default)]
pub struct Checked {
    /// Paragraph index -> flags (ranges relative to the paragraph). Every
    /// requested paragraph has an entry, possibly empty.
    pub flags: BTreeMap<usize, Vec<Flag>>,
    pub dropped: Vec<Dropped>,
    pub replies: Vec<Reply>,
    pub latency_ms: u128,
}

pub async fn check(
    backend: &dyn Backend,
    settings: &Settings,
    lang: Lang,
    context: Option<String>,
    text: &str,
    seg: &Segmented,
    ids: &[usize],
) -> Result<Checked> {
    let req = CheckRequest {
        lang,
        document: context,
        paragraphs: ids
            .iter()
            .map(|&i| (i, text[seg.paragraphs[i].clone()].to_string()))
            .collect(),
    };
    let t0 = Instant::now();
    let votes = settings.votes.max(1);
    let replies = futures::future::try_join_all((0..votes).map(|_| backend.check(&req))).await?;
    let latency_ms = t0.elapsed().as_millis();

    let mut out = Checked {
        latency_ms,
        ..Default::default()
    };
    let mut per_vote: Vec<BTreeMap<usize, Vec<Flag>>> = vec![];
    for reply in &replies {
        let mut m: BTreeMap<usize, Vec<Flag>> = BTreeMap::new();
        for raw in &reply.flags {
            match place(raw, ids, text, seg, settings.reasons) {
                Ok(fs) => {
                    let v = m.entry(raw.paragraph).or_default();
                    for f in fs {
                        if !v.iter().any(|g| g.range == f.range) {
                            v.push(f);
                        }
                    }
                }
                Err(why) => out.dropped.push(Dropped {
                    paragraph: raw.paragraph,
                    quote: raw.quote.clone(),
                    why,
                }),
            }
        }
        per_vote.push(m);
    }
    let vote_min = settings.vote_min.clamp(1, votes);
    for &id in ids {
        let lists: Vec<&[Flag]> = per_vote
            .iter()
            .map(|m| m.get(&id).map(|v| v.as_slice()).unwrap_or(&[]))
            .collect();
        out.flags.insert(id, vote(&lists, vote_min));
    }
    out.replies = replies;
    Ok(out)
}

fn place(
    raw: &RawFlag,
    ids: &[usize],
    text: &str,
    seg: &Segmented,
    reasons: ReasonMode,
) -> Result<Vec<Flag>, &'static str> {
    if !ids.contains(&raw.paragraph) {
        return Err("unknown paragraph");
    }
    if raw.category == Category::Other {
        return Err("no category");
    }
    if let Some(why) = implausible(raw) {
        return Err(why);
    }
    let pr = seg.paragraphs[raw.paragraph].clone();
    let par = &text[pr.clone()];
    let ranges = match anchor(par, &raw.quote, &raw.before) {
        Anchor::Found(r) => vec![r],
        Anchor::NotFound => return Err("quote not found"),
        // The same text in several places. A misspelling is wrong wherever
        // it occurs; other errors only where the preceding words match too.
        Anchor::Ambiguous(_) => {
            let before = if raw.category == Category::Spelling {
                ""
            } else {
                raw.before.as_str()
            };
            let all = anchor_all(par, &raw.quote, before);
            if all.is_empty() || (before.is_empty() && raw.category != Category::Spelling) {
                return Err("quote ambiguous");
            }
            all
        }
    };
    let reason = match reasons {
        ReasonMode::Short => clean_reason(&raw.reason, &raw.quote, par),
        ReasonMode::Category | ReasonMode::None => None,
    };
    let mut out = vec![];
    let mut why = "inside markup";
    for range in ranges {
        let abs = pr.start + range.start..pr.start + range.end;
        if seg.no_prose_in(text, abs.clone()) {
            continue;
        }
        if raw.category == Category::Spelling && seg.any_masked_in(abs.clone()) {
            why = "spelling touches markup";
            continue;
        }
        out.push(Flag {
            quote: par[range.clone()].to_string(),
            range,
            category: raw.category,
            reason: reason.clone(),
        });
    }
    if out.is_empty() { Err(why) } else { Ok(out) }
}

/// Shape checks on a flag, from the first real runs: a "spelling" error
/// spanning several words was never a misspelling; reasons about spelling
/// variety flagged British spellings in British documents despite the
/// prompt; whole-sentence quotes point at nothing in particular.
fn implausible(raw: &RawFlag) -> Option<&'static str> {
    let words = raw.quote.split_whitespace().count();
    if raw.category == Category::Spelling && words > 1 {
        return Some("multi-word spelling");
    }
    if words > MAX_QUOTE_WORDS {
        return Some("quote too long");
    }
    let r = raw.reason.to_lowercase();
    if ["variant", "variety", "british", "american", "inconsisten"]
        .iter()
        .any(|w| r.contains(w))
    {
        return Some("spelling variety");
    }
    None
}

const MAX_QUOTE_WORDS: usize = 6;

/// Keep flags that overlap a flag in at least `min` of the vote lists. The
/// representative is the first occurrence (lowest vote index).
fn vote(lists: &[&[Flag]], min: usize) -> Vec<Flag> {
    if lists.len() == 1 {
        return lists[0].to_vec();
    }
    let mut groups: Vec<(Flag, Vec<usize>)> = vec![];
    for (v, list) in lists.iter().enumerate() {
        for f in list.iter() {
            match groups
                .iter_mut()
                .find(|(g, _)| g.range.start < f.range.end && f.range.start < g.range.end)
            {
                Some((_, votes)) => {
                    if !votes.contains(&v) {
                        votes.push(v);
                    }
                }
                None => groups.push((f.clone(), vec![v])),
            }
        }
    }
    let mut out: Vec<Flag> = groups
        .into_iter()
        .filter(|(_, v)| v.len() >= min)
        .map(|(f, _)| f)
        .collect();
    out.sort_by_key(|f| f.range.start);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend;
    use crate::config::BackendKind;
    use crate::markup::segment;

    fn f(r: std::ops::Range<usize>) -> Flag {
        Flag {
            range: r,
            quote: String::new(),
            category: Category::Grammar,
            reason: None,
        }
    }

    #[test]
    fn voting_threshold() {
        let a = [f(0..3), f(10..12)];
        let b = [f(1..4)];
        let c = [f(20..22)];
        let lists: Vec<&[Flag]> = vec![&a, &b, &c];
        let kept: Vec<_> = vote(&lists, 2).into_iter().map(|f| f.range).collect();
        assert_eq!(kept, vec![0..3]);
        assert_eq!(vote(&lists, 1).len(), 3);
    }

    #[test]
    fn implausible_shapes() {
        let mk = |q: &str, c: Category, r: &str| RawFlag {
            paragraph: 0,
            quote: q.into(),
            before: String::new(),
            category: c,
            reason: r.into(),
        };
        assert!(implausible(&mk("Frame stacking", Category::Spelling, "misspelled")).is_some());
        assert!(implausible(&mk("randomises", Category::Spelling, "spelling variant inconsistent")).is_some());
        assert!(implausible(&mk("a b c d e f g", Category::Grammar, "fragment")).is_some());
        assert!(implausible(&mk("mamalls", Category::Spelling, "misspelled word")).is_none());
        assert!(implausible(&mk("is instead", Category::MissingWord, "missing word")).is_none());
    }

    #[tokio::test]
    async fn repeated_misspelling_flags_every_copy() {
        let text = "Test pbbadd here.\nTest pbbadd here.\nTest pbbadd here.\n";
        let s = Settings {
            backend: BackendKind::Mock,
            ..Default::default()
        };
        let seg = segment(Lang::Markdown, text);
        let raw = RawFlag {
            paragraph: 0,
            quote: "pbbadd".into(),
            before: "Test".into(),
            category: Category::Spelling,
            reason: "misspelled word".into(),
        };
        let fs = place(&raw, &[0], text, &seg, s.reasons).unwrap();
        assert_eq!(fs.len(), 3);
    }

    #[tokio::test]
    async fn mock_end_to_end() {
        let text = "# Notes\n\nWe recieve teh data from \\cite{teh}.\n\nIt is is fine `teh`.\n";
        let s = Settings {
            backend: BackendKind::Mock,
            ..Default::default()
        };
        let b = backend::build(&s).unwrap();
        let seg = segment(Lang::Markdown, text);
        let ids: Vec<usize> = (0..seg.paragraphs.len()).collect();
        let out = check(b.as_ref(), &s, Lang::Markdown, None, text, &seg, &ids)
            .await
            .unwrap();
        let quotes: Vec<_> = out
            .flags
            .values()
            .flatten()
            .map(|f| f.quote.as_str())
            .collect();
        assert_eq!(quotes, vec!["recieve", "teh", "is is"]);
    }
}
