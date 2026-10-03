//! Byte offsets → 1-based line / UTF-16 column, the unit TypeScript, source maps and editors use.
//!
//! `kubuno_views_syntax::syntax::LineIndex` counts columns in UTF-8 bytes (the desktop's choice); the web
//! outputs (diagnostics shown by Vite and `kbview-tsc`, source maps, check-file maps) all speak UTF-16, so
//! the web compiler converts once here.

/// A 1-based line and a 1-based column counted in UTF-16 code units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Pos {
    pub line: u32,
    pub column: u32,
}

/// The line starts of one text, for repeated offset → position conversions.
pub struct Lines<'t> {
    text: &'t str,
    starts: Vec<usize>,
}

impl<'t> Lines<'t> {
    pub fn new(text: &'t str) -> Self {
        let mut starts = vec![0usize];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        Self { text, starts }
    }

    /// The position of byte `offset` (clamped to the text; an offset inside a multi-byte character counts
    /// the units before that character).
    pub fn pos(&self, offset: usize) -> Pos {
        let offset = offset.min(self.text.len());
        let line = match self.starts.binary_search(&offset) {
            Ok(exact) => exact,
            Err(insert) => insert.saturating_sub(1),
        };
        let start = self.starts[line];
        let mut units = 0u32;
        for (i, c) in self.text[start..].char_indices() {
            if start + i >= offset {
                break;
            }
            units += c.len_utf16() as u32;
        }
        Pos { line: line as u32 + 1, column: units + 1 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_utf16_units() {
        let text = "ab\nçé𝄞x";
        let lines = Lines::new(text);
        assert_eq!(lines.pos(0), Pos { line: 1, column: 1 });
        assert_eq!(lines.pos(3), Pos { line: 2, column: 1 });
        // `ç` and `é` are 2 bytes / 1 unit each, `𝄞` 4 bytes / 2 units.
        let x = text.find('x').expect("x");
        assert_eq!(lines.pos(x), Pos { line: 2, column: 5 });
        assert_eq!(lines.pos(10_000), Pos { line: 2, column: 6 });
    }
}
