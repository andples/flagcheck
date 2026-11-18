//! Markdown: front matter, fenced code, display maths, HTML and link targets
//! are masked; headings and list items are their own paragraphs.

use super::{Builder, line_end};

pub(super) fn scan(text: &str, b: &mut Builder) {
    let bytes = text.as_bytes();
    let mut pos = 0;

    // YAML/TOML front matter.
    for fence in ["---", "+++"] {
        if text.starts_with(&format!("{fence}\n")) {
            if let Some(k) = text[4..].find(&format!("\n{fence}")) {
                let end = line_end(text, 4 + k + 1);
                b.block(0..end);
                pos = end;
            }
        }
    }

    while pos < text.len() {
        let end = line_end(text, pos);
        let line = &text[pos..end];
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        // Fenced code: ``` or ~~~ until a matching fence.
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let fence = &trimmed[..3];
            let mut j = (end + 1).min(text.len());
            let mut close = text.len();
            while j < text.len() {
                let e = line_end(text, j);
                if text[j..e].trim_start().starts_with(fence) {
                    close = e;
                    break;
                }
                j = e + 1;
            }
            b.block(pos..close);
            pos = (close + 1).min(text.len());
            continue;
        }

        // Display maths on its own lines: $$ ... $$
        if trimmed.starts_with("$$") {
            let after = pos + indent + 2;
            let close = text[after..]
                .find("$$")
                .map(|k| after + k + 2)
                .unwrap_or(text.len());
            let close = line_end(text, close);
            b.block(pos..close);
            pos = (close + 1).min(text.len());
            continue;
        }

        // Reference-style link definitions: [id]: url
        if trimmed.starts_with('[') && trimmed.contains("]:") && !trimmed.starts_with("[^") {
            b.block(pos..end);
            pos = end + 1;
            continue;
        }

        // HTML block (a line that is only a tag or comment).
        if trimmed.starts_with('<') && trimmed.trim_end().ends_with('>') {
            b.block(pos..end);
            pos = end + 1;
            continue;
        }

        // Thematic break / table separator rows.
        let t = trimmed.trim_end();
        if !t.is_empty()
            && t.chars()
                .all(|c| matches!(c, '-' | '*' | '_' | '|' | ':' | ' ' | '='))
        {
            b.block(pos..end);
            pos = end + 1;
            continue;
        }

        // Block prefixes: headings, quotes, list markers.
        let mut k = pos + indent;
        let mut starts_block = false;
        loop {
            let rest = &text[k..end];
            if rest.starts_with('>') {
                k += 1;
            } else if let Some(n) = heading_marker(rest) {
                k += n;
                starts_block = true;
                // A heading is always its own paragraph.
                b.brk(end);
            } else if let Some(n) = list_marker(rest) {
                k += n;
                starts_block = true;
            } else {
                break;
            }
            while k < end && bytes[k] == b' ' {
                k += 1;
            }
        }
        if starts_block {
            b.brk(pos);
        }
        b.mask(pos..k);
        inline(text, k, end, b);
        pos = end + 1;
    }
}

fn heading_marker(s: &str) -> Option<usize> {
    let n = s.bytes().take_while(|&c| c == b'#').count();
    (1..=6)
        .contains(&n)
        .then_some(n)
        .filter(|&n| s.as_bytes().get(n).is_none_or(|&c| c == b' '))
}

fn list_marker(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.len() >= 2 && matches!(b[0], b'-' | b'*' | b'+') && b[1] == b' ' {
        let mut n = 2;
        // Task list box.
        if s[n..].starts_with("[ ] ") || s[n..].starts_with("[x] ") || s[n..].starts_with("[X] ") {
            n += 4;
        }
        return Some(n);
    }
    let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if (1..=9).contains(&digits)
        && b.len() > digits + 1
        && matches!(b[digits], b'.' | b')')
        && b[digits + 1] == b' '
    {
        return Some(digits + 2);
    }
    None
}

/// Inline constructs within one line.
fn inline(text: &str, start: usize, end: usize, b: &mut Builder) {
    let bytes = text.as_bytes();
    let mut i = start;
    while i < end {
        match bytes[i] {
            b'\\' => i += 2,
            b'`' => {
                let ticks = bytes[i..end].iter().take_while(|&&c| c == b'`').count();
                let pat = "`".repeat(ticks);
                let from = i + ticks;
                let close = text[from..end]
                    .find(&pat)
                    .map(|k| from + k + ticks)
                    .unwrap_or(end);
                b.mask(i..close);
                i = close;
            }
            b'$' => {
                let from = i + 1;
                match text[from..end].find('$') {
                    Some(k) if k > 0 && !text[from..from + k].starts_with(' ') => {
                        b.mask(i..from + k + 1);
                        i = from + k + 1;
                    }
                    _ => i += 1,
                }
            }
            b'<' => {
                // Inline HTML, comments, autolinks.
                let close = if text[i..end].starts_with("<!--") {
                    text[i..].find("-->").map(|k| i + k + 3).unwrap_or(end)
                } else {
                    match text[i..end].find('>') {
                        Some(k) if is_tag(&text[i + 1..i + k]) => i + k + 1,
                        _ => {
                            i += 1;
                            continue;
                        }
                    }
                };
                b.mask(i..close);
                i = close;
            }
            b']' if bytes.get(i + 1) == Some(&b'(') => {
                // Link target: ](url "title")
                let close = super::balanced(text, i + 1, b'(', b')').min(end);
                b.mask(i..close);
                i = close;
            }
            b'[' if bytes.get(i + 1) == Some(&b'^') => {
                let close = text[i..end].find(']').map(|k| i + k + 1).unwrap_or(end);
                b.mask(i..close);
                i = close;
            }
            b'h' if text[i..end].starts_with("http://") || text[i..end].starts_with("https://") => {
                let close = text[i..end]
                    .find(|c: char| c.is_whitespace() || c == ')' || c == '>')
                    .map(|k| i + k)
                    .unwrap_or(end);
                b.mask(i..close);
                i = close;
            }
            _ => i += 1,
        }
    }
}

fn is_tag(inner: &str) -> bool {
    let inner = inner.trim_start_matches('/');
    inner.starts_with("http")
        || inner.contains('@')
        || inner
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
            && !inner.contains(' ')
        || inner.contains('=')
}

#[cfg(test)]
mod tests {
    use crate::markup::{Lang, prose, segment};

    fn p(text: &str) -> Vec<String> {
        prose(text, &segment(Lang::Markdown, text))
    }

    #[test]
    fn paragraphs_and_headings() {
        let t =
            "# Title here\nFirst para\ncontinues.\n\nSecond `code` para.\n- item one\n- item two\n";
        assert_eq!(
            p(t),
            vec![
                "·Title here",
                "First para\ncontinues.",
                "Second · para.",
                "·item one",
                "·item two"
            ]
        );
    }

    #[test]
    fn code_maths_links_front_matter() {
        let t = "---\ntitle: x\n---\nSee [the docs](http://a.b/c) and $x^2$ here.\n\n```rust\nlet teh = 1;\n```\n$$\na = b\n$$\nEnd <!-- note --> now.\n";
        assert_eq!(p(t), vec!["See [the docs· and · here.", "End · now."]);
    }

    #[test]
    fn dollars_without_maths() {
        assert_eq!(p("It costs $ 5 and $ 6.\n"), vec!["It costs $ 5 and $ 6."]);
    }
}
