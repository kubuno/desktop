//! Generic SettingsCard / SettingsExpander / group-header row model shared by
//! all Settings sub-pages (mirrors the CommunityToolkit `SettingsCard` and
//! `SettingsExpander` controls used by `Files.App/Views/Settings/*.xaml`).
//! The Appearance page keeps its dedicated module (swatches, slider…);
//! every other page describes itself as a `Vec<SettingsRow>`.

use kubuno_drive_desktop_app_controls::themes::shape::{radius, space};
use kubuno_desktop_ui::buttons::{Button, Size as ButtonSize, Switch};
use kubuno_desktop_ui::display::Badge;
use kubuno_desktop_ui::editors::Dropdown;
use kubuno_desktop_ui::feedback::{Callout, CalloutVariant};
use kubuno_desktop_ui::{Canvas, Widget, WidgetState};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::ui::{Hot, Painter, Rect, UiState};

/// The `@ui/Button` variants the settings pages need — the design system's own
/// enum now that `kubuno-desktop-ui` ships it, re-exported under the name these pages
/// already spell. Its two rules travel with it: the radius is FIXED at
/// `rounded-md` (4) and a button is NEVER bold — colour and fill carry the
/// hierarchy, weight does not.
pub(crate) use kubuno_desktop_ui::buttons::Variant as ButtonVariant;

/// WinUI SettingsCard MinHeight (calibrated on the running original app).
pub const ROW_H: f32 = 64.0;
/// StackPanel Spacing="4".
pub const ROW_GAP: f32 = 4.0;
/// Group header TextBlock: Padding="0,16,0,4" + FontSize=16 line.
pub const GROUP_HEADER_H: f32 = 42.0;
/// What keeps the InfoBar's box off the top and bottom edges of the card it
/// sits in — the card's own inset, not the callout's (`@ui/Callout` is
/// `w-full` and carries no outer margin of its own).
const INFOBAR_INSET_Y: f32 = space::XS;

/// InfoBar (warning) row height, MEASURED by the primitive that draws it: the
/// `@ui/Callout` height for a one-line message, plus the inset. It used to be a
/// flat 48, which is 3 DIP shorter than the callout actually is — the box and
/// the row it is laid out in can no longer disagree.
pub fn info_bar_height() -> f32 {
    info_bar_callout("").height_for(1) + INFOBAR_INSET_Y * 2.0
}

/// The InfoBar of a settings expander, as `@ui/Callout variant="warning"` —
/// `Severity="Warning"` is the only severity these pages use (the FoldersPage's
/// « calculating folder sizes is resource intensive »).
fn info_bar_callout(message: &str) -> Callout {
    Callout::new(message).with_variant(CalloutVariant::Warning)
}

/// Identifies a row for the click dispatch (page + row, payload for lists).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingId {
    /// Non-interactive rows (group headers, info bars, read-only text).
    None,

    // === GeneralPage ===
    GenLanguage,
    GenDateFormat,
    GenStartupHeader,
    /// Startup pages panel (ItemsHeader): add button + one line per page.
    GenStartupPages,
    GenOpenTabInExistingInstance,
    GenAlwaysSwitchToNewlyOpenedTab,
    GenWidgetsHeader,
    GenWidgetQuickAccess,
    GenWidgetDrives,
    GenWidgetNetworkLocations,
    GenWidgetFileTags,
    GenWidgetRecentFiles,
    GenAlwaysOpenDualPane,
    GenDualPaneArrangement,
    GenContextMenuHeader,
    GenCtxOpenInNewTab,
    GenCtxOpenInNewWindow,
    GenCtxOpenInNewPane,
    GenCtxCopyPath,
    GenCtxCreateFolderWithSelection,
    GenCtxCreateAlternateDataStream,
    GenCtxCreateShortcut,
    GenCtxPinToSideBar,
    GenCtxCompressionOptions,
    GenCtxSendToMenu,
    GenCtxOpenTerminal,
    GenCtxEditTagsMenu,
    GenCtxPinToStart,
    GenCtxOverflow,
    GenSmoothScrolling,
    GenTabScrollDirection,

    // === LayoutPage ===
    LaySyncPreferences,
    LayLayoutType,
    LaySortByHeader,
    LaySortDescending,
    LaySortPriority,
    LayGroupByHeader,
    LayGroupDescending,
    LayGroupByDateUnit,
    LayAutoSizeColumns,
    LayColumnsHeader,
    LayColTag,
    LayColSize,
    LayColType,
    LayColDate,
    LayColDateCreated,

    // === FoldersPage ===
    FolHiddenItemsHeader,
    FolShowHiddenItems,
    FolShowDotFiles,
    FolShowProtectedSystemFiles,
    FolShowAlternateStreams,
    FolShowFileExtensions,
    FolShowThumbnails,
    FolShowCheckboxes,
    FolSingleClickHeader,
    FolSingleClickFiles,
    FolSingleClickFolders,
    FolSingleClickColumnsView,
    FolOpenFoldersNewTab,
    FolDeleteConfirmation,
    FolExtensionWarning,
    FolSelectOnHover,
    FolDoubleClickToGoUp,
    FolScrollToPreviousFolder,
    FolSizeFormat,
    FolCalculateFolderSizesHeader,

    // === ActionsPage ===
    /// Search box + "Ajouter" + "Restaurer les valeurs par défaut".
    ActTopBar,
    /// One row per visible default key binding.
    ActCommand(usize),

    // === TagsPage ===
    TagsHeader,
    TagRow(usize),

    // === DevToolsPage ===
    DevOpenIdeHeader,
    DevIdeName,
    DevIdePath,
    DevGitHub,

    // === AdvancedPage ===
    AdvExportSettings,
    AdvImportSettings,
    AdvEditSettingsFile,
    AdvOpenOnStartup,
    AdvLeaveAppRunning,
    AdvSystemTrayIcon,
    AdvSetAsDefaultFileManager,
    AdvFlattenOptions,

    // === AboutPage ===
    AbtAppInfo,
    AbtSponsor,
    AbtDocumentation,
    AbtDiscussions,
    AbtFeedbackHeader,
    AbtFeatureRequest,
    AbtBugReport,
    AbtLogLocation,
    AbtTranslate,
    AbtLibrariesHeader,
    /// Third-party libraries panel (ItemsHeader grid of links).
    AbtLibrariesPanel,
    AbtGitHubRepo,
    AbtPrivacy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// Standalone SettingsCard.
    Card,
    /// SettingsExpander header; `exp` = index in the page's expanded array.
    ExpanderHeader { exp: usize, expanded: bool },
    /// SettingsCard inside an open SettingsExpander.
    ExpanderItem,
    /// Section TextBlock (FontSize=16 Medium), no card.
    GroupHeader,
    /// InfoBar (severity Warning) inside an expander card.
    InfoBar,
    /// Page-specific block (startup pages, libraries grid…), joined to the
    /// preceding expander card like a SettingsExpander.ItemsHeader.
    Custom,
}

#[derive(Debug, Clone, Copy)]
pub enum Icon {
    None,
    /// Segoe Fluent glyph (FontIcon).
    Glyph(&'static str),
    /// Original ThemedIcon / PathIcon geometry from themed-icons.txt.
    Vector(&'static str),
    /// PathIcon tinted with a custom color (tag swatches).
    VectorColored(&'static str, D2D1_COLOR_F),
}

#[derive(Debug, Clone)]
pub enum Control {
    None,
    Combo(String),
    /// Greyed ComboBox (IsEnabled=False).
    ComboDisabled(String),
    Toggle(bool),
    /// Greyed ToggleSwitch (IsEnabled=False).
    ToggleDisabled(bool),
    /// SettingsCard.ActionIcon (E8A7) — the whole card is clickable.
    Action,
    /// Push button with a text content.
    Button(String),
    /// Small transparent text buttons (Tags list "Modifier"/"Supprimer").
    Buttons(Vec<String>),
    /// Read-only value at the right edge.
    Text(String),
    /// KeyboardShortcut chips (one per key token).
    Shortcut(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct SettingsRow {
    pub id: SettingId,
    pub kind: RowKind,
    pub icon: Icon,
    pub label: String,
    pub description: Option<String>,
    pub control: Control,
    pub height: f32,
}

impl SettingsRow {
    pub fn card(id: SettingId, icon: Icon, label: impl Into<String>, control: Control) -> Self {
        Self {
            id,
            kind: RowKind::Card,
            icon,
            label: label.into(),
            description: None,
            control,
            height: ROW_H,
        }
    }

    pub fn expander(
        id: SettingId,
        icon: Icon,
        label: impl Into<String>,
        control: Control,
        exp: usize,
        expanded: bool,
    ) -> Self {
        Self {
            id,
            kind: RowKind::ExpanderHeader { exp, expanded },
            icon,
            label: label.into(),
            description: None,
            control,
            height: ROW_H,
        }
    }

    pub fn item(id: SettingId, label: impl Into<String>, control: Control) -> Self {
        Self {
            id,
            kind: RowKind::ExpanderItem,
            icon: Icon::None,
            label: label.into(),
            description: None,
            control,
            height: ROW_H,
        }
    }

    pub fn group(label: impl Into<String>) -> Self {
        Self {
            id: SettingId::None,
            kind: RowKind::GroupHeader,
            icon: Icon::None,
            label: label.into(),
            description: None,
            control: Control::None,
            height: GROUP_HEADER_H,
        }
    }

    pub fn info_bar(message: impl Into<String>) -> Self {
        Self {
            id: SettingId::None,
            kind: RowKind::InfoBar,
            icon: Icon::None,
            label: message.into(),
            description: None,
            control: Control::None,
            height: info_bar_height(),
        }
    }

    pub fn custom(id: SettingId, height: f32) -> Self {
        Self {
            id,
            kind: RowKind::Custom,
            icon: Icon::None,
            label: String::new(),
            description: None,
            control: Control::None,
            height,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
}

/// Computed layout of a generic settings page.
pub struct SettingsPageLayout {
    pub rows: Vec<SettingsRow>,
    pub rects: Vec<Rect>,
    pub extent: f32,
}

/// Stacks the rows below the page title, like the AppearancePage layout
/// (page starts at content.left + 300, rows begin at content.top + 104).
pub fn compute(content: &Rect, scroll: f32, rows: Vec<SettingsRow>) -> SettingsPageLayout {
    let page_left = content.left + 300.0;
    let right = content.right - 24.0;
    let mut rects = Vec::with_capacity(rows.len());
    let mut y = content.top + 56.0 - scroll;
    for row in &rows {
        rects.push(Rect::new(page_left, y, right, y + row.height));
        y += row.height + ROW_GAP;
    }
    SettingsPageLayout {
        rows,
        rects,
        extent: (y + scroll) - content.top + 16.0,
    }
}

/// True if the row is drawn INSIDE the card of the preceding expander.
fn joins_card(kind: RowKind) -> bool {
    matches!(kind, RowKind::ExpanderItem | RowKind::InfoBar | RowKind::Custom)
}

/// The ToggleSwitch's On/Off pair for the current culture — WinUI provides
/// them at the platform level, they don't appear in Files' resw.
pub(crate) fn toggle_state_label(on: bool) -> &'static str {
    let lang = kubuno_drive_desktop_localization::culture()
        .split('-')
        .next()
        .unwrap_or("en");
    match (lang, on) {
        ("fr", true) => "Activé",
        ("fr", false) => "Désactivé",
        ("de", true) => "Ein",
        ("de", false) => "Aus",
        ("es", true) => "Activado",
        ("es", false) => "Desactivado",
        ("it", true) => "Attivato",
        ("it", false) => "Disattivato",
        ("pt", true) => "Ativado",
        ("pt", false) => "Desativado",
        (_, true) => "On",
        (_, false) => "Off",
    }
}

/// The bounds a settings row holds its selector to. The desktop's own, and
/// kept: a purely content-sized trigger leaves the column of controls ragged,
/// which is what these two numbers were introduced to stop.
const COMBO_MIN_W: f32 = 188.0;
const COMBO_MAX_W: f32 = 340.0;

/// The selector a settings row shows, as the design system's own primitive.
///
/// **`@ui/Dropdown` and not `@ui/Combobox`.** The choice is not a toss-up: the
/// hand-written control this replaces already described a `Dropdown` in its own
/// doc comment — `padding: '0 4px 0 8px'`, a `gap` of 4 before a 10 DIP caret,
/// and a surface that is TRANSPARENT at rest and only fills on hover or while
/// the list is out. [`kubuno_desktop_ui::lists::ComboBox`] is an `Input` instead: `h-9`
/// FIXED, `px-3` on both sides, a filled white face at rest, a lucide
/// `ChevronDown` in a `w-4` column, and a `p-1` listbox popup rather than the
/// frosted `MenuDropdown` grid. Every one of those five differs from what these
/// pages ship, so adopting the `ComboBox` would have been a redesign; adopting
/// the `Dropdown` is an adoption. Its free trigger height and its `ghost`
/// variant are held in reserve — the settings rows want neither today.
///
/// One option, selected: a settings row paints the CURRENT value and opens the
/// real list through the shell's own flyout (`MainWindow::dropdown`), so the
/// primitive is used for its trigger and its popup never paints. A disabled row
/// keeps its value on screen for the same reason — the primitive's dead path
/// fades the trigger, it does not blank it.
fn settings_dropdown(value: &str) -> Dropdown {
    let mut d = Dropdown::new();
    d.add_option(value, None);
    d.set_selected_index(0);
    d
}

/// Trigger width, MEASURED by the primitive against the real font, then held to
/// the column bounds.
pub fn combo_width(c: &dyn Canvas, value: &str) -> f32 {
    settings_dropdown(value).measure(c).width.clamp(COMBO_MIN_W, COMBO_MAX_W)
}

/// Dropdown rect inside a row; expander headers spare the chevron zone.
pub fn combo_rect(c: &dyn Canvas, row: &Rect, header: bool, value: &str) -> Rect {
    let cy = (row.top + row.bottom) / 2.0;
    let right = if header { row.right - 44.0 } else { row.right - 12.0 };
    let d = settings_dropdown(value);
    let w = d.measure(c).width.clamp(COMBO_MIN_W, COMBO_MAX_W);
    let h = d.height;
    Rect::new(right - w, cy - h / 2.0, right, cy + h / 2.0)
}

/// Horizontal space reserved by a row's content control (for label clipping).
/// Every measured control asks the primitive that draws it, so a label is
/// clipped exactly where its control starts.
fn control_reserved(c: &dyn Canvas, control: &Control, header: bool) -> f32 {
    let chevron = if header { 32.0 } else { 0.0 };
    match control {
        Control::None => 50.0,
        Control::Toggle(_) | Control::ToggleDisabled(_) => 78.0 + chevron,
        Control::Action => 50.0,
        Control::Combo(v) | Control::ComboDisabled(v) => combo_width(c, v) + 24.0 + chevron,
        Control::Button(t) => button_width(c, t) + 24.0 + chevron,
        Control::Buttons(labels) => {
            labels
                .iter()
                .map(|l| text_button(l).width(c) + space::XS)
                .sum::<f32>()
                + 24.0
        }
        Control::Text(_) => 330.0,
        Control::Shortcut(tokens) => {
            tokens
                .iter()
                .map(|t| shortcut_badge(t).measure(c).width + CHIP_GAP)
                .sum::<f32>()
                + 24.0
        }
    }
}

/// ToggleSwitch zone in an expander header (left of the chevron).
pub fn header_toggle_rect(row: &Rect) -> Rect {
    Rect::new(row.right - 100.0, row.top, row.right - 44.0, row.bottom)
}

/// A settings row's push button — `@ui/Button size="sm"` (h-8, `px-3`, radius
/// 4, never bold). The variant is the caller's: a card shows `Secondary`, the
/// tags list shows `Text`.
fn settings_button(label: &str, variant: ButtonVariant) -> Button {
    Button::new(label).variant(variant).size(ButtonSize::Sm)
}

/// The tags list's "Modifier"/"Supprimer": the accent without a fill.
fn text_button(label: &str) -> Button {
    settings_button(label, ButtonVariant::Text)
}

/// The floor a settings button's width is held to. The desktop's own — a click
/// target narrower than that reads as an accident — and deliberately NOT
/// applied to the small text buttons, which hang inside a list row and have to
/// hug their label.
const BUTTON_MIN_W: f32 = 72.0;

/// Push-button width, MEASURED by the primitive against the real font.
pub fn button_width(c: &dyn Canvas, text: &str) -> f32 {
    settings_button(text, ButtonVariant::Secondary).width(c).max(BUTTON_MIN_W)
}

/// Push-button rect for `Control::Button`; expander headers spare the chevron.
pub fn button_rect(c: &dyn Canvas, row: &Rect, text: &str, header: bool) -> Rect {
    let cy = (row.top + row.bottom) / 2.0;
    let w = button_width(c, text);
    let h = ButtonSize::Sm.height();
    let right = if header { row.right - 44.0 } else { row.right - 12.0 };
    Rect::new(right - w, cy - h / 2.0, right, cy + h / 2.0)
}

/// Small text-button rects for `Control::Buttons`, right to left.
pub fn text_button_rects(c: &dyn Canvas, row: &Rect, labels: &[String]) -> Vec<Rect> {
    let cy = (row.top + row.bottom) / 2.0;
    let h = ButtonSize::Sm.height();
    let mut right = row.right - 12.0;
    let mut rects = Vec::new();
    for label in labels.iter().rev() {
        let w = text_button(label).width(c);
        rects.push(Rect::new(right - w, cy - h / 2.0, right, cy + h / 2.0));
        right -= w + space::XS;
    }
    rects.reverse();
    rects
}

/// A keyboard-shortcut chip, as the `@ui/Badge` primitive (`default`, size
/// `md`): `rounded-full`, `bg-surface-2`, `text-text-secondary`, `text-xs px-2
/// py-0.5`. It measures and paints itself — the chip's width and its 20 DIP
/// pill height are the primitive's, not this file's.
fn shortcut_badge(token: &str) -> Badge {
    Badge::new(token)
}

/// Gap between two chips (`Badge` has no gap of its own — this is ours).
const CHIP_GAP: f32 = 6.0;

impl Painter<'_> {
    /// Draws a generic settings page (every section except Appearance).
    pub(crate) fn draw_settings_page(&self, page: &SettingsPageLayout, state: &UiState) {
        let t = self.theme;
        let f = &self.renderer.formats;

        // Cards: an expander header + its items/info bars share one card.
        let mut i = 0usize;
        while i < page.rows.len() {
            let kind = page.rows[i].kind;
            if matches!(kind, RowKind::GroupHeader) {
                i += 1;
                continue;
            }
            let rect = page.rects[i];
            let mut last = i;
            if matches!(kind, RowKind::ExpanderHeader { .. }) {
                while last + 1 < page.rows.len() && joins_card(page.rows[last + 1].kind) {
                    last += 1;
                }
            }
            // `@ui/Card`: `rounded-xl border border-border bg-surface-0`.
            let group = Rect::new(rect.left, rect.top, rect.right, page.rects[last].bottom);
            self.fill_rounded(&group, radius::XL, &t.layer_background);
            self.stroke_rounded(&group, radius::XL, &t.card_stroke);
            for j in (i + 1)..=last {
                // An InfoBar is a `@ui/Callout` — a box with its OWN border,
                // so it doesn't take the card's row separator on top of it.
                if matches!(page.rows[j].kind, RowKind::InfoBar) {
                    continue;
                }
                let r = page.rects[j];
                self.hairline(r.left, r.top - 2.0, r.right);
            }
            i = last + 1;
        }

        // Row contents.
        for (idx, row) in page.rows.iter().enumerate() {
            let rect = &page.rects[idx];
            let interactive = row.id != SettingId::None || matches!(row.kind, RowKind::ExpanderHeader { .. });
            if interactive
                && !matches!(row.kind, RowKind::GroupHeader | RowKind::InfoBar)
                && state.hot == Some(Hot::SettingRow(idx))
            {
                // A standalone row IS the card, so it takes the card's radius;
                // a row inside a card only rounds like a small control.
                let r = if matches!(row.kind, RowKind::Card) { radius::XL } else { radius::SM };
                self.fill_rounded(rect, r, &t.control_fill_hover);
            }
            match row.kind {
                RowKind::GroupHeader => {
                    // Section header: `--kb-text-heading` (16) medium, like a
                    // `@ui/Card` title. Padding="0,16,0,4".
                    let text = Rect::new(rect.left, rect.top + 12.0, rect.right, rect.bottom);
                    self.text(&row.label, &text, &f.heading, &t.text_primary, false);
                    continue;
                }
                RowKind::InfoBar => {
                    self.draw_info_bar(rect, &row.label);
                    continue;
                }
                RowKind::Custom => continue, // drawn by the page module
                _ => {}
            }

            // HeaderIcon (24px zone at the left edge, like the SettingsCard).
            // `@ui/Card`: the header icon is 16px `text-text-secondary`.
            let icon_rect = Rect::new(rect.left + 12.0, rect.top, rect.left + 36.0, rect.bottom);
            match row.icon {
                Icon::None => {}
                Icon::Glyph(glyph) => self.text(glyph, &icon_rect, &f.icon, &t.text_secondary, true),
                Icon::Vector(name) => self.vector_icon(name, &icon_rect, 16.0, &t.text_secondary),
                Icon::VectorColored(name, color) => self.vector_icon(name, &icon_rect, 16.0, &color),
            }

            // Header (+ Description) text, clipped before the content control.
            let header = matches!(row.kind, RowKind::ExpanderHeader { .. });
            let label_left = rect.left + 48.0;
            let label_right = rect.right - control_reserved(self, &row.control, header);
            if let Some(desc) = &row.description {
                let top = Rect::new(label_left, rect.top + 10.0, label_right, rect.top + 34.0);
                let sub = Rect::new(label_left, rect.top + 32.0, label_right, rect.bottom - 8.0);
                self.text(&row.label, &top, &f.body, &t.text_primary, false);
                self.text(desc, &sub, &f.caption, &t.text_secondary, false);
            } else {
                let label_rect = Rect::new(label_left, rect.top, label_right, rect.bottom);
                self.text(&row.label, &label_rect, &f.body, &t.text_primary, false);
            }

            // Content control.
            match &row.control {
                Control::None => {}
                Control::Combo(value) | Control::ComboDisabled(value) => {
                    let combo = combo_rect(self, rect, header, value);
                    let disabled = matches!(row.control, Control::ComboDisabled(_));
                    self.draw_combo(&combo, value, disabled);
                }
                Control::Toggle(on) => {
                    if header {
                        self.toggle_switch_in(&header_toggle_rect(rect), *on);
                    } else {
                        self.toggle_switch(rect, *on);
                    }
                }
                Control::ToggleDisabled(on) => {
                    // WinUI greys a disabled switch: draw at reduced contrast.
                    let zone = if header {
                        header_toggle_rect(rect)
                    } else {
                        Rect::new(rect.right - 62.0, rect.top, rect.right - 22.0, rect.bottom)
                    };
                    self.toggle_switch_disabled(&zone, *on);
                }
                Control::Action => {
                    let action = Rect::new(rect.right - 40.0, rect.top, rect.right - 12.0, rect.bottom);
                    self.text("\u{E8A7}", &action, &f.icon_small, &t.text_secondary, true);
                }
                Control::Button(text) => {
                    let btn = button_rect(self, rect, text, header);
                    self.draw_button(&btn, text, ButtonVariant::Secondary, true);
                }
                Control::Buttons(labels) => {
                    // The tags list's "Modifier"/"Supprimer": accent-coloured
                    // labels with no fill — `@ui/Button variant="text"`.
                    for (btn, label) in text_button_rects(self, rect, labels).iter().zip(labels) {
                        self.draw_button(btn, label, ButtonVariant::Text, true);
                    }
                }
                Control::Text(value) => {
                    let zone = Rect::new(rect.right - 320.0, rect.top, rect.right - 12.0, rect.bottom);
                    self.text_aligned(
                        value,
                        &zone,
                        &f.body,
                        &t.text_secondary,
                        windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_TRAILING,
                    );
                }
                Control::Shortcut(tokens) => {
                    // KeyboardShortcut chips, right-aligned — one `@ui/Badge`
                    // per key token, each measured and painted by the
                    // primitive (pill radius included: `rounded-full` is half
                    // the height it is actually given).
                    let cy = (rect.top + rect.bottom) / 2.0;
                    let mut right = rect.right - 16.0;
                    for token in tokens.iter().rev() {
                        let badge = shortcut_badge(token);
                        let size = badge.measure(self);
                        let chip = Rect::new(
                            right - size.width,
                            cy - size.height / 2.0,
                            right,
                            cy + size.height / 2.0,
                        );
                        badge.paint(self, chip, WidgetState::REST);
                        right -= size.width + CHIP_GAP;
                    }
                }
            }

            // Expander chevron at the far right.
            if let RowKind::ExpanderHeader { expanded, .. } = row.kind {
                self.settings_expander_chevron(rect, expanded);
            }
        }
    }

    /// The single selector painter of the settings pages — the generic rows and
    /// the Appearance page both come through here — now the `@ui/Dropdown`
    /// primitive (see [`settings_dropdown`] for why it, and not the ComboBox).
    ///
    /// `disabled` keeps the VALUE on screen: a greyed `IsEnabled=False`
    /// ComboBox still shows what it is set to, and the primitive's dead path
    /// fades the trigger rather than blanking it.
    pub(crate) fn draw_combo(&self, combo: &Rect, value: &str, disabled: bool) {
        settings_dropdown(value).paint(self, *combo, WidgetState::REST.disabled(disabled));
    }

    /// `@ui/Button`, as the primitive. `rect` is the caller's — the settings
    /// rows lay their buttons out themselves (right-aligned inside a row, or
    /// inside the Actions page's top bar), so the size comes from
    /// [`button_width`] and the paint from here.
    pub(crate) fn draw_button(&self, rect: &Rect, label: &str, variant: ButtonVariant, enabled: bool) {
        settings_button(label, variant).paint(self, *rect, WidgetState::REST.disabled(!enabled));
    }

    /// InfoBar severity=Warning, as the `@ui/Callout` primitive. Only the
    /// horizontal inset is ours: the callout is `w-full`, so what holds it off
    /// the card's edges is the card's padding, not the control's.
    fn draw_info_bar(&self, rect: &Rect, message: &str) {
        let box_rect = Rect::new(
            rect.left + space::MD,
            rect.top + INFOBAR_INSET_Y,
            rect.right - space::MD,
            rect.bottom - INFOBAR_INSET_Y,
        );
        info_bar_callout(message).paint(self, box_rect, WidgetState::REST);
    }

    /// Every switch of a settings page, as `kubuno_desktop_ui::buttons::Switch`.
    ///
    /// `left` is the track's left edge and `center_y` the row's middle; the
    /// track is returned so the caller can place the state label against it.
    /// Handed exactly the rectangle the primitive measures, `Switch::track`
    /// gives it straight back, which is what makes the origin authoritative.
    pub(crate) fn settings_switch(&self, left: f32, center_y: f32, on: bool, enabled: bool) -> Rect {
        let switch = Switch::new().on(on);
        let size = switch.measure(self);
        let track = Rect::new(
            left,
            center_y - size.height / 2.0,
            left + size.width,
            center_y + size.height / 2.0,
        );
        switch.paint(self, track, WidgetState::REST.disabled(!enabled));
        track
    }

    /// The width a switch occupies — asked of the primitive, so the zones that
    /// reserve room for one cannot drift from what it paints.
    pub(crate) fn switch_width(&self) -> f32 {
        Switch::new().measure(self).width
    }

    /// ToggleSwitch drawn inside an explicit zone (header rows).
    fn toggle_switch_in(&self, zone: &Rect, on: bool) {
        let cy = (zone.top + zone.bottom) / 2.0;
        let track = self.settings_switch(zone.left, cy, on, true);
        self.toggle_state_text(&track, zone, on, false);
    }

    /// The ToggleSwitch's state label ("Activé"/"Désactivé"), to the left
    /// of the track — the On/Off strings come from WinUI, not Files' resw;
    /// we provide them for the embedded cultures, English otherwise.
    pub(crate) fn toggle_state_text(&self, track: &Rect, zone: &Rect, on: bool, greyed: bool) {
        let t = self.theme;
        let label = toggle_state_label(on);
        let rect = Rect::new(track.left - 170.0, zone.top, track.left - 12.0, zone.bottom);
        let color = if greyed {
            D2D1_COLOR_F { a: t.text_secondary.a * 0.4, ..t.text_secondary }
        } else {
            t.text_primary
        };
        self.text_aligned(
            label,
            &rect,
            &self.renderer.formats.body,
            &color,
            windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_TRAILING,
        );
    }

    /// Greyed ToggleSwitch (IsEnabled=False): the same switch, painted dead.
    fn toggle_switch_disabled(&self, zone: &Rect, on: bool) {
        let cy = (zone.top + zone.bottom) / 2.0;
        let left = zone.left.max(zone.right - self.switch_width());
        let track = self.settings_switch(left, cy, on, false);
        self.toggle_state_text(&track, zone, on, true);
    }

    /// Expander chevron (E70D/E70E) at the far right of a header row.
    /// `@ui/Accordion`: `<ChevronDown size={16} className="text-text-tertiary">`.
    pub(crate) fn settings_expander_chevron(&self, row: &Rect, expanded: bool) {
        let f = &self.renderer.formats;
        let chev = Rect::new(row.right - 40.0, row.top, row.right - 12.0, row.bottom);
        let glyph = if expanded { "\u{E70E}" } else { "\u{E70D}" };
        self.text(glyph, &chev, &f.icon, &self.theme.text_tertiary, true);
    }
}
