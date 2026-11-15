//! Byte offsets <-> LSP positions.
//!
//! Internally every offset is a UTF-8 byte offset into the document text.
//! LSP positions count "characters" in the negotiated encoding: UTF-16 code
//! units unless the client offered something else. Getting this wrong shifts
//! underlines on any line with non-ASCII text.

use tower_lsp_server::ls_types::{Position, PositionEncodingKind, Range as LspRange};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16,
    Utf32,
}

impl Encoding {
    /// Pick the encoding from the client's offered list (in preference
    /// order). UTF-8 is cheapest for us, so take it when offered.
    pub fn negotiate(offered: Option<&[PositionEncodingKind]>) -> Self {
        let Some(offered) = offered else {
            return Encoding::Utf16;
        };
        if offered.contains(&PositionEncodingKind::UTF8) {
            Encoding::Utf8
        } else {
            Encoding::Utf16
        }
    }

    pub fn kind(self) -> PositionEncodingKind {
        match self {
            Encoding::Utf8 => PositionEncodingKind::UTF8,
            Encoding::Utf16 => PositionEncodingKind::UTF16,
            Encoding::Utf32 => PositionEncodingKind::UTF32,
        }
    }

    fn units(self, c: char) -> u32 {
        match self {
            Encoding::Utf8 => c.len_utf8() as u32,
            Encoding::Utf16 => c.len_utf16() as u32,
            Encoding::Utf32 => 1,
        }
    }
}

/// Start offsets of every line, for fast conversion.
pub struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        LineIndex { starts }
    }

    pub fn position(&self, text: &str, offset: usize, enc: Encoding) -> Position {
        let offset = floor_char_boundary(text, offset.min(text.len()));
        let line = match self.starts.binary_search(&offset) {
            Ok(l) => l,
            Err(l) => l - 1,
        };
        let start = self.starts[line];
        let character = text[start..offset].chars().map(|c| enc.units(c)).sum();
        Position::new(line as u32, character)
    }

    pub fn range(&self, text: &str, r: std::ops::Range<usize>, enc: Encoding) -> LspRange {
        LspRange::new(
            self.position(text, r.start, enc),
            self.position(text, r.end, enc),
        )
    }

    /// LSP position -> byte offset. Positions past the end of a line clamp
    /// to the line end, as the spec requires.
    pub fn offset(&self, text: &str, pos: Position, enc: Encoding) -> usize {
        let line = pos.line as usize;
        if line >= self.starts.len() {
            return text.len();
        }
        let start = self.starts[line];
        let end = self
            .starts
            .get(line + 1)
            .map(|&s| s - 1)
            .unwrap_or(text.len());
        let mut units = 0u32;
        for (i, c) in text[start..end].char_indices() {
            if units >= pos.character {
                return start + i;
            }
            units += enc.units(c);
        }
        end
    }
}

pub fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_counts_surrogate_pairs() {
        let text = "a😀b\nééx";
        let idx = LineIndex::new(text);
        let b = text.find('b').unwrap();
        assert_eq!(idx.position(text, b, Encoding::Utf16), Position::new(0, 3));
        assert_eq!(idx.position(text, b, Encoding::Utf8), Position::new(0, 5));
        assert_eq!(idx.position(text, b, Encoding::Utf32), Position::new(0, 2));
        let x = text.find('x').unwrap();
        assert_eq!(idx.position(text, x, Encoding::Utf16), Position::new(1, 2));
    }

    #[test]
    fn offset_round_trips() {
        let text = "héllo 😀 wörld\nsecond line\n";
        let idx = LineIndex::new(text);
        for enc in [Encoding::Utf8, Encoding::Utf16, Encoding::Utf32] {
            for (i, _) in text.char_indices() {
                let p = idx.position(text, i, enc);
                assert_eq!(idx.offset(text, p, enc), i, "{enc:?} at {i}");
            }
        }
    }

    #[test]
    fn offset_clamps_past_line_end() {
        let text = "ab\ncd";
        let idx = LineIndex::new(text);
        assert_eq!(idx.offset(text, Position::new(0, 99), Encoding::Utf16), 2);
        assert_eq!(idx.offset(text, Position::new(9, 0), Encoding::Utf16), 5);
    }
}
