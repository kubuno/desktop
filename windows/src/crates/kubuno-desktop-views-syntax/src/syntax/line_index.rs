//! Byte offset → 1-based line/column, for diagnostics.

use rowan::TextSize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    /// 1-based.
    pub line: u32,
    /// 1-based, counted in UTF-8 bytes from the start of the line (adequate
    /// for the `.kbview` grammar, which has no wide-character alignment
    /// concerns of its own; an LSP-facing UTF-16 conversion, if ever needed,
    /// is a consumer-side concern per `mod.rs`'s `Diagnostic` doc comment).
    pub column: u32,
}

/// The byte offset of the start of each line, built once per parse.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// `line_starts[0] == 0`; `line_starts[i]` is the offset right after the
    /// `i`-th newline.
    line_starts: Vec<u32>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0u32];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        Self { line_starts }
    }

    pub fn line_col(&self, offset: TextSize) -> LineCol {
        let offset: u32 = offset.into();
        // Last line whose start is <= offset.
        let line = match self.line_starts.binary_search(&offset) {
            Ok(exact) => exact,
            Err(insert_at) => insert_at.saturating_sub(1),
        };
        let line_start = self.line_starts[line];
        LineCol { line: line as u32 + 1, column: (offset - line_start) + 1 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_first_column() {
        let idx = LineIndex::new("abc");
        assert_eq!(idx.line_col(TextSize::from(0)), LineCol { line: 1, column: 1 });
    }

    #[test]
    fn after_a_newline_is_line_two() {
        let idx = LineIndex::new("ab\ncd");
        // 'c' is right after the newline at offset 2.
        assert_eq!(idx.line_col(TextSize::from(3)), LineCol { line: 2, column: 1 });
        assert_eq!(idx.line_col(TextSize::from(4)), LineCol { line: 2, column: 2 });
    }

    #[test]
    fn several_lines() {
        let idx = LineIndex::new("one\ntwo\nthree");
        // "one\ntwo\nthree": 'o'=0,'n'=1,'e'=2,'\n'=3,'t'=4,'w'=5,'o'=6,'\n'=7,'t'=8(start of "three").
        assert_eq!(idx.line_col(TextSize::from(8)), LineCol { line: 3, column: 1 }); // 't' of "three"
        assert_eq!(idx.line_col(TextSize::from(9)), LineCol { line: 3, column: 2 }); // 'h' of "three"
    }
}
