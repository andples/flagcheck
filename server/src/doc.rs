//! An open document: text, segmentation, paragraph hashes, the context
//! snapshot sent to the model, and flags carried over while a paragraph
//! waits to be re-checked.

use crate::anchor::on_boundary;
use crate::flag::Flag;
use crate::markup::{Lang, Segmented, segment};
use crate::position::LineIndex;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Stable 64-bit FNV-1a. Used for paragraph hashes that are written to
/// `.flagcheck/ignored.txt`, so it must not change between builds.
pub fn fnv(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for p in parts {
        for &b in *p {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn paragraph_hash(text: &str) -> u64 {
    fnv(&[text.as_bytes()])
}

const SNAPSHOT_MAX_AGE: Duration = Duration::from_secs(240);
const SNAPSHOT_MAX_CHANGED: f64 = 0.2;

pub struct Snapshot {
    pub text: String,
    hashes: HashSet<u64>,
    at: Instant,
}

pub struct Doc {
    pub lang: Lang,
    pub path: Option<PathBuf>,
    pub text: String,
    pub version: i32,
    pub seg: Segmented,
    pub index: LineIndex,
    /// Hash of each paragraph's source text.
    pub hashes: Vec<u64>,
    /// Flags from the previous version of a changed paragraph whose quote
    /// is still present, shown until the paragraph is re-checked. Avoids
    /// underlines blinking off and on while typing.
    pub provisional: HashMap<usize, Vec<Flag>>,
    pub snapshot: Option<Snapshot>,
    /// Bumped on every change; a debounced check only runs if unchanged.
    pub generation: u64,
    pub disabled: bool,
}

impl Doc {
    pub fn new(lang: Lang, path: Option<PathBuf>, text: String, version: i32) -> Self {
        let mut d = Doc {
            lang,
            path,
            text: String::new(),
            version,
            seg: Segmented::default(),
            index: LineIndex::new(""),
            hashes: vec![],
            provisional: HashMap::new(),
            snapshot: None,
            generation: 0,
            disabled: false,
        };
        d.set_text(text, version, |_| None);
        d
    }

    pub fn paragraph(&self, i: usize) -> &str {
        &self.text[self.seg.paragraphs[i].clone()]
    }

    /// Replace the text. `cached` looks up the flags of a paragraph (by its
    /// text) from before the change, for carrying them over.
    pub fn set_text(&mut self, text: String, version: i32, cached: impl Fn(&str) -> Option<Vec<Flag>>) {
        let old: Vec<(std::ops::Range<usize>, Vec<Flag>)> = self
            .seg
            .paragraphs
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                let flags = cached(&self.text[r.clone()]).or_else(|| self.provisional.get(&i).cloned())?;
                (!flags.is_empty()).then(|| (r.clone(), flags))
            })
            .collect();
        let old_len = self.text.len();

        self.text = text;
        self.version = version;
        self.seg = segment(self.lang, &self.text);
        self.index = LineIndex::new(&self.text);
        self.hashes = self
            .seg
            .paragraphs
            .iter()
            .map(|r| paragraph_hash(&self.text[r.clone()]))
            .collect();
        self.generation += 1;
        self.disabled = self
            .text
            .lines()
            .take(5)
            .any(|l| l.contains("flagcheck: off"));

        // Carry flags over to changed paragraphs, matched to the old
        // paragraph at the same distance from the start or end of the file.
        let new_len = self.text.len();
        let mut provisional = HashMap::new();
        for (i, r) in self.seg.paragraphs.iter().enumerate() {
            if cached(&self.text[r.clone()]).is_some() {
                continue;
            }
            let par = &self.text[r.clone()];
            let shift = new_len as isize - old_len as isize;
            let Some((_, flags)) = old.iter().find(|(or, _)| {
                overlaps(or, r) || overlaps(&shifted(or, shift), r)
            }) else {
                continue;
            };
            let kept: Vec<Flag> = flags
                .iter()
                .filter_map(|f| {
                    let hits: Vec<usize> = par
                        .match_indices(&f.quote)
                        .map(|(k, _)| k)
                        .filter(|&k| on_boundary(par, k, k + f.quote.len()))
                        .collect();
                    (hits.len() == 1).then(|| Flag {
                        range: hits[0]..hits[0] + f.quote.len(),
                        ..f.clone()
                    })
                })
                .collect();
            if !kept.is_empty() {
                provisional.insert(i, kept);
            }
        }
        self.provisional = provisional;
    }

    /// The document context to send, refreshing the snapshot when it is
    /// old or has drifted too far from the current text. A stable snapshot
    /// keeps the prompt prefix identical, so it stays in the prompt cache.
    pub fn context(&mut self) -> String {
        let fresh = self.snapshot.as_ref().is_some_and(|s| {
            let changed = self.hashes.iter().filter(|h| !s.hashes.contains(h)).count();
            s.at.elapsed() < SNAPSHOT_MAX_AGE
                && (changed as f64) <= SNAPSHOT_MAX_CHANGED * self.hashes.len().max(1) as f64
        });
        if !fresh {
            self.snapshot = Some(Snapshot {
                text: self.with_includes(),
                hashes: self.hashes.iter().copied().collect(),
                at: Instant::now(),
            });
        }
        self.snapshot.as_ref().unwrap().text.clone()
    }

    /// The text plus any LaTeX `\input`/`\include` files, one level deep.
    fn with_includes(&self) -> String {
        let mut s = self.text.clone();
        let Some(dir) = self.path.as_ref().and_then(|p| p.parent()) else {
            return s;
        };
        for inc in &self.seg.includes {
            let mut p = dir.join(inc);
            if p.extension().is_none() {
                p.set_extension("tex");
            }
            if let Ok(t) = std::fs::read_to_string(&p) {
                s.push_str(&format!("\n\n%% included file: {inc}\n{t}"));
            }
        }
        s
    }
}

fn overlaps(a: &std::ops::Range<usize>, b: &std::ops::Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

fn shifted(r: &std::ops::Range<usize>, by: isize) -> std::ops::Range<usize> {
    let s = (r.start as isize + by).max(0) as usize;
    let e = (r.end as isize + by).max(0) as usize;
    s..e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flag::Category;

    #[test]
    fn carries_flags_while_editing() {
        let mut d = Doc::new(Lang::Markdown, None, "We recieve teh data.\n".into(), 1);
        let flags = vec![
            Flag { range: 3..10, quote: "recieve".into(), category: Category::Spelling, reason: None },
            Flag { range: 11..14, quote: "teh".into(), category: Category::Spelling, reason: None },
        ];
        let first = d.paragraph(0).to_string();
        let lookup = move |p: &str| (p == first).then(|| flags.clone());
        // User fixes "teh" and keeps typing.
        d.set_text("We recieve the data now.\n".into(), 2, &lookup);
        let p = &d.provisional[&0];
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].quote, "recieve");
        assert_eq!(p[0].range, 3..10);
    }

    #[test]
    fn opt_out_comment() {
        let d = Doc::new(Lang::Markdown, None, "<!-- flagcheck: off -->\nTeh.\n".into(), 1);
        assert!(d.disabled);
    }

    #[test]
    fn hash_is_stable() {
        // Pinned: ignored.txt files depend on this value.
        assert_eq!(paragraph_hash("abc"), PINNED);
    }

    const PINNED: u64 = 0xfc182483ee0806dc;
}
