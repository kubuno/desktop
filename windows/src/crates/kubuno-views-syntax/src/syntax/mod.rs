//! The lossless syntax tree: lexer, parser, and the `rowan` plumbing that
//! ties them to a typed red/green tree.
//!
//! See `vskubuno/docs/XML_VIEWS.md` §6 for why `rowan` was chosen over
//! `quick-xml`/`roxmltree`/`xot`.

mod kind;
mod lexer;
mod line_index;
mod parser;

pub use kind::SyntaxKind;
pub use line_index::{LineCol, LineIndex};

/// The `rowan::Language` glue: how a `.kbview` file's [`SyntaxKind`] maps
/// onto rowan's internal raw `u16` kind. `rowan` is generic over this so the
/// same crate can host several grammars; we only ever have one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KubunoViewLanguage {}

impl rowan::Language for KubunoViewLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        assert!(raw.0 <= SyntaxKind::__LAST as u16);
        // SAFETY-free: `SyntaxKind` is `#[repr(u16)]` and every value up to
        // `__LAST` is a declared variant, so this transmute-by-cast is total.
        // (No `unsafe`: done through a match table instead — see
        // `kind::from_u16`.)
        kind::from_u16(raw.0)
    }

    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        kind.into()
    }
}

/// A position/range diagnostic — a parse error or a validator finding.
/// Carries a 1-based line/column (per the phase 2a brief) alongside the raw
/// byte [`rowan::TextRange`], so a caller can either show it to a human or
/// hand it to an LSP `Diagnostic` (which wants zero-based UTF-16 columns —
/// a §5-mentioned future consumer, not built here).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub range: rowan::TextRange,
    /// 1-based start line.
    pub line: u32,
    /// 1-based start column (UTF-8 byte offset within the line + 1; see
    /// [`LineIndex`] for the exact rule).
    pub column: u32,
    pub message: String,
}

pub type SyntaxNode = rowan::SyntaxNode<KubunoViewLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<KubunoViewLanguage>;
pub type SyntaxElement = rowan::NodeOrToken<SyntaxNode, SyntaxToken>;
pub type GreenNode = rowan::GreenNode;

/// The result of parsing one `.kbview` file: a lossless tree plus whatever
/// diagnostics the error-tolerant parser collected along the way. Parsing
/// never fails outright — even garbage input produces a tree (full of
/// [`SyntaxKind::ERROR_NODE`]s) and a non-empty `diagnostics`.
#[derive(Debug, Clone)]
pub struct Parse {
    green: GreenNode,
    pub diagnostics: Vec<Diagnostic>,
    line_index: LineIndex,
}

impl Parse {
    /// The root [`SyntaxNode`] (kind [`SyntaxKind::DOCUMENT`]).
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    pub fn green(&self) -> &GreenNode {
        &self.green
    }

    /// The 1-based line/column a byte offset falls on.
    pub fn line_col(&self, offset: rowan::TextSize) -> LineCol {
        self.line_index.line_col(offset)
    }

    /// Round-trips: reconstructs the exact original text from the tree.
    /// `parse(text).text() == text` for every input, malformed or not — this
    /// is the property phase 2a's round-trip tests pin down.
    pub fn text(&self) -> String {
        self.syntax().text().to_string()
    }
}

/// Parses `text` into a lossless [`Parse`]. Never panics on malformed input:
/// unexpected tokens are wrapped in [`SyntaxKind::ERROR_NODE`]s and recorded
/// as diagnostics, and every byte of `text` still ends up in the tree.
pub fn parse(text: &str) -> Parse {
    let tokens = lexer::lex(text);
    let (green, diagnostics) = parser::parse(text, tokens);
    Parse { green, diagnostics, line_index: LineIndex::new(text) }
}
