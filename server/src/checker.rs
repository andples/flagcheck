//! One check: send paragraphs to the backend (k times when voting), anchor
//! the quotes, apply the markup mask and the flag-only sanitizer.
//!
//! This is shared by the language server and the CLI/benchmark, so both see
//! exactly the same pipeline.

use crate::anchor::{Anchor, anchor};
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
                Ok(f) => {
                    let v = m.entry(raw.paragraph).or_default();
                    if !v.iter().any(|g| g.range == f.range) {
                        v.push(f);
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
) -> Result<Flag, &'static str> {
    if !ids.contains(&raw.paragraph) {
        return Err("unknown paragraph");
    }
    let pr = seg.paragraphs[raw.paragraph].clone();
    let par = &text[pr.clone()];
    let range = match anchor(par, &raw.quote, &raw.before) {
        Anchor::Found(r) => r,
        Anchor::NotFound => return Err("quote not found"),
        Anchor::Ambiguous(_) => return Err("quote ambiguous"),
    };
    let abs = pr.start + range.start..pr.start + range.end;
    if seg.no_prose_in(text, abs.clone()) {
        return Err("inside markup");
    }
    if raw.category == Category::Spelling && seg.any_masked_in(abs.clone()) {
        return Err("spelling touches markup");
    }
    let reason = match reasons {
        ReasonMode::Short => clean_reason(&raw.reason, &raw.quote, par),
        ReasonMode::Category | ReasonMode::None => None,
    };
    Ok(Flag {
        quote: par[range.clone()].to_string(),
        range,
        category: raw.category,
        reason,
    })
}

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
