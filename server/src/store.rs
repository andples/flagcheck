//! Per-project state in `.flagcheck/` at the workspace root:
//! `words.txt` (one word per line) and `ignored.txt` (paragraph hash, tab,
//! quote). Both are plain text so they can be reviewed and committed.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct Store {
    dir: Option<PathBuf>,
    pub words: HashSet<String>,
    pub ignored: HashSet<(u64, String)>,
}

impl Store {
    pub fn load(root: Option<&Path>) -> Self {
        let dir = root.map(|r| r.join(".flagcheck"));
        let mut s = Store {
            dir,
            ..Default::default()
        };
        if let Some(d) = &s.dir {
            if let Ok(t) = std::fs::read_to_string(d.join("words.txt")) {
                s.words = t
                    .lines()
                    .map(|l| l.trim().to_lowercase())
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .collect();
            }
            if let Ok(t) = std::fs::read_to_string(d.join("ignored.txt")) {
                s.ignored = t
                    .lines()
                    .filter_map(|l| {
                        let (h, q) = l.split_once('\t')?;
                        Some((u64::from_str_radix(h, 16).ok()?, unescape(q)))
                    })
                    .collect();
            }
        }
        s
    }

    pub fn add_word(&mut self, w: &str) -> std::io::Result<()> {
        let w = w.trim().to_lowercase();
        if w.is_empty() || !self.words.insert(w.clone()) {
            return Ok(());
        }
        self.append("words.txt", &w)
    }

    pub fn ignore(&mut self, hash: u64, quote: &str) -> std::io::Result<()> {
        if !self.ignored.insert((hash, quote.to_string())) {
            return Ok(());
        }
        self.append("ignored.txt", &format!("{hash:016x}\t{}", escape(quote)))
    }

    pub fn is_ignored(&self, hash: u64, quote: &str) -> bool {
        self.ignored.contains(&(hash, quote.to_string()))
    }

    /// A spelling flag on a listed word (case-insensitive) is suppressed.
    pub fn has_word(&self, quote: &str) -> bool {
        let q = quote.trim().to_lowercase();
        !q.is_empty() && self.words.contains(&q)
    }

    fn append(&self, file: &str, line: &str) -> std::io::Result<()> {
        let Some(d) = &self.dir else { return Ok(()) };
        std::fs::create_dir_all(d)?;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(d.join(file))?;
        writeln!(f, "{line}")
    }
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace('\t', "\\t")
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("flagcheck-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        {
            let mut s = Store::load(Some(&dir));
            s.add_word("Kubernetes").unwrap();
            s.ignore(0xabc, "a\nb\tc").unwrap();
        }
        let s = Store::load(Some(&dir));
        assert!(s.has_word("kubernetes"));
        assert!(s.is_ignored(0xabc, "a\nb\tc"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
