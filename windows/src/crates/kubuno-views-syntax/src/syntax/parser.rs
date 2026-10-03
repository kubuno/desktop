//! Recursive-descent, error-tolerant parser: token stream → lossless green
//! tree. Never fails outright — see the module doc on `lexer` for why total
//! coverage of the input is what makes that possible.
//!
//! Recovery strategy: on an unexpected token, the current construct is closed
//! early, the offending token is wrapped in an [`SyntaxKind::ERROR_NODE`] (so
//! it still ends up in the tree — losslessness holds even for garbage), a
//! [`Diagnostic`] is recorded, and parsing resumes at the next token. This is
//! deliberately simple (no bracket-matching recovery, no skip-to-sync-token
//! heuristics) — the grammar is small enough that "wrap one token and keep
//! going" does not spiral into a diagnostic storm for the malformed inputs
//! phase 2a's tests exercise (unterminated strings/tags, mismatched end tags,
//! stray bytes).

use super::lexer::LexToken;
use super::{Diagnostic, GreenNode, LineIndex, SyntaxKind};
use rowan::{GreenNodeBuilder, TextRange, TextSize};

pub fn parse(text: &str, tokens: Vec<LexToken>) -> (GreenNode, Vec<Diagnostic>) {
    let mut p = Parser {
        text,
        tokens,
        pos: 0,
        builder: GreenNodeBuilder::new(),
        errors: Vec::new(),
        line_index: LineIndex::new(text),
    };
    p.parse_document();
    let green = p.builder.finish();
    (green, p.errors)
}

struct Parser<'a> {
    text: &'a str,
    tokens: Vec<LexToken>,
    /// Index into `tokens` of the next token to consume (trivia included).
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    errors: Vec<Diagnostic>,
    line_index: LineIndex,
}

impl<'a> Parser<'a> {
    // ── low-level token access ──────────────────────────────────────────

    fn nth_raw(&self, n: usize) -> Option<LexToken> {
        self.tokens.get(self.pos + n).copied()
    }

    /// Tied to `'a` (the source text's own lifetime), not to `&self`'s
    /// borrow — otherwise every `self.builder.token(_, self.token_text(t))`
    /// call below would try to hold an immutable borrow of `self` across a
    /// mutable one (`self.builder`) in the same expression.
    fn token_text(&self, t: LexToken) -> &'a str {
        &self.text[t.range]
    }

    /// The next non-trivia token's kind, without consuming anything —
    /// trivia gets bumped into the tree lazily, right before whatever node
    /// is open when the grammar next needs to look past it. This is what
    /// keeps trivia attached to its true parent (see the module doc).
    fn peek(&mut self) -> Option<SyntaxKind> {
        self.skip_trivia();
        self.nth_raw(0).map(|t| t.kind)
    }

    /// Whether the next non-trivia token is a whitespace-only [`SyntaxKind::TEXT`]
    /// run — an ordinary blank line/indent outside the root element (before
    /// the prolog, or trailing after `</Root>`), as opposed to stray real
    /// content, which stays an error.
    fn at_blank_text(&mut self) -> bool {
        self.skip_trivia();
        self.nth_raw(0).is_some_and(|t| t.kind == SyntaxKind::TEXT && self.token_text(t).chars().all(char::is_whitespace))
    }

    fn skip_trivia(&mut self) {
        while let Some(t) = self.nth_raw(0) {
            if t.kind.is_trivia() {
                self.bump_raw(t);
            } else {
                break;
            }
        }
    }

    /// Pushes the current raw token into the tree and advances, whatever its
    /// kind — the only place tokens leave `self.tokens` and enter the tree.
    fn bump_raw(&mut self, t: LexToken) {
        self.builder.token(t.kind.into(), self.token_text(t));
        self.pos += 1;
    }

    /// Consumes and returns the next non-trivia token (trivia before it is
    /// bumped first, so it lands in the currently open node).
    fn bump(&mut self) -> Option<LexToken> {
        self.skip_trivia();
        let t = self.nth_raw(0)?;
        self.bump_raw(t);
        Some(t)
    }

    /// Consumes and returns the next non-trivia token only when it has
    /// `kind` — the peek-then-bump combinator every "expected token X here"
    /// grammar rule uses, so a mismatch is `None` rather than a `.expect()`
    /// panic on what "should" always match (`CLAUDE.md` §7).
    fn eat(&mut self, kind: SyntaxKind) -> Option<LexToken> {
        if self.peek() == Some(kind) {
            self.bump()
        } else {
            None
        }
    }

    fn error_at(&mut self, range: TextRange, message: impl Into<String>) {
        let lc = self.line_index.line_col(range.start());
        self.errors.push(Diagnostic { range, line: lc.line, column: lc.column, message: message.into() });
    }

    /// End-of-input's "range": an empty range at the end of the text, so a
    /// diagnostic about a missing closer still has *some* location.
    fn eof_range(&self) -> TextRange {
        let end = TextSize::from(self.text.len() as u32);
        TextRange::new(end, end)
    }

    // ── grammar ──────────────────────────────────────────────────────────

    fn parse_document(&mut self) {
        self.builder.start_node(SyntaxKind::DOCUMENT.into());

        if self.peek() == Some(SyntaxKind::L_ANGLE_QUESTION) {
            self.parse_prolog();
        }

        // Leading comments/blank lines before the root element are ordinary;
        // real stray content is not and falls through to the error below.
        while matches!(self.peek(), Some(SyntaxKind::COMMENT)) || self.at_blank_text() {
            self.bump();
        }

        match self.peek() {
            Some(SyntaxKind::L_ANGLE) => self.parse_element(),
            Some(_) => self.recover_unexpected("expected the root element"),
            None => self.error_at(self.eof_range(), "empty document: expected a root element"),
        }

        // Trailing whitespace/comments (a file's final newline, most often)
        // are completely ordinary — only genuine leftover content is an
        // error. Keeps the tree total either way: whatever is left, drained
        // one token at a time.
        while matches!(self.peek(), Some(SyntaxKind::COMMENT)) || self.at_blank_text() {
            self.bump();
        }
        while self.peek().is_some() {
            self.recover_unexpected("unexpected content after the root element");
        }

        self.builder.finish_node(); // DOCUMENT
    }

    fn parse_prolog(&mut self) {
        self.builder.start_node(SyntaxKind::PROLOG.into());
        self.bump(); // `<?`
        // `xml` target name, if present.
        if self.peek() == Some(SyntaxKind::IDENT) {
            self.bump();
        }
        self.parse_attributes_until(SyntaxKind::QUESTION_R_ANGLE);
        match self.peek() {
            Some(SyntaxKind::QUESTION_R_ANGLE) => {
                self.bump();
            }
            _ => {
                let range = self.next_range_or_eof();
                self.error_at(range, "unterminated processing instruction, expected `?>`");
            }
        }
        self.builder.finish_node(); // PROLOG
    }

    /// `<Name attr="v" …>` children `</Name>`, or `<Name attr="v" …/>`.
    fn parse_element(&mut self) {
        self.builder.start_node(SyntaxKind::ELEMENT.into());

        let self_closing = self.parse_start_tag();

        if !self_closing {
            self.parse_content();
            self.parse_end_tag();
        }

        self.builder.finish_node(); // ELEMENT
    }

    /// Returns whether the tag self-closed (`/>`), in which case the caller
    /// must not look for children or an end tag.
    fn parse_start_tag(&mut self) -> bool {
        self.builder.start_node(SyntaxKind::START_TAG.into());
        self.bump(); // `<`

        let name_range = match self.eat(SyntaxKind::IDENT) {
            Some(t) => Some(t.range),
            None => {
                let range = self.next_range_or_eof();
                self.error_at(range, "expected an element name after `<`");
                None
            }
        };

        self.parse_attributes_until_tag_close();

        let self_closing = match self.peek() {
            Some(SyntaxKind::SLASH_R_ANGLE) => {
                self.bump();
                true
            }
            Some(SyntaxKind::R_ANGLE) => {
                self.bump();
                false
            }
            _ => {
                let range = name_range.unwrap_or_else(|| self.next_range_or_eof());
                self.error_at(range, "unterminated tag, expected `>` or `/>`");
                // Recovery: nothing sane left to consume as part of the tag —
                // treat it as self-closing so the caller does not go hunting
                // for an end tag that will never come.
                true
            }
        };

        self.builder.finish_node(); // START_TAG
        self_closing
    }

    fn parse_end_tag(&mut self) {
        if self.peek() != Some(SyntaxKind::L_ANGLE_SLASH) {
            let range = self.next_range_or_eof();
            self.error_at(range, "expected a closing tag `</…>`");
            return;
        }
        self.builder.start_node(SyntaxKind::END_TAG.into());
        self.bump(); // `</`
        if self.peek() == Some(SyntaxKind::IDENT) {
            self.bump();
        } else {
            let range = self.next_range_or_eof();
            self.error_at(range, "expected an element name after `</`");
        }
        match self.peek() {
            Some(SyntaxKind::R_ANGLE) => {
                self.bump();
            }
            _ => {
                let range = self.next_range_or_eof();
                self.error_at(range, "unterminated closing tag, expected `>`");
            }
        }
        self.builder.finish_node(); // END_TAG
    }

    /// `name="value"` pairs, stopping at a start/prolog tag closer.
    fn parse_attributes_until_tag_close(&mut self) {
        self.parse_attributes_until_any(&[SyntaxKind::R_ANGLE, SyntaxKind::SLASH_R_ANGLE])
    }

    fn parse_attributes_until(&mut self, stop: SyntaxKind) {
        self.parse_attributes_until_any(&[stop])
    }

    fn parse_attributes_until_any(&mut self, stops: &[SyntaxKind]) {
        loop {
            match self.peek() {
                Some(SyntaxKind::IDENT) => self.parse_attribute(),
                Some(k) if stops.contains(&k) => break,
                Some(_) => {
                    self.recover_unexpected("expected an attribute or the tag's closing `>`");
                }
                None => break, // EOF — the caller reports the missing closer.
            }
        }
    }

    fn parse_attribute(&mut self) {
        self.builder.start_node(SyntaxKind::ATTRIBUTE.into());
        self.bump(); // IDENT (name)

        if self.peek() == Some(SyntaxKind::EQ) {
            self.bump();
            match self.eat(SyntaxKind::STRING) {
                Some(t) => self.check_string_terminated(t),
                None => {
                    let range = self.next_range_or_eof();
                    self.error_at(range, "expected a quoted attribute value after `=`");
                }
            }
        } else {
            let range = self.next_range_or_eof();
            self.error_at(range, "expected `=` after attribute name");
        }
        self.builder.finish_node(); // ATTRIBUTE
    }

    /// A [`SyntaxKind::STRING`] token that does not end with the quote it
    /// opened with ran off the end of the file — the lexer still emits it as
    /// one token (total coverage), the parser is what flags it.
    fn check_string_terminated(&mut self, t: LexToken) {
        let s = self.token_text(t);
        let well_formed = match s.chars().next() {
            Some(quote) if quote == '"' || quote == '\'' => s.len() >= 2 && s.ends_with(quote),
            _ => false,
        };
        if !well_formed {
            self.error_at(t.range, "unterminated string literal");
        }
    }

    /// Element/text/comment/CDATA children, until `</` (the parent's end
    /// tag) or end of input.
    fn parse_content(&mut self) {
        loop {
            match self.peek() {
                Some(SyntaxKind::L_ANGLE) => self.parse_element(),
                Some(SyntaxKind::TEXT) | Some(SyntaxKind::CDATA) | Some(SyntaxKind::COMMENT) => {
                    self.bump();
                }
                Some(SyntaxKind::L_ANGLE_SLASH) | None => break,
                Some(_) => self.recover_unexpected("unexpected content"),
            }
        }
    }

    /// The range diagnostics should point at when the parser cannot make
    /// sense of what comes next: the next token if any, else an empty range
    /// at end-of-file.
    fn next_range_or_eof(&mut self) -> TextRange {
        self.skip_trivia();
        self.nth_raw(0).map(|t| t.range).unwrap_or_else(|| self.eof_range())
    }

    /// Wraps exactly one unexpected token in an [`SyntaxKind::ERROR_NODE`]
    /// and records a diagnostic — the shared "don't get stuck" primitive
    /// every recovery path above falls back on.
    fn recover_unexpected(&mut self, message: &str) {
        self.skip_trivia();
        match self.nth_raw(0) {
            Some(t) => {
                self.error_at(t.range, message);
                self.builder.start_node(SyntaxKind::ERROR_NODE.into());
                self.bump_raw(t);
                self.builder.finish_node();
            }
            None => self.error_at(self.eof_range(), message),
        }
    }
}
