//! Typst: comments, maths, raw text, `#` code expressions (but not the
//! content blocks they take), labels and references are masked. Headings and
//! list items are their own paragraphs.

use super::{Builder, balanced, line_end};

pub(super) fn scan(text: &str, b: &mut Builder) {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut line_start = true;
    while i < text.len() {
        let c = bytes[i];
        if line_start {
            line_start = false;
            let indent = bytes[i..].iter().take_while(|&&c| c == b' ' || c == b'\t').count();
            let k = i + indent;
            let rest = &text[k..line_end(text, k)];
            let marker = if rest.starts_with('=') {
                let n = rest.bytes().take_while(|&c| c == b'=').count();
                if rest.as_bytes().get(n) == Some(&b' ') {
                    b.brk(line_end(text, k));
                    Some(n + 1)
                } else {
                    None
                }
            } else if rest.starts_with("- ") || rest.starts_with("+ ") {
                Some(2)
            } else if rest.starts_with("/ ") {
                // Term list: "/ Term: description"
                Some(2)
            } else {
                let d = rest.bytes().take_while(|c| c.is_ascii_digit()).count();
                (d > 0 && rest[d..].starts_with(". ")).then_some(d + 2)
            };
            if let Some(n) = marker {
                b.brk(i);
                b.mask(i..k + n);
                i = k + n;
                continue;
            }
        }
        match c {
            b'\n' => {
                line_start = true;
                i += 1;
            }
            b'\\' => {
                // Escape or forced line break.
                b.mask(i..i + 1);
                i += 1 + text[i + 1..].chars().next().map_or(0, |c| c.len_utf8());
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                let e = line_end(text, i);
                b.mask(i..e);
                i = e;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let e = text[i + 2..].find("*/").map(|k| i + 2 + k + 2).unwrap_or(text.len());
                b.mask(i..e);
                i = e;
            }
            b'$' => {
                let e = find_unescaped(text, i + 1, b'$').map(|k| k + 1).unwrap_or(text.len());
                b.mask(i..e);
                i = e;
            }
            b'`' => {
                let ticks = bytes[i..].iter().take_while(|&&c| c == b'`').count();
                let (pat, from) = if ticks >= 3 {
                    ("`".repeat(ticks), i + ticks)
                } else {
                    ("`".to_string(), i + 1)
                };
                let e = text[from..].find(&pat).map(|k| from + k + pat.len()).unwrap_or(text.len());
                if ticks >= 3 {
                    b.block(i..e);
                } else {
                    b.mask(i..e);
                }
                i = e;
            }
            b'<' => {
                // Label: <name>
                match text[i + 1..].find('>') {
                    Some(k) if is_label(&text[i + 1..i + 1 + k]) => {
                        b.mask(i..i + 2 + k);
                        i += 2 + k;
                    }
                    _ => i += 1,
                }
            }
            b'@' => {
                // Reference: @key, optionally followed by a [supplement].
                let n = ident_len(&text[i + 1..]);
                if n > 0 {
                    b.mask(i..i + 1 + n);
                    i += 1 + n;
                } else {
                    i += 1;
                }
            }
            b'#' => i = code(text, i, b),
            b'[' | b']' => {
                b.mask(i..i + 1);
                i += 1;
            }
            _ => i += 1,
        }
    }
}

/// Mask a `#` code expression starting at `i`; return where scanning resumes.
/// Content-block arguments (`[...]`) are left for the prose scanner.
fn code(text: &str, i: usize, b: &mut Builder) -> usize {
    let bytes = text.as_bytes();
    let rest = &text[i + 1..];
    for kw in ["set ", "show ", "let ", "import ", "include "] {
        if rest.starts_with(kw) {
            // Statement: to the end of the line, or a balanced group.
            let mut j = i + 1 + kw.len();
            let e = line_end(text, i);
            while j < e {
                match bytes[j] {
                    b'(' => j = balanced(text, j, b'(', b')'),
                    b'[' => j = balanced(text, j, b'[', b']'),
                    b'{' => j = balanced(text, j, b'{', b'}'),
                    _ => j += 1,
                }
            }
            let j = j.max(e);
            b.block(i..j);
            return j;
        }
    }
    if rest.starts_with('{') || rest.starts_with('(') {
        let (o, c) = if rest.starts_with('{') { (b'{', b'}') } else { (b'(', b')') };
        let e = balanced(text, i + 1, o, c);
        b.mask(i..e);
        return e;
    }
    // #name(.field)*(args)*[content]*
    let mut j = i + 1;
    loop {
        let n = ident_len(&text[j..]);
        if n == 0 {
            break;
        }
        j += n;
        if bytes.get(j) == Some(&b'.') && ident_len(&text[j + 1..]) > 0 {
            j += 1;
            continue;
        }
        break;
    }
    if j == i + 1 {
        return i + 1;
    }
    while bytes.get(j) == Some(&b'(') {
        j = balanced(text, j, b'(', b')');
    }
    b.mask(i..j);
    j
}

fn ident_len(s: &str) -> usize {
    let mut n = 0;
    for (k, c) in s.char_indices() {
        let ok = if k == 0 {
            c.is_alphabetic() || c == '_'
        } else {
            c.is_alphanumeric() || c == '_' || c == '-' || c == ':' || c == '.'
        };
        if !ok {
            break;
        }
        n = k + c.len_utf8();
    }
    // Trailing '.', ':' or '-' is punctuation, not part of the key.
    while n > 0 && matches!(s.as_bytes()[n - 1], b'.' | b':' | b'-') {
        n -= 1;
    }
    n
}

fn is_label(s: &str) -> bool {
    !s.is_empty() && !s.contains(char::is_whitespace) && ident_len(s) == s.len()
}

fn find_unescaped(text: &str, from: usize, c: u8) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut j = from;
    while j < bytes.len() {
        if bytes[j] == b'\\' {
            j += 2;
            continue;
        }
        if bytes[j] == c {
            return Some(j);
        }
        j += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::markup::{Lang, prose, segment};

    fn p(text: &str) -> Vec<String> {
        prose(text, &segment(Lang::Typst, text))
    }

    #[test]
    fn headings_code_maths() {
        let t = "#set page(width: 10cm)\n#import \"x.typ\": *\n= Introduction <intro>\nWe show $x^2$ grows, see @intro and #emph[this text].\n// teh comment\n- first item\n- second item\n";
        assert_eq!(
            p(t),
            vec![
                "·Introduction ·",
                "We show · grows, see · and ·this text·.\n·",
                "·first item",
                "·second item",
            ]
        );
    }

    #[test]
    fn raw_blocks() {
        let t = "Use `teh` here.\n\n```py\nteh = 1\n```\nDone.\n";
        assert_eq!(p(t), vec!["Use · here.", "Done."]);
    }
}
