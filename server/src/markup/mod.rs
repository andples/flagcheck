//! Split markup source into prose paragraphs and a mask of non-prose bytes.
//!
//! The model sees raw source, so it can use the markup as context. The mask
//! is applied to its answers afterwards: a flag whose words are all masked
//! (inside maths, a citation key, a macro name...) is dropped. This keeps the
//! markup handling deterministic and testable instead of trusting the prompt.
//!
//! Each scanner is deliberately small and line/character based. They do not
//! need to parse documents correctly, only to never call markup "prose".

mod latex;
mod markdown;
mod typst;

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Markdown,
    Latex,
    Typst,
}

impl Lang {
    pub fn from_language_id(id: &str) -> Option<Lang> {
        match id.to_ascii_lowercase().as_str() {
            "markdown" | "md" | "mdx" | "quarto" | "rmarkdown" => Some(Lang::Markdown),
            "latex" | "tex" | "plaintex" | "context" | "bibtex" => Some(Lang::Latex),
            "typst" | "typ" => Some(Lang::Typst),
            _ => None,
        }
    }

    pub fn from_path(path: &str) -> Option<Lang> {
        let ext = path.rsplit('.').next()?.to_ascii_lowercase();
        match ext.as_str() {
            "md" | "markdown" | "mdx" | "qmd" | "rmd" => Some(Lang::Markdown),
            "tex" | "ltx" | "sty" | "cls" => Some(Lang::Latex),
            "typ" => Some(Lang::Typst),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Lang::Markdown => "markdown",
            Lang::Latex => "latex",
            Lang::Typst => "typst",
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Segmented {
    /// Byte ranges of prose paragraphs, in order, non-overlapping.
    pub paragraphs: Vec<Range<usize>>,
    /// Sorted, merged byte ranges that are never prose.
    pub mask: Vec<Range<usize>>,
    /// Files pulled in with `\input`/`\include` (LaTeX only), as written.
    pub includes: Vec<String>,
}

impl Segmented {
    pub fn is_masked(&self, i: usize) -> bool {
        let k = self.mask.partition_point(|r| r.end <= i);
        self.mask.get(k).is_some_and(|r| r.start <= i)
    }

    /// True when every alphanumeric byte in `r` is masked, i.e. the span
    /// holds no prose words at all.
    pub fn no_prose_in(&self, text: &str, r: Range<usize>) -> bool {
        text[r.clone()]
            .char_indices()
            .filter(|(_, c)| c.is_alphanumeric())
            .all(|(i, _)| self.is_masked(r.start + i))
    }

    pub fn any_masked_in(&self, r: Range<usize>) -> bool {
        let k = self.mask.partition_point(|m| m.end <= r.start);
        self.mask.get(k).is_some_and(|m| m.start < r.end)
    }
}

pub fn segment(lang: Lang, text: &str) -> Segmented {
    let mut b = Builder::new(text);
    match lang {
        Lang::Markdown => markdown::scan(text, &mut b),
        Lang::Latex => latex::scan(text, &mut b),
        Lang::Typst => typst::scan(text, &mut b),
    }
    b.finish()
}

/// Collects mask ranges and paragraph breaks, then cuts paragraphs.
pub(crate) struct Builder<'a> {
    text: &'a str,
    mask: Vec<Range<usize>>,
    /// Offsets where a paragraph must end/start (in addition to blank lines).
    breaks: Vec<usize>,
    /// Ranges that must not be part of any paragraph (code blocks etc.).
    exclude: Vec<Range<usize>>,
    includes: Vec<String>,
}

impl<'a> Builder<'a> {
    fn new(text: &'a str) -> Self {
        Builder {
            text,
            mask: vec![],
            breaks: vec![],
            exclude: vec![],
            includes: vec![],
        }
    }

    pub fn mask(&mut self, r: Range<usize>) {
        if r.start < r.end {
            self.mask.push(r);
        }
    }

    /// A block that is masked and also separates paragraphs.
    pub fn block(&mut self, r: Range<usize>) {
        if r.start < r.end {
            self.mask.push(r.clone());
            self.exclude.push(r);
        }
    }

    pub fn brk(&mut self, at: usize) {
        self.breaks.push(at);
    }

    pub fn include(&mut self, path: String) {
        self.includes.push(path);
    }

    fn finish(mut self) -> Segmented {
        let text = self.text;
        self.mask = merge(self.mask);
        let exclude = merge(self.exclude);

        // Candidate cut points: blank lines, explicit breaks, excluded blocks.
        let mut cuts: Vec<Range<usize>> = vec![];
        let mut pos = 0;
        for line in text.split_inclusive('\n') {
            if line.trim().is_empty() {
                cuts.push(pos..pos + line.len());
            }
            pos += line.len();
        }
        for &b in &self.breaks {
            cuts.push(b..b);
        }
        cuts.extend(exclude);
        cuts.sort_by_key(|r| (r.start, r.end));

        let seg = Segmented {
            paragraphs: vec![],
            mask: self.mask,
            includes: self.includes,
        };
        let mut paragraphs = vec![];
        let mut start = 0;
        for c in cuts {
            if c.start > start {
                paragraphs.push(start..c.start);
            }
            start = start.max(c.end);
        }
        if start < text.len() {
            paragraphs.push(start..text.len());
        }
        let paragraphs = paragraphs
            .into_iter()
            .filter_map(|r| trim(text, r))
            .filter(|r| !seg.no_prose_in(text, r.clone()))
            .collect();
        Segmented { paragraphs, ..seg }
    }
}

fn trim(text: &str, r: Range<usize>) -> Option<Range<usize>> {
    let s = &text[r.clone()];
    let lead = s.len() - s.trim_start().len();
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(r.start + lead..r.start + lead + t.len())
    }
}

fn merge(mut v: Vec<Range<usize>>) -> Vec<Range<usize>> {
    v.sort_by_key(|r| r.start);
    let mut out: Vec<Range<usize>> = vec![];
    for r in v {
        match out.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

/// Byte offset of the end of the line containing `i` (excluding '\n').
pub(crate) fn line_end(text: &str, i: usize) -> usize {
    text[i..].find('\n').map(|k| i + k).unwrap_or(text.len())
}

/// Given `open` at `i`, return the offset just past its matching `close`,
/// honouring nesting and backslash escapes. Falls back to the end of the
/// paragraph (next blank line) when unbalanced, so a stray brace cannot mask
/// the rest of the file.
pub(crate) fn balanced(text: &str, i: usize, open: u8, close: u8) -> usize {
    let bytes = text.as_bytes();
    debug_assert_eq!(bytes[i], open);
    let mut depth = 0usize;
    let mut j = i;
    let limit = text[i..]
        .find("\n\n")
        .map(|k| i + k)
        .unwrap_or(text.len());
    while j < limit {
        let b = bytes[j];
        if b == b'\\' {
            j += 2;
            continue;
        }
        if b == open {
            depth += 1;
        } else if b == close {
            depth -= 1;
            if depth == 0 {
                return j + 1;
            }
        }
        j += 1;
    }
    limit
}

#[cfg(test)]
pub(crate) fn prose(text: &str, seg: &Segmented) -> Vec<String> {
    // Paragraph text with masked bytes replaced, for readable assertions.
    seg.paragraphs
        .iter()
        .map(|r| {
            let mut s = String::new();
            for (i, c) in text[r.clone()].char_indices() {
                if seg.is_masked(r.start + i) {
                    if !s.ends_with('·') {
                        s.push('·');
                    }
                } else {
                    s.push(c);
                }
            }
            s
        })
        .collect()
}
