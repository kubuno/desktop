//! The markup attributes of a view that are not properties of an element: `x:` (the view's own
//! directives — `x:Name`, `x:Inherits`…) and `d:` (design-time only — `d:Visible`, `d:ItemsSource`;
//! read by the designer, ignored when the app runs, `VIEWS-SPEC.md` §6.4). The `d:` attributes of the
//! root element that are typed (`DesignWidth`, `DesignHeight`) are listed by
//! `kubuno_desktop_views_model::DESIGN_TIME_ATTRIBUTES`.

/// The prefix of the view's directives (`x:Name`).
pub const DIRECTIVE_PREFIX: &str = "x:";

/// The prefix of the design-time attributes (`d:Visible`).
pub const DESIGN_PREFIX: &str = "d:";

/// The element name attribute.
pub const NAME_ATTRIBUTE: &str = "x:Name";

/// Whether `attribute` is a directive (`x:…`).
pub fn is_directive(attribute: &str) -> bool {
    attribute.starts_with(DIRECTIVE_PREFIX)
}

/// Whether `attribute` is a design-time attribute (`d:…`).
pub fn is_design_time(attribute: &str) -> bool {
    attribute.starts_with(DESIGN_PREFIX)
}

/// Whether `attribute` is markup rather than a property or an event of the element (`x:…`, `d:…`,
/// and the namespace declarations `xmlns`/`xmlns:…` of [`crate::namespaces`]): what the validator
/// never checks against the registry.
pub fn is_markup(attribute: &str) -> bool {
    is_directive(attribute) || is_design_time(attribute) || crate::namespaces::is_declaration(attribute)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_markup_attributes() {
        assert!(is_markup(NAME_ATTRIBUTE));
        assert!(is_markup("d:ItemsSource"));
        assert!(is_directive("x:Inherits"));
        assert!(is_design_time("d:Visible"));
        assert!(!is_markup("Text"));
        assert!(!is_markup("OnClick"));
        assert!(is_markup("xmlns") && is_markup("xmlns:x"));
    }
}
