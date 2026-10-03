//! The Properties window editor a property asks for (`PropertyMeta::editor`, the `editor` field of
//! `kbview-registry.json`, `vskubuno/docs/VIEWS-SPEC.md` §10.3), parsed from its wire string.
//!
//! The registry keeps the string itself (it is what the export, the Visual Studio designer and the
//! web registry exchange); [`EditorKind::parse`] gives tools a closed type to match on.

/// One editor of the Properties window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditorKind<'a> {
    /// A name of the Kubuno icon set, an image file or a resource (`"icon"`).
    Icon,
    /// An image file or a resource (`"image"`).
    Image,
    /// A theme colour token or a free colour (`"color"`).
    Color,
    /// A font (`"font"`).
    Font,
    /// A mouse cursor (`"cursor"`).
    Cursor,
    /// Several lines of text, one value per line (`"lines"`).
    Lines,
    /// A list, set with a binding only (`"list"`).
    List,
    /// An object, set with a binding only (`"object"`).
    Object,
    /// The `x:Name` of another element of the view whose class is (or derives from) the given one
    /// (`"reference:ContextMenu"`).
    Reference(&'a str),
    /// The name of a class deriving from the given one (`"class:UserControl"`).
    Class(&'a str),
    /// An editor this version does not know: tools fall back to a text box.
    Other(&'a str),
}

impl<'a> EditorKind<'a> {
    /// The editor written `editor` (never fails: an unknown string is [`EditorKind::Other`]).
    pub fn parse(editor: &'a str) -> Self {
        if let Some(class) = editor.strip_prefix("reference:") {
            return EditorKind::Reference(class);
        }
        if let Some(class) = editor.strip_prefix("class:") {
            return EditorKind::Class(class);
        }
        match editor {
            "icon" => EditorKind::Icon,
            "image" => EditorKind::Image,
            "color" => EditorKind::Color,
            "font" => EditorKind::Font,
            "cursor" => EditorKind::Cursor,
            "lines" => EditorKind::Lines,
            "list" => EditorKind::List,
            "object" => EditorKind::Object,
            other => EditorKind::Other(other),
        }
    }

    /// Whether a property with this editor is set with a binding only (a list or an object).
    pub fn is_bound_only(self) -> bool {
        matches!(self, EditorKind::List | EditorKind::Object)
    }
}

#[cfg(test)]
mod tests {
    use super::EditorKind;

    #[test]
    fn parses_every_editor_of_the_spec() {
        assert_eq!(EditorKind::parse("icon"), EditorKind::Icon);
        assert_eq!(EditorKind::parse("image"), EditorKind::Image);
        assert_eq!(EditorKind::parse("color"), EditorKind::Color);
        assert_eq!(EditorKind::parse("font"), EditorKind::Font);
        assert_eq!(EditorKind::parse("cursor"), EditorKind::Cursor);
        assert_eq!(EditorKind::parse("lines"), EditorKind::Lines);
        assert_eq!(EditorKind::parse("list"), EditorKind::List);
        assert_eq!(EditorKind::parse("object"), EditorKind::Object);
        assert_eq!(EditorKind::parse("reference:ContextMenu"), EditorKind::Reference("ContextMenu"));
        assert_eq!(EditorKind::parse("class:UserControl"), EditorKind::Class("UserControl"));
        assert_eq!(EditorKind::parse("spline"), EditorKind::Other("spline"));
        assert!(EditorKind::parse("list").is_bound_only());
        assert!(!EditorKind::parse("color").is_bound_only());
    }
}
