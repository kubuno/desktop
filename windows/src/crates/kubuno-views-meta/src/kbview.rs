//! A small, dependency-free reader of `.kbview` files for the tools that cannot link `kubuno-views`
//! (the `#[kubuno::view]` proc macro, which runs in the compiler): the elements in document order,
//! with their attributes, line and depth. It is tolerant — comments, processing instructions and
//! `CDATA` sections are skipped, text content is ignored — and reports the first malformed tag it
//! meets with its line. The full parser, validator and editor stay `kubuno_views::syntax`.

/// One element of a view, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KbElement {
    /// The element name (`"Button"`).
    pub name: String,
    /// Its attributes in document order, values decoded (`&amp;` → `&`).
    pub attributes: Vec<(String, String)>,
    /// 1-based line of its `<`.
    pub line: usize,
    /// 0 for the root, 1 for its children…
    pub depth: usize,
}

impl KbElement {
    /// The value of attribute `name`, if written.
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// Its `x:Name`, if any.
    pub fn x_name(&self) -> Option<&str> {
        self.attribute("x:Name")
    }

    /// The event attributes it writes (`OnClick="save"`) and their handler names: an attribute
    /// named `On` followed by an upper-case letter (`On` alone is a `Switch` property).
    pub fn event_handlers(&self) -> impl Iterator<Item = (&str, &str)> {
        self.attributes.iter().filter(|(n, _)| is_event_attribute(n)).map(|(n, v)| (n.as_str(), v.as_str()))
    }
}

/// Whether an attribute name is an event (`OnClick`, `OnTextChanged`), not a property.
pub fn is_event_attribute(name: &str) -> bool {
    name.strip_prefix("On").and_then(|rest| rest.chars().next()).is_some_and(|c| c.is_ascii_uppercase())
}

/// A malformed view: what and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KbError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for KbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

/// Reads every element of `text`, in document order.
pub fn scan(text: &str) -> Result<Vec<KbElement>, KbError> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut i = 0;
    let line_at = |pos: usize| text[..pos.min(text.len())].bytes().filter(|b| *b == b'\n').count() + 1;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &text[i..];
        let skip_to = |end: &str, from: usize| -> Result<usize, KbError> {
            text[from..].find(end).map(|p| from + p + end.len()).ok_or_else(|| KbError { line: line_at(from), message: format!("`{end}` expected before the end of the file") })
        };
        if rest.starts_with("<!--") {
            i = skip_to("-->", i + 4)?;
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            i = skip_to("]]>", i + 9)?;
            continue;
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            i = skip_to(">", i + 2)?;
            continue;
        }
        if let Some(close) = rest.strip_prefix("</") {
            let end = close.find('>').ok_or_else(|| KbError { line: line_at(i), message: "unterminated closing tag".to_string() })?;
            let name = close[..end].trim();
            match stack.pop() {
                Some(open) if open == name => {}
                Some(open) => return Err(KbError { line: line_at(i), message: format!("`</{name}>` closes `<{open}>`") }),
                None => return Err(KbError { line: line_at(i), message: format!("`</{name}>` has no opening tag") }),
            }
            i += 2 + end + 1;
            continue;
        }
        // A start tag.
        let start = i;
        i += 1;
        let name_end = i + text[i..].find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(text.len() - i);
        let name = text[i..name_end].to_string();
        if name.is_empty() {
            return Err(KbError { line: line_at(start), message: "an element name is expected after `<`".to_string() });
        }
        i = name_end;
        let mut attributes = Vec::new();
        let self_closing;
        loop {
            while i < bytes.len() && (bytes[i] as char).is_whitespace() {
                i += 1;
            }
            if i >= bytes.len() {
                return Err(KbError { line: line_at(start), message: format!("`<{name}` is not closed") });
            }
            if text[i..].starts_with("/>") {
                self_closing = true;
                i += 2;
                break;
            }
            if bytes[i] == b'>' {
                self_closing = false;
                i += 1;
                break;
            }
            let attr_end = i + text[i..].find(|c: char| c.is_whitespace() || c == '=' || c == '>' || c == '/').unwrap_or(text.len() - i);
            let attr = text[i..attr_end].to_string();
            if attr.is_empty() {
                return Err(KbError { line: line_at(i), message: format!("unexpected character in `<{name}>`") });
            }
            i = attr_end;
            while i < bytes.len() && (bytes[i] as char).is_whitespace() {
                i += 1;
            }
            if i >= bytes.len() || bytes[i] != b'=' {
                return Err(KbError { line: line_at(i.min(bytes.len().saturating_sub(1))), message: format!("attribute `{attr}` has no value") });
            }
            i += 1;
            while i < bytes.len() && (bytes[i] as char).is_whitespace() {
                i += 1;
            }
            let quote = bytes.get(i).copied().filter(|q| *q == b'"' || *q == b'\'').ok_or_else(|| KbError { line: line_at(i), message: format!("the value of `{attr}` must be quoted") })?;
            let value_start = i + 1;
            let value_end = value_start
                + text[value_start..].find(quote as char).ok_or_else(|| KbError { line: line_at(i), message: format!("the value of `{attr}` is not closed") })?;
            attributes.push((attr, decode(&text[value_start..value_end])));
            i = value_end + 1;
        }
        out.push(KbElement { name: name.clone(), attributes, line: line_at(start), depth: stack.len() });
        if !self_closing {
            stack.push(name);
        }
    }
    if let Some(open) = stack.pop() {
        return Err(KbError { line: line_at(text.len()), message: format!("`<{open}>` is never closed") });
    }
    Ok(out)
}

/// Decodes the XML character references of an attribute value.
fn decode(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp..];
        let Some(semi) = after.find(';') else {
            out.push_str(after);
            return out;
        };
        let entity = &after[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => out.push(c),
            None => out.push_str(&after[..=semi]),
        }
        rest = &after[semi + 1..];
    }
    out.push_str(rest);
    out
}

/// Every element name a view may use without a custom control: the control classes, the
/// structural child elements and the non-visual components of `kubuno_views` (a test there keeps
/// this list equal to its registry).
pub const BUILTIN_ELEMENTS: &[&str] = &[
    "Accordion", "AccordionSection", "Badge", "Breadcrumb", "BreadcrumbItem", "Button", "Callout", "Card", "CheckBox", "CheckedListBox", "ColorField", "Column",
    "ComboBox", "ContextMenu", "DataTable", "DatePicker", "DockArea", "DockPanel", "Dropdown", "EmptyState", "FloatingWindow", "GradientField", "GroupBox", "Icon", "IconButton", "Item", "Label", "LinkLabel", "ListBox",
    "ListView", "MaskedField", "MenuItem", "MonthCalendar", "NumericField", "Option", "PaintBox", "Panel", "ProgressBar", "RadioButton", "ScrollArea",
    "SearchField", "Separator", "Slider", "Spinner", "Splitter", "Stack", "Step", "Stepper", "Switch", "TabItem", "Tabs", "TextArea", "TextField", "Timer",
    "ToolTip", "Toolbar", "ToolbarItem", "TreeView", "UserControl", "WorkspaceShell",
    "Avatar", "PictureBox", "Popover", "Repeater", "Sidebar", "SidebarItem", "SidebarSection", "SplashArtwork", "StatusBar", "StatusLabel", "TableLayoutPanel",
    // The ribbon family (vskubuno/docs/RIBBON.md).
    "Ribbon", "RibbonTab", "RibbonContextualTabGroup", "RibbonGroup", "RibbonControlGroup", "RibbonBox", "RibbonQuickAccessToolbar", "RibbonBackstage", "BackstageTab", "BackstageButton", "BackstageSeparator", "RibbonButton", "RibbonToggleButton", "RibbonRadioButton", "RibbonMenuButton", "RibbonSplitButton", "RibbonColorPicker", "RibbonMenuItem", "RibbonSplitMenuItem", "RibbonCheckBox", "RibbonComboBox", "RibbonTextBox", "RibbonNumericField", "RibbonGallery", "RibbonGalleryCategory", "RibbonGalleryItem", "RibbonLabel", "RibbonSeparator", "Command",
    // The menu family (vskubuno/docs/MENUS.md).
    "MenuBar", "MenuSeparator", "MenuHeader", "DropDownButton", "SplitButton",
];

/// The components and controls of the Kubuno libraries an application links rather than defines
/// (`kubuno-data`, i.e. `kubuno::data`, `kubuno-print`, i.e. `kubuno::printing`, and
/// `kubuno-app-storage-components`, i.e. `kubuno::storage`): known to `#[kubuno::view]` without a
/// `#[derive(Component)]` in the application. [`DATA_ELEMENTS`], [`PRINT_ELEMENTS`] and
/// [`STORAGE_ELEMENTS`] together (a test of this crate); a test of each library keeps its list equal to
/// what it registers. Their `x:Name`d elements are `kubuno::Control` fields, or the handles of
/// [`PRINT_TYPED`] and [`STORAGE_TYPED`].
pub const LIBRARY_ELEMENTS: &[&str] = &[
    "BindingNavigator", "BindingSource", "DbCommand", "DbConnection", "ErrorProvider", "TableAdapter", "PageSetupDialog", "PreviewPagesButton", "PrintDialog",
    "PrintDocument", "PrintPreviewControl", "PrintPreviewDialog", "RegistryKey", "SecretStore", "Settings", "FileStore", "KeyValueStore", "LocalDatabase",
];

/// The storage components `kubuno-app-storage-components` registers (vskubuno docs/STORAGE-COMPONENTS.md).
pub const STORAGE_ELEMENTS: &[&str] = &["FileStore", "KeyValueStore", "RegistryKey", "SecretStore", "Settings"];

/// The storage elements the `kubuno` crate has a typed handle for, in `kubuno::storage`
/// (`settings: kubuno::storage::Settings`).
pub const STORAGE_TYPED: &[&str] = &["FileStore", "KeyValueStore", "RegistryKey", "SecretStore", "Settings"];

/// The elements that exist on some platforms only, with those platforms (`windows`, `linux`, `macos`, `web`,
/// `android`, `ios`): the language server warns when a view of a project targeting another platform uses one
/// (vskubuno docs/STORAGE-COMPONENTS.md §5.5). Elements not listed exist everywhere.
pub const PLATFORM_ELEMENTS: &[(&str, &[&str])] = &[("RegistryKey", &["windows"])];

/// The components and controls `kubuno-data` registers.
pub const DATA_ELEMENTS: &[&str] = &["BindingNavigator", "BindingSource", "DbCommand", "DbConnection", "ErrorProvider", "LocalDatabase", "TableAdapter"];

/// The components and controls `kubuno-print` registers.
pub const PRINT_ELEMENTS: &[&str] = &["PageSetupDialog", "PreviewPagesButton", "PrintDialog", "PrintDocument", "PrintPreviewControl", "PrintPreviewDialog"];

/// The printing elements the `kubuno` crate has a typed handle for, in `kubuno::printing`
/// (`print_document1: kubuno::printing::PrintDocument`).
pub const PRINT_TYPED: &[&str] = &["PageSetupDialog", "PrintDialog", "PrintDocument", "PrintPreviewControl", "PrintPreviewDialog"];

/// The controls the `kubuno` crate has a typed handle for (`kubuno::Button`…): an `x:Name`d element
/// of one of these kinds becomes a field of that type; the others are `kubuno::Control`.
pub const TYPED_CONTROLS: &[&str] = &[
    "Accordion", "Badge", "Breadcrumb", "Button", "Callout", "Card", "CheckBox", "CheckedListBox", "ColorField", "ComboBox", "DataTable", "DatePicker", "Dropdown",
    "EmptyState", "GradientField", "GroupBox", "Icon", "IconButton", "Label", "LinkLabel", "ListBox", "ListView", "MaskedField", "MonthCalendar", "NumericField", "PaintBox", "Panel",
    "ProgressBar", "RadioButton", "ScrollArea", "SearchField", "Separator", "Slider", "Spinner", "Splitter", "Stack", "Stepper", "Switch", "Tabs", "TextArea",
    "TextField", "Toolbar", "TreeView", "DockArea", "WorkspaceShell",
    "Avatar", "PictureBox", "Popover", "Repeater", "Sidebar", "StatusBar", "TableLayoutPanel",
    "Ribbon", "RibbonTab", "RibbonContextualTabGroup", "RibbonGroup", "RibbonControlGroup", "RibbonBox", "RibbonQuickAccessToolbar", "RibbonBackstage", "BackstageTab", "BackstageButton", "BackstageSeparator", "RibbonButton", "RibbonToggleButton", "RibbonRadioButton", "RibbonMenuButton", "RibbonSplitButton", "RibbonColorPicker", "RibbonMenuItem", "RibbonSplitMenuItem", "RibbonCheckBox", "RibbonComboBox", "RibbonTextBox", "RibbonNumericField", "RibbonGallery", "RibbonGalleryCategory", "RibbonGalleryItem", "RibbonLabel", "RibbonSeparator", "Command",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_elements_attributes_lines_and_depths() {
        let text = "<!-- a view -->\n<Panel DesignWidth=\"800\" OnLoad=\"main_view_load\">\n  <TextField x:Name=\"status\" Text=\"a &amp; b\"/>\n  <Button x:Name='hello' OnClick=\"hello_click\" On=\"x\">Go</Button>\n</Panel>\n";
        let els = scan(text).expect("valid view");
        assert_eq!(els.len(), 3);
        assert_eq!((els[0].name.as_str(), els[0].line, els[0].depth), ("Panel", 2, 0));
        assert_eq!(els[1].x_name(), Some("status"));
        assert_eq!(els[1].attribute("Text"), Some("a & b"));
        assert_eq!((els[2].line, els[2].depth), (4, 1));
        assert_eq!(els[2].event_handlers().collect::<Vec<_>>(), vec![("OnClick", "hello_click")]);
        assert_eq!(els[0].event_handlers().collect::<Vec<_>>(), vec![("OnLoad", "main_view_load")]);
    }

    #[test]
    fn the_library_elements_are_the_data_print_and_storage_elements() {
        let mut both: Vec<&str> = DATA_ELEMENTS.iter().chain(PRINT_ELEMENTS).chain(STORAGE_ELEMENTS).copied().collect();
        both.sort_unstable();
        let mut all = LIBRARY_ELEMENTS.to_vec();
        all.sort_unstable();
        assert_eq!(all, both);
        assert!(PRINT_TYPED.iter().all(|t| PRINT_ELEMENTS.contains(t)));
        assert!(STORAGE_TYPED.iter().all(|t| STORAGE_ELEMENTS.contains(t)));
        assert!(PLATFORM_ELEMENTS.iter().all(|(e, p)| !p.is_empty() && (LIBRARY_ELEMENTS.contains(e) || BUILTIN_ELEMENTS.contains(e))));
    }

    #[test]
    fn reports_malformed_views_with_their_line() {
        let err = scan("<Panel>\n  <Button Text=\"x\">\n</Panel>").expect_err("mismatched");
        assert_eq!(err.line, 3);
        assert!(err.message.contains("closes"), "{err}");
        assert!(scan("<Panel Text=x/>").is_err());
        assert!(scan("<Panel>").is_err());
    }

    #[test]
    fn decodes_character_references() {
        assert_eq!(decode("&lt;a&gt; &#65;&#x42; &unknown; &"), "<a> AB &unknown; &");
    }

    #[test]
    fn event_attributes_are_on_plus_an_upper_case_letter() {
        assert!(is_event_attribute("OnClick"));
        assert!(!is_event_attribute("On"));
        assert!(!is_event_attribute("Only"));
        assert!(!is_event_attribute("Text"));
    }
}
