//! The handful of attributes every element accepts regardless of its
//! registry entry — `x:Name`, `Dock`, `Anchor`, `X`, `Y`, `Width`, `Height`
//! (`XML_VIEWS.md` §1: not a per-component concept, read by the *parent*
//! layout engine the same way WinForms reads `Control.Dock`/`Control.Anchor`
//! off any control).
//!
//! ## Missing `kubuno-views` API this works around
//!
//! `kubuno_views::validate` has its own `COMMON_ATTRIBUTES` table (`validate.
//! rs`) with exactly this data, but it is a private `const`, not part of the
//! crate's public API — reasonably so, since phase 2a only needed it inside
//! the validator itself. Completion (offering `Dock`/`Anchor`/… alongside a
//! component's own registered properties) and hover (a doc string when the
//! cursor is on one of them) both need the same table from outside the
//! crate, so it is duplicated here rather than asking the crate — currently
//! being edited by another agent for the interpreter — to export it mid-edit.
//! If `kubuno_views::validate` (or `registry`) later exposes this as `pub`,
//! this module should be deleted and its one call site
//! (`completion.rs`/`hover.rs`) switched to the real export; the risk in the
//! meantime is exactly one list to keep in sync by hand, the same risk
//! `registry/mod.rs`'s own module doc already accepts for the registry vs.
//! `kubuno-ui` itself (mitigated there by a compiled `smoke` test; there is
//! no equivalent mechanical check available for a private `const`, so this
//! module's own test below just pins the list against `kubuno-views`'
//! observable behaviour: every name here must be accepted by the validator
//! on an otherwise-unknown-attribute-free element).
use kubuno_views::registry::PropKind;

pub struct CommonAttr {
    pub name: &'static str,
    pub kind: PropKind,
    pub doc: &'static str,
}

pub const COMMON_ATTRIBUTES: &[CommonAttr] = &[
    CommonAttr {
        name: "Dock",
        kind: PropKind::Enum(&["None", "Top", "Bottom", "Left", "Right", "Fill"]),
        doc: "Edge of the parent panel the element is docked to, or Fill to take the remaining space.",
    },
    CommonAttr {
        name: "Anchor",
        kind: PropKind::String,
        doc: "Edges of the parent panel the element stays attached to when it is resized, for example Top, Left.",
    },
    CommonAttr { name: "X", kind: PropKind::F32, doc: "Distance from the left edge of the parent panel, in pixels." },
    CommonAttr { name: "Y", kind: PropKind::F32, doc: "Distance from the top edge of the parent panel, in pixels." },
    CommonAttr { name: "Width", kind: PropKind::F32, doc: "Width of the element, in pixels." },
    CommonAttr { name: "Height", kind: PropKind::F32, doc: "Height of the element, in pixels." },
    // Design-time only (`kubuno_views::registry::DESIGN_TIME_ATTRIBUTES`): valid on the root element.
    CommonAttr {
        name: "DesignWidth",
        kind: PropKind::F32,
        doc: "Width of the view in the designer, in pixels (design time only, ignored when the app runs).",
    },
    CommonAttr {
        name: "DesignHeight",
        kind: PropKind::F32,
        doc: "Height of the view in the designer, in pixels (design time only, ignored when the app runs).",
    },
];

/// `x:Name="…"` itself: free per `XML_VIEWS.md` §1 ("not a synthesized field
/// name … it *is* a `FocusId`"), so it is not in [`COMMON_ATTRIBUTES`]
/// (which models typed, checked properties) but is still worth a hover/
/// completion doc string of its own.
// Implementation note: x:Name feeds `FocusId::of(...)` and the event/handler tables directly (not a
// synthesized field name; there is no retained widget to name).
pub const X_NAME_DOC: &str = "Name of the element, used to refer to it from the code (for example in handler names).";

pub fn lookup(name: &str) -> Option<&'static CommonAttr> {
    COMMON_ATTRIBUTES.iter().find(|a| a.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubuno_views::syntax::parse;
    use kubuno_views::validate::validate_with_default_registry;

    /// Pins this hand-copied table against the crate it was copied from: any
    /// name listed here must round-trip through the real validator without
    /// an "unknown attribute" diagnostic on a real, minimal element. Catches
    /// the table drifting (a name renamed/removed upstream) the moment
    /// `kubuno-views` changes, same spirit as `registry/mod.rs`'s own
    /// `smoke`-test mitigation for its metadata table.
    #[test]
    fn every_common_attribute_is_accepted_by_the_real_validator() {
        for attr in COMMON_ATTRIBUTES {
            let value = match attr.kind {
                PropKind::Bool => "true",
                PropKind::F32 => "1",
                PropKind::String => "Top,Left",
                PropKind::Enum(variants) => variants[0],
            };
            let src = format!(r#"<Button {}="{value}"/>"#, attr.name);
            let p = parse(&src);
            assert!(p.diagnostics.is_empty(), "{src}: {:?}", p.diagnostics);
            let diags = validate_with_default_registry(&p);
            assert!(diags.is_empty(), "{src}: {diags:?}");
        }
    }

    #[test]
    fn x_name_is_accepted_too() {
        let p = parse(r#"<Button x:Name="go"/>"#);
        assert!(validate_with_default_registry(&p).is_empty());
    }

    #[test]
    fn lookup_finds_dock_and_rejects_unknown() {
        assert!(lookup("Dock").is_some());
        assert!(lookup("NotAThing").is_none());
    }
}
