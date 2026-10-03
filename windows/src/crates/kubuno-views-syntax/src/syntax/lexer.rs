//! A hand-written tokenizer for the `.kbview` subset of XML.
//!
//! Total coverage is the load-bearing property: every byte of the input
//! becomes part of exactly one token (including whitespace, comments and
//! unrecognised bytes as [`SyntaxKind::ERROR_TOKEN`]), so the parser can
//! rebuild the tree losslessly no matter how malformed the input is. This is
//! what makes the round-trip property (`text(parse(s)) == s`) hold
//! unconditionally rather than only for well-formed files.

use super::SyntaxKind;
use rowan::{TextRange, TextSize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LexToken {
    pub kind: SyntaxKind,
    pub range: TextRange,
}

/// Lexes `text` into a flat token stream. Never fails: unrecognised bytes
/// become [`SyntaxKind::ERROR_TOKEN`]s rather than aborting the scan.
pub fn lex(text: &str) -> Vec<LexToken> {
    Lexer { text, bytes: text.as_bytes(), pos: 0, in_tag: false, pi_mode: false }.run()
}

struct Lexer<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: u32,
    /// Whether we are between `<`/`</`/`<?` and the matching `>`/`/>`/`?>`.
    in_tag: bool,
    /// Whether the tag currently open is a processing instruction (`<? … ?>`),
    /// so its closer is `?>` rather than `>`/`/>`.
    pi_mode: bool,
}

impl<'a> Lexer<'a> {
    fn len(&self) -> u32 {
        self.bytes.len() as u32
    }

    fn at(&self, offset: u32) -> Option<u8> {
        self.bytes.get(offset as usize).copied()
    }

    fn starts_with(&self, at: u32, needle: &str) -> bool {
        self.text.get(at as usize..).is_some_and(|rest| rest.starts_with(needle))
    }

    fn push(&mut self, tokens: &mut Vec<LexToken>, kind: SyntaxKind, start: u32, end: u32) {
        debug_assert!(start <= end);
        if start == end {
            return; // Never emit an empty token — it cannot move `pos` forward.
        }
        tokens.push(LexToken { kind, range: TextRange::new(TextSize::from(start), TextSize::from(end)) });
        self.pos = end;
    }

    fn run(mut self) -> Vec<LexToken> {
        let mut tokens = Vec::new();
        while self.pos < self.len() {
            if self.in_tag {
                self.lex_tag_content(&mut tokens);
            } else {
                self.lex_data(&mut tokens);
            }
        }
        tokens
    }

    /// Outside any tag: text content, or the start of a tag/comment/CDATA/PI.
    fn lex_data(&mut self, tokens: &mut Vec<LexToken>) {
        let start = self.pos;
        if self.at(start) != Some(b'<') {
            let mut end = start;
            while end < self.len() && self.at(end) != Some(b'<') {
                end += 1;
            }
            self.push(tokens, SyntaxKind::TEXT, start, end);
            return;
        }

        if self.starts_with(start, "<!--") {
            let end = self.find_or_eof(start + 4, "-->");
            self.push(tokens, SyntaxKind::COMMENT, start, end);
        } else if self.starts_with(start, "<![CDATA[") {
            let end = self.find_or_eof(start + 9, "]]>");
            self.push(tokens, SyntaxKind::CDATA, start, end);
        } else if self.starts_with(start, "<?") {
            self.push(tokens, SyntaxKind::L_ANGLE_QUESTION, start, start + 2);
            self.in_tag = true;
            self.pi_mode = true;
        } else if self.starts_with(start, "</") {
            self.push(tokens, SyntaxKind::L_ANGLE_SLASH, start, start + 2);
            self.in_tag = true;
            self.pi_mode = false;
        } else {
            self.push(tokens, SyntaxKind::L_ANGLE, start, start + 1);
            self.in_tag = true;
            self.pi_mode = false;
        }
    }

    /// Scans from `from` for `needle`; returns the offset right after it, or
    /// end-of-input when `needle` never appears (an unterminated comment /
    /// CDATA section — still one token, covering the rest of the file, so
    /// the tree stays total).
    fn find_or_eof(&self, from: u32, needle: &str) -> u32 {
        match self.text.get(from as usize..).and_then(|rest| rest.find(needle)) {
            Some(idx) => from + idx as u32 + needle.len() as u32,
            None => self.len(),
        }
    }

    /// Inside `< … >` / `</ … >` / `<? … ?>`: whitespace, `Name`/`Name="value"`
    /// pairs, and the closing delimiter.
    fn lex_tag_content(&mut self, tokens: &mut Vec<LexToken>) {
        let start = self.pos;
        let Some(c) = self.at(start) else {
            // Unreachable in practice — `run()`'s loop only calls this while
            // `pos < len()` — but advancing `pos` to end-of-input rather than
            // panicking keeps the "no `unwrap`/`expect` outside tests" rule
            // (`CLAUDE.md` §7) true even for a case that cannot happen today.
            self.pos = self.len();
            return;
        };

        if c.is_ascii_whitespace() {
            let mut end = start;
            while self.at(end).is_some_and(|b| b.is_ascii_whitespace()) {
                end += 1;
            }
            self.push(tokens, SyntaxKind::WHITESPACE, start, end);
            return;
        }

        if self.pi_mode && self.starts_with(start, "?>") {
            self.push(tokens, SyntaxKind::QUESTION_R_ANGLE, start, start + 2);
            self.in_tag = false;
            self.pi_mode = false;
            return;
        }
        if !self.pi_mode && self.starts_with(start, "/>") {
            self.push(tokens, SyntaxKind::SLASH_R_ANGLE, start, start + 2);
            self.in_tag = false;
            return;
        }
        if !self.pi_mode && c == b'>' {
            self.push(tokens, SyntaxKind::R_ANGLE, start, start + 1);
            self.in_tag = false;
            return;
        }
        // A stray '>' while `pi_mode` (a malformed `<?xml ... >` missing its
        // `?`) still ends the tag — better recovery than swallowing the rest
        // of the file looking for a `?>` that was never coming.
        if self.pi_mode && c == b'>' {
            self.push(tokens, SyntaxKind::R_ANGLE, start, start + 1);
            self.in_tag = false;
            self.pi_mode = false;
            return;
        }

        if c == b'=' {
            self.push(tokens, SyntaxKind::EQ, start, start + 1);
            return;
        }

        if c == b'"' || c == b'\'' {
            let quote = c;
            let mut end = start + 1;
            while let Some(b) = self.at(end) {
                end += 1;
                if b == quote {
                    break;
                }
            }
            // `end` now covers either the closing quote (found) or the rest
            // of the file (unterminated — the parser flags this by checking
            // whether the token's last byte matches its first).
            self.push(tokens, SyntaxKind::STRING, start, end);
            return;
        }

        if is_ident_start(c) {
            let mut end = start;
            while self.at(end).is_some_and(is_ident_continue) {
                end += 1;
            }
            self.push(tokens, SyntaxKind::IDENT, start, end);
            return;
        }

        // Anything else inside a tag (stray punctuation, or a non-ASCII byte
        // that is not part of a well-formed name) — one error token, then
        // keep scanning so a single bad byte does not blank the file. Must
        // consume a whole UTF-8 character, not a fixed one byte: `start` is
        // always a char boundary here (every other branch above only ever
        // stops scanning at one), and `text[start..]` starting on a
        // multi-byte character (accented text inside a malformed tag is
        // exactly what `syntax_survives_utf8_garbage_without_panicking`
        // exercises) must not be cut mid-character, or slicing later panics.
        let char_len = self.text[start as usize..].chars().next().map_or(1, char::len_utf8) as u32;
        self.push(tokens, SyntaxKind::ERROR_TOKEN, start, start + char_len);
    }
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

/// Letters, digits, `_`, `-`, `.` and `:` — enough for `Panel`, `x:Name`,
/// `data-foo`, `Header.Icon`-style names without pulling in the full XML
/// `Name` production.
fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<SyntaxKind> {
        lex(text).into_iter().map(|t| t.kind).collect()
    }

    /// The lexer's own contribution to the round-trip guarantee: every byte
    /// of `text` must be covered by the concatenation of the token ranges.
    fn assert_total_coverage(text: &str) {
        let tokens = lex(text);
        let mut pos = 0u32;
        for t in &tokens {
            assert_eq!(u32::from(t.range.start()), pos, "gap before {t:?} in {text:?}");
            pos = u32::from(t.range.end());
        }
        assert_eq!(pos, text.len() as u32, "trailing gap in {text:?}");
    }

    #[test]
    fn simple_self_closing_tag() {
        assert_total_coverage(r#"<Button Text="Ok"/>"#);
        assert_eq!(
            kinds(r#"<Button Text="Ok"/>"#),
            vec![
                SyntaxKind::L_ANGLE,
                SyntaxKind::IDENT,
                SyntaxKind::WHITESPACE,
                SyntaxKind::IDENT,
                SyntaxKind::EQ,
                SyntaxKind::STRING,
                SyntaxKind::SLASH_R_ANGLE,
            ]
        );
    }

    #[test]
    fn element_with_text_child() {
        assert_total_coverage("<Label>hi</Label>");
        assert_eq!(
            kinds("<Label>hi</Label>"),
            vec![
                SyntaxKind::L_ANGLE,
                SyntaxKind::IDENT,
                SyntaxKind::R_ANGLE,
                SyntaxKind::TEXT,
                SyntaxKind::L_ANGLE_SLASH,
                SyntaxKind::IDENT,
                SyntaxKind::R_ANGLE,
            ]
        );
    }

    #[test]
    fn comment_and_prolog() {
        let src = r#"<?xml version="1.0"?><!-- hi --><Root/>"#;
        assert_total_coverage(src);
        assert_eq!(
            kinds(src),
            vec![
                SyntaxKind::L_ANGLE_QUESTION,
                SyntaxKind::IDENT,
                SyntaxKind::WHITESPACE,
                SyntaxKind::IDENT,
                SyntaxKind::EQ,
                SyntaxKind::STRING,
                SyntaxKind::QUESTION_R_ANGLE,
                SyntaxKind::COMMENT,
                SyntaxKind::L_ANGLE,
                SyntaxKind::IDENT,
                SyntaxKind::SLASH_R_ANGLE,
            ]
        );
    }

    #[test]
    fn namespaced_name_is_one_ident() {
        assert_eq!(kinds(r#"<View x:Name="a"/>"#)[3], SyntaxKind::IDENT);
        let toks = lex(r#"<View x:Name="a"/>"#);
        // token[3] is the `x:Name` ident.
        let t = &toks[3];
        assert_eq!(&r#"<View x:Name="a"/>"#[t.range], "x:Name");
    }

    #[test]
    fn unterminated_string_covers_to_eof() {
        let src = r#"<Button Text="Ok"#;
        assert_total_coverage(src);
        let toks = lex(src);
        let last = toks.last().unwrap();
        assert_eq!(last.kind, SyntaxKind::STRING);
        assert_eq!(u32::from(last.range.end()), src.len() as u32);
    }

    #[test]
    fn unterminated_comment_covers_to_eof() {
        let src = "<!-- never closed";
        assert_total_coverage(src);
        assert_eq!(kinds(src), vec![SyntaxKind::COMMENT]);
    }

    #[test]
    fn garbage_bytes_become_error_tokens_not_a_panic() {
        let src = "<Button ###/>";
        assert_total_coverage(src);
        let ks = kinds(src);
        assert!(ks.contains(&SyntaxKind::ERROR_TOKEN));
    }

    #[test]
    fn empty_input_is_fine() {
        assert_total_coverage("");
        assert!(lex("").is_empty());
    }

    #[test]
    fn error_token_does_not_split_a_multibyte_character() {
        // A non-ASCII byte sequence in a position no branch recognises
        // (inside a tag, not part of an identifier) used to fall through to
        // a fixed one-*byte* `ERROR_TOKEN`, splitting 'è' (2 UTF-8 bytes)
        // and panicking on the next `str` slice.
        let src = "<A è=\"x\"/>";
        assert_total_coverage(src);
        let toks = lex(src);
        for t in &toks {
            assert!(src.is_char_boundary(u32::from(t.range.start()) as usize));
            assert!(src.is_char_boundary(u32::from(t.range.end()) as usize));
        }
    }

    #[test]
    fn cdata_section() {
        let src = "<Script><![CDATA[a < b]]></Script>";
        assert_total_coverage(src);
        assert!(kinds(src).contains(&SyntaxKind::CDATA));
    }
}
