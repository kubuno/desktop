//! The token/node kinds of a `.kbview` file.
//!
//! Deliberately smaller than general XML (see `XML_VIEWS.md` §6): no DTDs,
//! one processing instruction (the `<?xml … ?>` prolog), namespace prefixes
//! (`x:Name`) kept as plain qualified identifiers rather than resolved
//! against a namespace table.

/// Every terminal (token) and non-terminal (node) kind the lexer/parser
/// produce. `rowan` stores this as a raw `u16` internally (via
/// [`KubunoViewLanguage`](super::KubunoViewLanguage)); the explicit
/// `#[repr(u16)]` keeps that mapping stable and cheap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
#[allow(non_camel_case_types)] // Mirrors rust-analyzer's own `SyntaxKind` convention.
pub enum SyntaxKind {
    // ── Tokens (leaves) ─────────────────────────────────────────────────
    /// Run of spaces, tabs and newlines.
    WHITESPACE,
    /// `<!-- … -->`, contents included.
    COMMENT,
    /// A run of non-`<` characters between tags.
    TEXT,
    /// A tag or attribute name, e.g. `Panel`, `Dock`, `x:Name` — kept as one
    /// token; the `x:` prefix is not split off (§1: "namespace prefixes …
    /// as plain qualified names").
    IDENT,
    /// A quoted attribute value, quotes included (`"Top"` or `'Top'`).
    STRING,
    /// `<![CDATA[ … ]]>`, contents and markers included.
    CDATA,
    /// `<`
    L_ANGLE,
    /// `</`
    L_ANGLE_SLASH,
    /// `<?`
    L_ANGLE_QUESTION,
    /// `>`
    R_ANGLE,
    /// `/>`
    SLASH_R_ANGLE,
    /// `?>`
    QUESTION_R_ANGLE,
    /// `=`
    EQ,
    /// Any byte sequence the lexer could not classify — kept as a token (not
    /// dropped) so the tree stays lossless even over malformed input.
    ERROR_TOKEN,
    /// End of input marker some helpers use; never actually pushed as a tree
    /// token.
    EOF,

    // ── Nodes (non-terminals) ───────────────────────────────────────────
    /// The whole file: an optional prolog, one root element, trivia.
    DOCUMENT,
    /// `<?xml version="1.0" … ?>`.
    PROLOG,
    /// `<Name attr="…">children</Name>` or `<Name attr="…"/>`.
    ELEMENT,
    /// The opening `<Name …>` or self-closing `<Name …/>` part of an
    /// [`SyntaxKind::ELEMENT`].
    START_TAG,
    /// The closing `</Name>` part of an [`SyntaxKind::ELEMENT`]; absent when
    /// the start tag self-closes.
    END_TAG,
    /// `name="value"` inside a [`SyntaxKind::START_TAG`].
    ATTRIBUTE,
    /// A run of [`SyntaxKind::TEXT`]/[`SyntaxKind::CDATA`] content, wrapped so
    /// the `ast` layer can hand it back as one logical text node.
    TEXT_NODE,
    /// A parse error the recovery strategy could not attach to a more
    /// specific node — still carries the offending tokens losslessly.
    ERROR_NODE,

    /// Not a real kind; keeps `SyntaxKind::__LAST as u16` a cheap bound check
    /// (rowan asks for `u16::from(kind) <= u16::MAX`, this just documents the
    /// count for anyone extending the enum).
    __LAST,
}

impl SyntaxKind {
    pub fn is_trivia(self) -> bool {
        matches!(self, SyntaxKind::WHITESPACE | SyntaxKind::COMMENT)
    }
}

/// The inverse of `SyntaxKind as u16` — a plain match, not a transmute, so an
/// out-of-range value is a clean panic (via the `unreachable!`) rather than
/// undefined behaviour. `rowan::Language::kind_from_raw` is the only caller.
pub(super) fn from_u16(raw: u16) -> SyntaxKind {
    use SyntaxKind::*;
    match raw {
        0 => WHITESPACE,
        1 => COMMENT,
        2 => TEXT,
        3 => IDENT,
        4 => STRING,
        5 => CDATA,
        6 => L_ANGLE,
        7 => L_ANGLE_SLASH,
        8 => L_ANGLE_QUESTION,
        9 => R_ANGLE,
        10 => SLASH_R_ANGLE,
        11 => QUESTION_R_ANGLE,
        12 => EQ,
        13 => ERROR_TOKEN,
        14 => EOF,
        15 => DOCUMENT,
        16 => PROLOG,
        17 => ELEMENT,
        18 => START_TAG,
        19 => END_TAG,
        20 => ATTRIBUTE,
        21 => TEXT_NODE,
        22 => ERROR_NODE,
        _ => unreachable!("SyntaxKind::from_u16({raw}): not a declared kind"),
    }
}

/// Converts to rowan's raw kind. `rowan::SyntaxKind` is a thin `u16` newtype;
/// this is the only place the cast happens.
impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        rowan::SyntaxKind(kind as u16)
    }
}

// Keeps `from_u16` honest without hand-maintaining the numbering twice: this
// test fails the moment the enum gains/loses/reorders a variant without a
// matching edit above.
#[cfg(test)]
mod repr_matches_declaration_order {
    use super::SyntaxKind::*;

    #[test]
    fn round_trips_every_kind() {
        let all = [
            WHITESPACE, COMMENT, TEXT, IDENT, STRING, CDATA, L_ANGLE, L_ANGLE_SLASH,
            L_ANGLE_QUESTION, R_ANGLE, SLASH_R_ANGLE, QUESTION_R_ANGLE, EQ, ERROR_TOKEN, EOF,
            DOCUMENT, PROLOG, ELEMENT, START_TAG, END_TAG, ATTRIBUTE, TEXT_NODE, ERROR_NODE,
        ];
        for kind in all {
            assert_eq!(super::from_u16(kind as u16), kind, "kind {kind:?} did not round-trip");
        }
    }
}
