//! Port of `Files.App/Views/Settings/ActionsPage.xaml` + `ActionsViewModel`:
//! the list of commands with their DEFAULT key bindings, extracted verbatim
//! from `Files.App/Actions/**` (`HotKey`/`SecondHotKey`/… properties of every
//! `[GeneratedRichCommand]` action, ordinal order like CommandManager, minus
//! invisible bindings, mouse keys and non-globally-accessible commands).
//! Editing key bindings (ActionsSettingsService) is not ported yet: the
//! search box, "Ajouter" and "Restaurer les valeurs par défaut" are inert.

use drive_app_controls::themes::shape::{height, radius, space};

use crate::ui::{Painter, Rect};
use crate::views::settings::controls::{button_width, ButtonVariant, Control, SettingId, SettingsRow};

/// (Description resource key, key token, KeyModifiers token) — one entry per
/// visible default key binding, in `CommandManager` enumeration order.
pub const DEFAULT_KEY_BINDINGS: [(&str, &str, &str); 72] = [
    ("AddItemDescription", "I", "CtrlShift"),
    ("CloseActivePaneDescription", "W", "CtrlAlt"),
    ("CloseAllTabsDescription", "W", "CtrlShift"),
    ("CloseSelectedTabDescription", "W", "Ctrl"),
    ("CloseSelectedTabDescription", "F4", "Ctrl"),
    ("CopyItemDescription", "C", "Ctrl"),
    ("CopyItemPathDescription", "C", "CtrlShift"),
    ("CopyItemPathWithQuotesDescription", "C", "CtrlAlt"),
    ("CreateFolderDescription", "N", "CtrlShift"),
    ("CutItemDescription", "X", "Ctrl"),
    ("DecompressArchiveDescription", "E", "Ctrl"),
    ("DecompressArchiveHereSmartDescription", "E", "CtrlShift"),
    ("DeleteItemDescription", "Delete", ""),
    ("DeleteItemDescription", "D", "Ctrl"),
    ("DeleteItemPermanentlyDescription", "Delete", "Shift"),
    ("DuplicateSelectedTabDescription", "K", "CtrlShift"),
    ("EditPathDescription", "L", "Ctrl"),
    ("EditPathDescription", "D", "Alt"),
    ("EnterCompactOverlayDescription", "Up", "CtrlAlt"),
    ("ExitCompactOverlayDescription", "Down", "CtrlAlt"),
    ("FocusOtherPaneDescription", "Right", "CtrlShift"),
    ("LaunchPreviewPopupDescription", "Space", ""),
    ("LayoutAdaptiveDescription", "Number6", "CtrlShift"),
    ("LayoutCardsDescription", "Number3", "CtrlShift"),
    ("LayoutColumnsDescription", "Number5", "CtrlShift"),
    ("LayoutDecreaseSizeDescription", "Subtract", "Ctrl"),
    ("LayoutDetailsDescription", "Number1", "CtrlShift"),
    ("LayoutGridDescription", "Number4", "CtrlShift"),
    ("LayoutIncreaseSizeDescription", "Add", "Ctrl"),
    ("LayoutListDescription", "Number2", "CtrlShift"),
    ("NavigateBackDescription", "Left", "Alt"),
    ("NavigateBackDescription", "Back", ""),
    ("NavigateForwardDescription", "Right", "Alt"),
    ("NavigateUpDescription", "Up", "Alt"),
    ("NewTabDescription", "T", "Ctrl"),
    ("NewWindowDescription", "N", "Ctrl"),
    ("NextTabDescription", "Tab", "Ctrl"),
    ("OpenClassicPropertiesDescription", "Enter", "AltShift"),
    ("OpenCommandPaletteDescription", "P", "CtrlShift"),
    ("OpenHelpDescription", "F1", ""),
    ("OpenItemDescription", "Enter", ""),
    ("OpenLogFileDescription", "OemPeriod", "Ctrl"),
    ("OpenLogFileLocationDescription", "OemPeriod", "CtrlShift"),
    ("OpenPropertiesDescription", "Enter", "Alt"),
    ("OpenSettingsDescription", "OemComma", "Ctrl"),
    ("EditSettingsFileDescription", "OemComma", "CtrlShift"),
    ("OpenTerminalDescription", "Oem3", "Ctrl"),
    ("OpenTerminalAsAdminDescription", "Oem3", "CtrlShift"),
    ("PasteItemDescription", "V", "Ctrl"),
    ("PasteItemToSelectionDescription", "V", "CtrlShift"),
    ("PreviousTabDescription", "Tab", "CtrlShift"),
    ("RedoDescription", "Y", "Ctrl"),
    ("RefreshItemsDescription", "R", "Ctrl"),
    ("RefreshItemsDescription", "F5", ""),
    ("RenameDescription", "F2", ""),
    ("ReopenClosedTabDescription", "T", "CtrlShift"),
    ("SearchDescription", "F", "Ctrl"),
    ("SearchDescription", "F3", ""),
    ("SelectAllDescription", "A", "Ctrl"),
    ("SplitPaneHorizontallyDescription", "H", "AltShift"),
    ("AddVerticalPaneDescription", "V", "AltShift"),
    ("ToggleThemeDescription", "T", "CtrlAlt"),
    ("ToggleCompactOverlayDescription", "F12", ""),
    ("ToggleDualPaneDescription", "S", "CtrlShift"),
    ("ToggleFilterHeaderDescription", "F", "CtrlShift"),
    ("ToggleFullScreenDescription", "F11", ""),
    ("ToggleInfoPaneDescription", "I", "CtrlAlt"),
    ("ToggleSelectDescription", "Space", "Ctrl"),
    ("ToggleShowHiddenItemsDescription", "H", "Ctrl"),
    ("ToggleSidebarDescription", "B", "Ctrl"),
    ("ToggleToolbarDescription", "B", "CtrlShift"),
    ("UndoDescription", "Z", "Ctrl"),
];

/// Resolves `{0, plural, one {…} other {…}}` in a description resource, like
/// `GetLocalizedFormatResource(count)` with count = 0 (no selection on the
/// settings page). French CLDR maps 0 to "one", other cultures to "other".
pub fn resolve_description(key: &'static str) -> String {
    let raw = drive_localization::tr(key);
    let Some(start) = raw.find("{0, plural,") else {
        return raw.into();
    };
    // Find the matching closing brace of the plural block.
    let bytes = raw.as_bytes();
    let mut depth = 0usize;
    let mut end = raw.len();
    for (i, &byte) in bytes.iter().enumerate().skip(start) {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    let block = &raw[start..end];
    let branch = if drive_localization::culture().starts_with("fr") { "one" } else { "other" };
    let replacement = extract_branch(block, branch)
        .or_else(|| extract_branch(block, "other"))
        .unwrap_or_default();
    format!("{}{}{}", &raw[..start], replacement, &raw[end..])
}

/// Extracts the content of `name {…}` inside an ICU plural block.
fn extract_branch<'a>(block: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!("{name} {{");
    let at = block.find(&pat)? + pat.len();
    let rest = &block[at..];
    let mut depth = 1usize;
    for (i, b) in rest.bytes().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&rest[..i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// `HotKey.LocalizedLabel` tokens: modifiers (Alt, Ctrl, Shift, Win order)
/// then the key, each as its own KeyboardShortcut chip.
pub fn key_binding_tokens(key: &str, modifiers: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    if modifiers.contains("Alt") {
        tokens.push(tr_key("Menu"));
    }
    if modifiers.contains("Ctrl") {
        tokens.push(tr_key("Control"));
    }
    if modifiers.contains("Shift") {
        tokens.push(tr_key("Shift"));
    }
    if modifiers.contains("Win") {
        tokens.push(tr_key("Windows"));
    }
    tokens.push(key_label(key));
    tokens
}

fn tr_key(name: &str) -> String {
    drive_localization::tr_opt(&format!("Key.{name}"))
        .map(str::to_owned)
        .unwrap_or_else(|| name.to_owned())
}

/// `HotKey.LocalizedKeys[key]`.
fn key_label(key: &str) -> String {
    match key {
        // Named keys -> "Key/<name>" resources.
        "Enter" | "Space" | "Escape" | "Back" | "Tab" | "Insert" | "Delete" | "Left" | "Right"
        | "Down" | "Up" | "Home" | "End" | "PageDown" | "PageUp" | "Pause" => tr_key(key),
        "GoBack" => tr_key("BrowserGoBack"),
        "GoForward" => tr_key("BrowserGoForward"),
        // NumPad keys: "<NumPadTypeName> <key>".
        "Add" => format!("{} +", drive_localization::tr("NumPadTypeName")),
        "Subtract" => format!("{} -", drive_localization::tr("NumPadTypeName")),
        // Number row.
        k if k.starts_with("Number") => k["Number".len()..].to_string(),
        // OEM keys: layout-dependent character (GetKeyCharacter).
        "Oem3" => oem_char(0xC0),
        "OemComma" => oem_char(0xBC),
        "OemPeriod" => oem_char(0xBE),
        "OemMinus" => oem_char(0xBD),
        "OemPlus" => oem_char(0xBB),
        // F-keys and letters, verbatim.
        k => k.to_string(),
    }
}

/// Character produced by a virtual key in the current keyboard layout.
fn oem_char(vk: u32) -> String {
    use windows::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, MAPVK_VK_TO_CHAR};
    let ch = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_CHAR) } & 0xFFFF;
    char::from_u32(ch).map(String::from).unwrap_or_default()
}

/// Top bar height (subtitle "Commandes" + search box + buttons).
pub const TOP_BAR_H: f32 = 44.0;
/// Key-binding cards (ListViewItem MinHeight=48 + margins).
pub const BINDING_ROW_H: f32 = 56.0;

pub fn rows() -> Vec<SettingsRow> {
    let mut rows = Vec::new();
    rows.push(SettingsRow::custom(SettingId::ActTopBar, TOP_BAR_H));
    for (i, (desc, key, modifiers)) in DEFAULT_KEY_BINDINGS.iter().enumerate() {
        rows.push(
            SettingsRow::card(
                SettingId::ActCommand(i),
                crate::views::settings::controls::Icon::None,
                resolve_description(desc),
                Control::Shortcut(key_binding_tokens(key, modifiers)),
            )
            .with_height(BINDING_ROW_H),
        );
    }
    rows
}

impl Painter<'_> {
    /// Subtitle row: "Commandes" + search box + Ajouter/Restaurer buttons
    /// (ActionsPage.xaml top Grid). Editing is not ported: shown greyed.
    pub(crate) fn draw_actions_top_bar(&self, rect: &Rect) {
        let t = self.theme;
        let f = &self.renderer.formats;
        let tr = drive_localization::tr;

        let title = Rect::new(rect.left, rect.top + 8.0, rect.left + 220.0, rect.bottom);
        self.text(tr("Commands"), &title, &f.body_strong, &t.text_primary, false);

        // Restore defaults + Add buttons, right-aligned. `@ui/Button
        // variant="secondary" size="sm"` (h-8, px-3, radius 4, never bold),
        // shown DISABLED (`disabled:opacity-50`): editing is not ported.
        let cy = (rect.top + rect.bottom) / 2.0 + 4.0;
        let btn_h = height::BUTTON_SM;
        let restore = tr("RestoreDefaults");
        let rw = button_width(self, restore);
        let restore_rect = Rect::new(rect.right - rw, cy - btn_h / 2.0, rect.right, cy + btn_h / 2.0);
        self.draw_button(&restore_rect, restore, ButtonVariant::Secondary, false);

        let add = tr("AddCommand");
        let aw = button_width(self, add);
        let add_rect = Rect::new(
            restore_rect.left - space::SM - aw,
            cy - btn_h / 2.0,
            restore_rect.left - space::SM,
            cy + btn_h / 2.0,
        );
        self.draw_button(&add_rect, add, ButtonVariant::Secondary, false);

        // Search box (MinWidth=200, PlaceholderText=Rechercher). `@ui/Input`:
        // h-9, `rounded-md`, `bg-white border-border`, `px-3`, 14px text and a
        // `placeholder:text-text-tertiary` hint.
        let input_h = height::BUTTON_MD;
        let search = Rect::new(
            add_rect.left - space::SM - 200.0,
            cy - input_h / 2.0,
            add_rect.left - space::SM,
            cy + input_h / 2.0,
        );
        self.fill_rounded(&search, radius::SM, &t.layer_background);
        self.stroke_rounded(&search, radius::SM, &t.card_stroke);
        let hint = Rect::new(search.left + space::MD, search.top, search.right - space::SM, search.bottom);
        self.text(tr("Search"), &hint, &f.body, &t.text_tertiary, false);
    }
}
