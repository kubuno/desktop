//! Byte offset ⇄ LSP [`Position`] conversion.
//!
//! `kubuno_views::syntax::LineIndex` (used internally by the parser/validator
//! for its own `Diagnostic::line`/`column`) is **not** reused here: it is
//! 1-based and counts columns in UTF-8 *bytes*, which is exactly right for a
//! human-facing "file:line:col" message but wrong for LSP, whose
//! `Position::character` is defined by the spec as a **UTF-16 code unit**
//! offset within the line (0-based line and character). A `.kbview` file with
//! any non-ASCII text (an accented placeholder, an emoji icon name…) would
//! otherwise put every diagnostic/completion/hover after that point at the
//! wrong column in the editor. So this module is a second, LSP-shaped index,
//! built fresh per document text — see `kubuno-views` §6 note in
//! `XML_VIEWS.md`: this crate is one of exactly the "language server" readers
//! that design anticipated needing its own UTF-16 conversion.

use lsp_types::Position;
use rowan::TextSize;

/// Precomputed byte offsets of each line's start, for O(log n) line lookup.
/// Rebuilt whenever a document's text changes (full sync — see
/// `documents.rs`), same lifetime as the text it indexes.
#[derive(Debug, Clone)]
pub struct PositionIndex {
    /// `line_starts[0] == 0`; `line_starts[i]` is the byte offset right after
    /// the `i`-th `\n`. Mirrors `kubuno_views::syntax::LineIndex`'s own
    /// construction exactly (byte offset of the character right after each
    /// `\n`), which is what keeps a line's `\r` (CRLF) attached to the
    /// *previous* line's content rather than the next line's — consistent
    /// with how `kubuno-views` itself treats line boundaries.
    line_starts: Vec<u32>,
}

impl PositionIndex {
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0u32];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        Self { line_starts }
    }

    /// A UTF-8 byte offset into `text` → a 0-based LSP `Position` (line,
    /// UTF-16 character). `text` must be the same text this index was built
    /// from (or an equal-prefix superset up to `offset`).
    pub fn offset_to_position(&self, text: &str, offset: TextSize) -> Position {
        let offset: u32 = offset.into();
        let offset = offset.min(text.len() as u32); // Defensive clamp — never index out of bounds.
        let line = match self.line_starts.binary_search(&offset) {
            Ok(exact) => exact,
            Err(insert_at) => insert_at.saturating_sub(1),
        };
        let line_start = self.line_starts[line];
        // UTF-16 code units from the line's start up to `offset` — correct
        // regardless of what lies beyond `offset` on the line (a trailing
        // `\r`, more content, or the line terminator itself never enters this
        // slice).
        let character = text[line_start as usize..offset as usize].encode_utf16().count() as u32;
        Position { line: line as u32, character }
    }

    /// The inverse of [`Self::offset_to_position`]: a 0-based LSP `Position`
    /// → a UTF-8 byte offset into `text`. Out-of-range input (a line past the
    /// end of the file, or a character past the end of its line — both
    /// legitimate transiently while an editor's buffer and the server's last
    /// known text are briefly out of sync) is clamped rather than rejected,
    /// so a caller never has to fall back to "do nothing" for a slightly
    /// stale position.
    pub fn position_to_offset(&self, text: &str, pos: Position) -> TextSize {
        let line = (pos.line as usize).min(self.line_starts.len() - 1);
        let line_start = self.line_starts[line] as usize;
        let line_end = self.line_starts.get(line + 1).map(|&e| e as usize).unwrap_or(text.len());
        // `line_end` (per `line_starts`' own construction — the byte right
        // after a `\n`) includes the line terminator itself in
        // `text[line_start..line_end]`. That is the right boundary for
        // *finding* which line an offset falls on (`offset_to_position`
        // mirrors it), but clamping an out-of-range `character` to "end of
        // line" must land *before* the terminator (`\n`, or `\r\n`), never
        // consume it — otherwise a character far past a short line's end
        // would clamp one byte into the next line instead of to this line's
        // true end.
        let mut line_text = &text[line_start..line_end];
        if let Some(stripped) = line_text.strip_suffix('\n') {
            line_text = stripped.strip_suffix('\r').unwrap_or(stripped);
        }

        let mut byte_offset = line_start;
        let mut units_seen = 0u32;
        for ch in line_text.chars() {
            if units_seen >= pos.character {
                break;
            }
            units_seen += ch.len_utf16() as u32;
            byte_offset += ch.len_utf8();
        }
        TextSize::from(byte_offset as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_round_trips() {
        let text = "abc\ndef";
        let idx = PositionIndex::new(text);
        let pos = idx.offset_to_position(text, TextSize::from(5)); // 'e'
        assert_eq!(pos, Position { line: 1, character: 1 });
        assert_eq!(idx.position_to_offset(text, pos), TextSize::from(5));
    }

    #[test]
    fn first_position_is_zero_zero() {
        let text = "abc";
        let idx = PositionIndex::new(text);
        assert_eq!(idx.offset_to_position(text, TextSize::from(0)), Position { line: 0, character: 0 });
    }

    #[test]
    fn multibyte_bmp_character_counts_as_one_utf16_unit() {
        // 'é' is 2 UTF-8 bytes, 1 UTF-16 code unit.
        let text = "<A Text=\"é\"/>";
        let idx = PositionIndex::new(text);
        let e_byte_offset = text.find('é').unwrap() as u32;
        let pos = idx.offset_to_position(text, TextSize::from(e_byte_offset));
        assert_eq!(pos.character, e_byte_offset); // Everything before 'é' here is ASCII, so byte == UTF-16 count up to it.
        // The byte right after 'é' (2 UTF-8 bytes later) is 1 UTF-16 unit later.
        let after = idx.offset_to_position(text, TextSize::from(e_byte_offset + 2));
        assert_eq!(after.character, pos.character + 1);
    }

    #[test]
    fn astral_character_counts_as_two_utf16_units() {
        // An emoji outside the BMP is 4 UTF-8 bytes but a UTF-16 surrogate
        // pair (2 code units) — the case a naive "one code point = one
        // column" mapping gets wrong.
        let text = "<A Icon=\"🚀\"/>";
        let idx = PositionIndex::new(text);
        let rocket_start = text.find('🚀').unwrap() as u32;
        let before = idx.offset_to_position(text, TextSize::from(rocket_start));
        let after = idx.offset_to_position(text, TextSize::from(rocket_start + 4)); // 🚀 is 4 UTF-8 bytes
        assert_eq!(after.character, before.character + 2);
    }

    #[test]
    fn position_to_offset_is_the_inverse_for_multibyte_text() {
        let text = "<A Text=\"héllo\"/>";
        let idx = PositionIndex::new(text);
        for byte_offset in [0u32, 5, 9, 10, 11, text.len() as u32] {
            if !text.is_char_boundary(byte_offset as usize) {
                continue;
            }
            let pos = idx.offset_to_position(text, TextSize::from(byte_offset));
            let back = idx.position_to_offset(text, pos);
            assert_eq!(u32::from(back), byte_offset, "round-trip failed for byte {byte_offset}");
        }
    }

    #[test]
    fn crlf_line_breaks_keep_carriage_return_on_the_previous_line() {
        let text = "abc\r\ndef";
        let idx = PositionIndex::new(text);
        // 'd' is right after the '\n' — line 1, character 0.
        let d_offset = text.find('d').unwrap() as u32;
        assert_eq!(idx.offset_to_position(text, TextSize::from(d_offset)), Position { line: 1, character: 0 });
        // The '\r' itself is still on line 0, at character 3.
        let cr_offset = text.find('\r').unwrap() as u32;
        assert_eq!(idx.offset_to_position(text, TextSize::from(cr_offset)), Position { line: 0, character: 3 });
    }

    #[test]
    fn second_line_first_column_after_newline() {
        let text = "one\ntwo\nthree";
        let idx = PositionIndex::new(text);
        assert_eq!(idx.offset_to_position(text, TextSize::from(4)), Position { line: 1, character: 0 });
        assert_eq!(idx.offset_to_position(text, TextSize::from(8)), Position { line: 2, character: 0 });
    }

    #[test]
    fn position_past_end_of_line_clamps_to_line_end() {
        let text = "ab\ncd";
        let idx = PositionIndex::new(text);
        let pos = Position { line: 0, character: 100 }; // Way past "ab".
        let offset = idx.position_to_offset(text, pos);
        assert_eq!(u32::from(offset), 2); // Clamped to just before the '\n'.
    }

    #[test]
    fn position_past_end_of_file_clamps_to_last_line() {
        let text = "ab\ncd";
        let idx = PositionIndex::new(text);
        let pos = Position { line: 50, character: 0 };
        let offset = idx.position_to_offset(text, pos);
        assert_eq!(u32::from(offset), 3); // Start of the last line ("cd").
    }
}
