//! LaTeX: comments, maths, verbatim-like environments, macro names and the
//! arguments of reference-like macros are masked. Sectioning commands,
//! `\item` and environment boundaries start new paragraphs.

use super::{Builder, balanced, line_end};

/// Environments whose whole body is not prose.
/// Maths environments are masked but do not split the paragraph, because
/// display maths usually sits inside a sentence.
const MATH_ENVS: &[&str] = &[
    "equation", "equation*", "align", "align*", "alignat", "alignat*", "gather", "gather*",
    "multline", "multline*", "flalign", "flalign*", "eqnarray", "eqnarray*", "math",
    "displaymath", "split", "subequations",
];

/// Environments whose whole body is not prose, and which end a paragraph.
const OPAQUE_ENVS: &[&str] = &[
    "verbatim", "Verbatim", "lstlisting", "minted", "comment", "tikzpicture",
    "filecontents", "filecontents*", "thebibliography", "pgfpicture", "axis",
];

/// Macros whose arguments are not prose (all of them, required and optional).
const OPAQUE_ARGS: &[&str] = &[
    "cite", "citep", "citet", "citealp", "citealt", "citeauthor", "citeyear", "parencite",
    "textcite", "autocite", "footcite", "nocite", "ref", "eqref", "autoref", "cref", "Cref",
    "pageref", "nameref", "vref", "label", "url", "href", "includegraphics", "input",
    "include", "includeonly", "bibliography", "bibliographystyle", "addbibresource",
    "usepackage", "RequirePackage", "documentclass", "begin", "end", "newcommand",
    "renewcommand", "providecommand", "DeclareMathOperator", "newenvironment",
    "renewenvironment", "def", "let", "setlength", "addtolength", "setcounter",
    "addtocounter", "vspace", "hspace", "vskip", "hskip", "color", "textcolor",
    "pagecolor", "definecolor", "graphicspath", "lstinline", "mintinline", "verb",
    "pagestyle", "thispagestyle", "pagenumbering", "bibitem", "newtheorem", "hypersetup",
    "geometry", "fontsize", "linespread", "resizebox", "scalebox", "rule", "raisebox",
    "multicolumn", "multirow", "cline", "cmidrule", "column", "tag", "numberwithin",
    "crefname", "Crefname", "SI", "si", "num", "qty", "unit", "ensuremath",
];

/// Macros whose first argument is prose but which start a new block.
const SECTIONING: &[&str] = &[
    "part", "chapter", "section", "subsection", "subsubsection", "paragraph",
    "subparagraph", "caption", "title", "author", "date", "item",
    "maketitle", "tableofcontents", "par", "newpage", "clearpage", "noindent", "centering",
    "abstract", "keywords",
];

pub(super) fn scan(text: &str, b: &mut Builder) {
    let bytes = text.as_bytes();
    let mut i = 0;

    // Preamble: everything before \begin{document} is configuration.
    if let Some(k) = text.find("\\begin{document}") {
        let mut j = 0;
        // Keep \title{...} and \author{...} prose visible inside the preamble.
        while j < k {
            match find_macro(text, j, k, &["title", "author"]) {
                Some(arg) => {
                    b.block(j..arg.start);
                    b.brk(arg.end);
                    j = arg.end;
                }
                None => {
                    b.block(j..k);
                    j = k;
                }
            }
        }
        i = k;
    }

    while i < text.len() {
        match bytes[i] {
            b'%' => {
                let e = line_end(text, i);
                b.mask(i..e);
                i = e;
            }
            b'$' => {
                let display = bytes.get(i + 1) == Some(&b'$');
                let open = if display { 2 } else { 1 };
                let close = find_unescaped(text, i + open, if display { "$$" } else { "$" })
                    .map(|k| k + open)
                    .unwrap_or_else(|| line_end(text, i));
                b.mask(i..close);
                i = close;
            }
            b'&' => {
                b.mask(i..i + 1);
                i += 1;
            }
            b'~' => {
                // Non-breaking space: harmless, but not a word.
                i += 1;
            }
            b'\\' => {
                let name_end = i + 1 + bytes[i + 1..]
                    .iter()
                    .take_while(|c| c.is_ascii_alphabetic() || **c == b'@')
                    .count();
                let name = &text[i + 1..name_end];
                let mut ne = name_end;
                if name.is_empty() {
                    // Control symbol: \\, \[, \(, \%, \, etc.
                    let Some(c) = text[i + 1..].chars().next() else {
                        b.mask(i..i + 1);
                        break;
                    };
                    match c {
                        '[' | '(' => {
                            let pat = if c == '[' { "\\]" } else { "\\)" };
                            let close = text[i + 2..]
                                .find(pat)
                                .map(|k| i + 2 + k + 2)
                                .unwrap_or_else(|| line_end(text, i));
                            b.mask(i..close);
                            i = close;
                        }
                        '\\' => {
                            b.mask(i..i + 2);
                            i += 2;
                        }
                        _ => {
                            // \%, \&, \_ etc. are printed characters; mask the
                            // backslash only.
                            b.mask(i..i + 1);
                            i += 1 + c.len_utf8();
                        }
                    }
                    continue;
                }
                if text[ne..].starts_with('*') {
                    ne += 1;
                }
                let star_name = &text[i + 1..ne];
                if name == "verb" || name == "lstinline" {
                    // \verb|...|
                    let delim = text[ne..].chars().next().unwrap_or('|');
                    let from = ne + delim.len_utf8();
                    let close = text[from..]
                        .find(delim)
                        .map(|k| from + k + delim.len_utf8())
                        .unwrap_or_else(|| line_end(text, i));
                    b.mask(i..close);
                    i = close;
                    continue;
                }
                if name == "begin" || name == "end" {
                    let env = brace_arg(text, ne);
                    let env_name = env.as_ref().map(|r| &text[r.start + 1..r.end - 1]);
                    let close = env.as_ref().map(|r| r.end).unwrap_or(ne);
                    let opaque = env_name.filter(|e| OPAQUE_ENVS.contains(e) || MATH_ENVS.contains(e));
                    if let (true, Some(e)) = (name == "begin", opaque) {
                        let end_pat = format!("\\end{{{e}}}");
                        let block_end = text[close..]
                            .find(&end_pat)
                            .map(|k| close + k + end_pat.len())
                            .unwrap_or(text.len());
                        if MATH_ENVS.contains(&e) {
                            b.mask(i..block_end);
                        } else {
                            b.block(i..block_end);
                        }
                        i = block_end;
                        continue;
                    }
                    // Mask \begin{env} plus any [opt]/{arg} right after it
                    // (e.g. tabular column specs).
                    let mut j = close;
                    while let Some(r) = brace_arg(text, j).or_else(|| bracket_arg(text, j)) {
                        if name == "end" {
                            break;
                        }
                        j = r.end;
                    }
                    b.block(i..j);
                    i = j;
                    continue;
                }
                if name == "input" || name == "include" || name == "subfile" {
                    if let Some(r) = brace_arg(text, ne) {
                        b.include(text[r.start + 1..r.end - 1].trim().to_string());
                    }
                }
                if SECTIONING.contains(&name) {
                    b.brk(i);
                    if name != "item" {
                        // A heading's argument is a paragraph on its own.
                        let mut j = ne;
                        while let Some(r) = bracket_arg(text, j) {
                            j = r.end;
                        }
                        if let Some(r) = brace_arg(text, j) {
                            b.brk(r.end);
                        }
                    }
                }
                if OPAQUE_ARGS.contains(&name) || OPAQUE_ARGS.contains(&star_name) {
                    let mut j = ne;
                    while let Some(r) = brace_arg(text, j).or_else(|| bracket_arg(text, j)) {
                        j = r.end;
                    }
                    b.mask(i..j);
                    i = j;
                    continue;
                }
                // Prose-carrying macro: mask only the name and optional args.
                b.mask(i..ne);
                let mut j = ne;
                while let Some(r) = bracket_arg(text, j) {
                    b.mask(r.clone());
                    j = r.end;
                }
                i = j;
            }
            b'{' | b'}' => {
                b.mask(i..i + 1);
                i += 1;
            }
            _ => i += 1,
        }
    }
}

/// `{...}` starting at `i` (after optional spaces), as a byte range
/// including the braces.
fn brace_arg(text: &str, i: usize) -> Option<std::ops::Range<usize>> {
    let j = skip_spaces(text, i);
    (text.as_bytes().get(j) == Some(&b'{')).then(|| j..balanced(text, j, b'{', b'}'))
}

fn bracket_arg(text: &str, i: usize) -> Option<std::ops::Range<usize>> {
    let j = skip_spaces(text, i);
    (text.as_bytes().get(j) == Some(&b'[')).then(|| j..balanced(text, j, b'[', b']'))
}

fn skip_spaces(text: &str, mut i: usize) -> usize {
    // Do not cross a blank line.
    let bytes = text.as_bytes();
    let mut newlines = 0;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n') {
        if bytes[i] == b'\n' {
            newlines += 1;
            if newlines > 1 {
                break;
            }
        }
        i += 1;
    }
    i
}

fn find_unescaped(text: &str, from: usize, pat: &str) -> Option<usize> {
    let mut j = from;
    while let Some(k) = text[j..].find(pat) {
        let at = j + k;
        if at > 0 && text.as_bytes()[at - 1] == b'\\' {
            j = at + 1;
            continue;
        }
        return Some(at);
    }
    None
}

/// Find the next `\name{...}` among `names` in `text[from..to]`; return the
/// argument's inner range.
fn find_macro(
    text: &str,
    from: usize,
    to: usize,
    names: &[&str],
) -> Option<std::ops::Range<usize>> {
    let mut j = from;
    while let Some(k) = text[j..to].find('\\') {
        let at = j + k;
        for n in names {
            let after = at + 1 + n.len();
            if text[at + 1..].starts_with(n)
                && !text.as_bytes().get(after).is_some_and(|c| c.is_ascii_alphabetic())
            {
                if let Some(r) = brace_arg(text, after) {
                    if r.end <= to {
                        return Some(r.start + 1..r.end - 1);
                    }
                }
            }
        }
        j = at + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::markup::{Lang, prose, segment};

    fn p(text: &str) -> Vec<String> {
        prose(text, &segment(Lang::Latex, text))
    }

    #[test]
    fn body_maths_citations() {
        let t = "\\documentclass{article}\n\\usepackage{amsmath}\n\\title{A Study}\n\\begin{document}\n\\section{Intro}\nWe show $x^2$ is big \\cite{knuth84}, see Fig.~\\ref{fig:a}. % todo teh\nMore \\emph{text} here.\n\n\\begin{equation}\na = b\n\\end{equation}\nAfter.\n\\end{document}\n";
        assert_eq!(
            p(t),
            vec![
                "A Study",
                "·Intro·",
                "We show · is big ·, see Fig.~·. ·\nMore ·text· here.",
                "·\nAfter.",
            ]
        );
    }

    #[test]
    fn items_and_includes() {
        let t = "\\begin{itemize}\n\\item First one.\n\\item Second one.\n\\end{itemize}\n\\input{chapters/intro}\n";
        let seg = segment(Lang::Latex, t);
        assert_eq!(prose(t, &seg), vec!["· First one.", "· Second one."]);
        assert_eq!(seg.includes, vec!["chapters/intro"]);
    }

    #[test]
    fn escapes_and_verb() {
        let t = "Costs 5\\% more \\verb|teh| and \\(a+b\\) or \\[c\\] done.\n";
        assert_eq!(p(t), vec!["Costs 5·% more · and · or · done."]);
    }
}
