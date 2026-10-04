//! What a view asks of the window that shows it (`vskubuno/docs/EVENTS.md` §16): its `Form`
//! properties (the root element's `Title`, `Icon`, `StartPosition`, `FormBorderStyle`, caption
//! buttons, `ShowInTaskbar`, `TopMost`, `Opacity`, `WindowState`, `MinimumSize`/`MaximumSize`,
//! `AcceptButton`/`CancelButton`, `KeyPreview`), its `<ToolTip>` settings and its `<ContextMenu>`s —
//! read once per compile — and the runtime state that shows them: the tooltip under the pointer
//! and the open context menu.

use kubuno_desktop_controls::host::{self, FormBorderStyle, FormOptions, StartPosition, WindowState};
use kubuno_desktop_controls::toolstrip::StripItem;
use kubuno_desktop_ui::lists::{Menu, MenuEntry, MenuKey};
use kubuno_desktop_ui::{Rect, Widget, WidgetState};

use crate::ast::{AstNode, Element};
use crate::binding::{PropSource, ViewModel};
use crate::props::Props;

/// The kind of window a view is (`WindowKind`): a preset of the `Form` properties a WinForms
/// developer would otherwise set one by one. A property the view writes itself wins over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowKind {
    /// A main window (WinForms' defaults).
    #[default]
    Form,
    /// A modal dialog: `FixedDialog`, no minimise/maximise, centred on its owner, no task bar
    /// button.
    Dialog,
    /// A tool window: `SizableToolWindow` (slim band, close button only), no task bar button.
    ToolWindow,
    /// A splash screen: borderless, centred on the screen, top-most, no task bar button; it closes
    /// itself after `SplashDuration` milliseconds.
    Splash,
    /// A flyout: borderless, rounded, Acrylic, no task bar button; it closes when it loses the
    /// activation (a click outside).
    Flyout,
    /// A document window drawn inside its MDI parent (`MdiParent`), in the in-window
    /// `FloatingWindow` look.
    MdiChild,
}

impl WindowKind {
    pub fn parse(text: &str) -> Self {
        match text {
            "Dialog" => Self::Dialog,
            "ToolWindow" => Self::ToolWindow,
            "Splash" => Self::Splash,
            "Flyout" => Self::Flyout,
            "MdiChild" => Self::MdiChild,
            _ => Self::Form,
        }
    }

    /// The `FormBorderStyle` it implies.
    pub fn border_style(self) -> FormBorderStyle {
        match self {
            Self::Form | Self::MdiChild => FormBorderStyle::Sizable,
            Self::Dialog => FormBorderStyle::FixedDialog,
            Self::ToolWindow => FormBorderStyle::SizableToolWindow,
            Self::Splash | Self::Flyout => FormBorderStyle::None,
        }
    }
}

/// A colour property: a theme token (follows the theme) or a free colour.
fn color_of(root: &Element, name: &str) -> Option<crate::style::ColorValue> {
    let text = root.attribute(name)?.value()?;
    crate::style::parse_color(text.trim()).ok().flatten()
}

/// `CaptionButtons="pin:Pin:Keep on top; settings:Settings2:Settings"`: the window's own caption
/// buttons, `id:Glyph[:Tooltip]` separated by `;`. An id starting with `!` is disabled, one ending
/// with `*` is a toggle that is on.
pub fn parse_caption_buttons(text: &str) -> Vec<kubuno_desktop_controls::window_chrome::CaptionCommand> {
    text.split(';')
        .filter_map(|item| {
            let mut parts = item.splitn(3, ':').map(str::trim);
            let id = parts.next().filter(|s| !s.is_empty())?;
            let glyph = parts.next().unwrap_or("").to_string();
            let tooltip = parts.next().unwrap_or("").to_string();
            let enabled = !id.starts_with('!');
            let checked = id.ends_with('*');
            let id = id.trim_start_matches('!').trim_end_matches('*').to_string();
            Some(kubuno_desktop_controls::window_chrome::CaptionCommand { id, glyph, tooltip, enabled, checked })
        })
        .collect()
}

/// The root element's `Form` properties (see the module doc).
#[derive(Default, Clone)]
pub struct FormSpec {
    title: Option<PropSource<String>>,
    subtitle: Option<PropSource<String>>,
    icon: Option<String>,
    pub kind: WindowKind,
    start_position: StartPosition,
    location: Option<(f32, f32)>,
    border_style: FormBorderStyle,
    control_box: bool,
    minimize_box: bool,
    maximize_box: bool,
    show_in_taskbar: bool,
    top_most: Option<PropSource<bool>>,
    opacity: Option<PropSource<f32>>,
    window_state: Option<PropSource<String>>,
    min_size: Option<(f32, f32)>,
    max_size: Option<(f32, f32)>,
    /// `AcceptButton`, `CancelButton`: the `x:Name` of a button.
    pub accept_button: Option<String>,
    pub cancel_button: Option<String>,
    pub key_preview: bool,
    show_icon: bool,
    help_button: bool,
    size_grip: host::SizeGripStyle,
    transparency_key: Option<crate::style::ColorValue>,
    resize_border: bool,
    /// `Chrome`: `None` = the application's.
    pub chrome: Option<host::Chrome>,
    backdrop: Option<host::Backdrop>,
    corner: host::CornerPreference,
    /// `CornerRadius`: the radius of the window's corners in DIP, over `CornerPreference`
    /// (`host::FormOptions::corner_radius`).
    corner_radius: Option<f32>,
    /// `CornerRadius` on a flyout: shown as a floating panel rounded at it (`host::FloatingPanel`).
    panel_radius: Option<f32>,
    border_color: Option<crate::style::ColorValue>,
    /// `TitleBarStyle` as resolved ([`title_bar_style`]: written, else `Tall` for a view hosting the header's menus).
    title_bar_style: kubuno_desktop_controls::window_chrome::TitleBarStyle,
    title_bar_height: Option<f32>,
    /// `TitleBarPadding`: the band's side insets (`None`: the chrome's own).
    title_bar_padding: Option<f32>,
    title_bar_background: Option<crate::style::ColorValue>,
    title_bar_foreground: Option<crate::style::ColorValue>,
    /// `AccentColor`: the window's own accent (its band, its primary buttons…).
    pub accent: Option<crate::style::ColorValue>,
    title_alignment: kubuno_desktop_controls::window_chrome::TitleAlignment,
    caption_style: kubuno_desktop_controls::window_chrome::ButtonStyle,
    caption_buttons: Vec<kubuno_desktop_controls::window_chrome::CaptionCommand>,
    extend_content: bool,
    show_title: bool,
    right_to_left_layout: bool,
    /// `IsMdiContainer`: the view hosts MDI children in its client area.
    pub is_mdi_container: bool,
    /// `SplashDuration` (ms): a splash screen closes itself after it (`0`: stays).
    pub splash_duration: f32,
    /// The view shows some of the header's standard items (`HeaderSpec`) on a band it does not colour (`coloured_band`):
    /// the band takes the web header's neutral look.
    pub header_items: bool,
}

impl FormSpec {
    /// The `Form` properties written on `root` (`base_dir` resolves a relative icon path).
    pub fn read(root: &Element, base_dir: Option<&std::path::Path>) -> Self {
        let Some(meta) = root.name().and_then(|n| crate::registry::lookup(&n)) else { return Self::defaults() };
        let props = Props::new(root, meta);
        let literal = |name: &str| root.attribute(name).and_then(|a| a.value()).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let flag = |name: &str, default: bool| literal(name).map(|v| v == "true").unwrap_or(default);
        let size = |name: &str| literal(name).and_then(|v| crate::style::parse_size(&v).ok()).filter(|(w, h)| *w > 0.0 || *h > 0.0);
        let kind = literal("WindowKind").map(|k| WindowKind::parse(&k)).unwrap_or_default();
        let dialog = kind == WindowKind::Dialog;
        let floating = matches!(kind, WindowKind::Splash | WindowKind::Flyout);
        // A view's own `Title` wins over a root `Card`'s (the card's title is also the window's then).
        let title = root.attribute("Title").is_some().then(|| props.str("Title", "")).and_then(Result::ok);
        let subtitle = root.attribute("Subtitle").is_some().then(|| props.str("Subtitle", "")).and_then(Result::ok);
        let icon = literal("Icon").map(|p| match base_dir {
            // `{Res app}` → `kbres:app` (vskubuno docs/RESOURCES.md).
            _ if crate::resources::image_uri(&p).is_some() => crate::resources::image_uri(&p).unwrap_or_default(),
            // A Lucide glyph name (`Icon="FileText"`) is not a path.
            Some(dir) if std::path::Path::new(&p).is_relative() && p.contains(['.', '/', '\\']) => dir.join(&p).to_string_lossy().into_owned(),
            _ => p,
        });
        let location = match (literal("X").and_then(|v| v.parse().ok()), literal("Y").and_then(|v| v.parse().ok())) {
            (Some(x), Some(y)) => Some((x, y)),
            (Some(x), None) => Some((x, 0.0)),
            (None, Some(y)) => Some((0.0, y)),
            _ => None,
        };
        use kubuno_desktop_controls::window_chrome::{ButtonStyle, TitleAlignment};
        Self {
            title,
            subtitle,
            icon,
            kind,
            start_position: match literal("StartPosition").as_deref() {
                Some("Manual") => StartPosition::Manual,
                Some("CenterScreen") => StartPosition::CenterScreen,
                Some("WindowsDefaultBounds") => StartPosition::WindowsDefaultBounds,
                Some("CenterParent") => StartPosition::CenterParent,
                Some(_) => StartPosition::WindowsDefaultLocation,
                None if dialog => StartPosition::CenterParent,
                None if kind == WindowKind::Splash => StartPosition::CenterScreen,
                None => StartPosition::WindowsDefaultLocation,
            },
            location,
            border_style: match literal("FormBorderStyle").as_deref() {
                Some("None") => FormBorderStyle::None,
                Some("FixedSingle") => FormBorderStyle::FixedSingle,
                Some("Fixed3D") => FormBorderStyle::Fixed3D,
                Some("FixedDialog") => FormBorderStyle::FixedDialog,
                Some("FixedToolWindow") => FormBorderStyle::FixedToolWindow,
                Some("SizableToolWindow") => FormBorderStyle::SizableToolWindow,
                Some(_) => FormBorderStyle::Sizable,
                None => kind.border_style(),
            },
            control_box: flag("ControlBox", true),
            minimize_box: flag("MinimizeBox", !dialog),
            maximize_box: flag("MaximizeBox", !dialog),
            show_in_taskbar: flag("ShowInTaskbar", matches!(kind, WindowKind::Form | WindowKind::MdiChild)),
            top_most: match root.attribute("TopMost") {
                Some(_) => props.bool("TopMost", false).ok(),
                None => (kind == WindowKind::Splash).then_some(PropSource::Literal(true)),
            },
            opacity: root.attribute("Opacity").is_some().then(|| props.f32("Opacity", 100.0)).and_then(Result::ok),
            window_state: root.attribute("WindowState").is_some().then(|| props.str("WindowState", "Normal")).and_then(Result::ok),
            min_size: size("MinimumSize"),
            max_size: size("MaximumSize"),
            accept_button: literal("AcceptButton"),
            cancel_button: literal("CancelButton"),
            key_preview: flag("KeyPreview", false),
            show_icon: flag("ShowIcon", true),
            help_button: flag("HelpButton", false),
            size_grip: match literal("SizeGripStyle").as_deref() {
                Some("Show") => host::SizeGripStyle::Show,
                Some("Hide") => host::SizeGripStyle::Hide,
                _ => host::SizeGripStyle::Auto,
            },
            transparency_key: color_of(root, "TransparencyKey"),
            resize_border: flag("ResizeBorder", false),
            chrome: match literal("Chrome").as_deref() {
                Some("Kubuno") => Some(host::Chrome::Kubuno),
                Some("System") => Some(host::Chrome::System),
                Some("None") => Some(host::Chrome::Custom),
                _ => None,
            },
            backdrop: match literal("Backdrop").as_deref() {
                Some("None") => Some(host::Backdrop::None),
                Some("Mica") => Some(host::Backdrop::Mica),
                Some("MicaAlt") => Some(host::Backdrop::MicaAlt),
                Some("Acrylic") => Some(host::Backdrop::Acrylic),
                _ if kind == WindowKind::Flyout => Some(host::Backdrop::Acrylic),
                _ => None,
            },
            corner: match literal("CornerPreference").as_deref() {
                Some("Round") => host::CornerPreference::Round,
                Some("RoundSmall") => host::CornerPreference::RoundSmall,
                Some("DoNotRound") => host::CornerPreference::DoNotRound,
                _ if floating => host::CornerPreference::Round,
                _ => host::CornerPreference::Default,
            },
            // A splash screen and a flyout are rounded like a window (`Round` above) though they
            // have no title bar; a flyout given a radius is a floating panel at that radius, `0`
            // keeps a plain, square flyout.
            corner_radius: literal("CornerRadius").and_then(|v| host::form::parse_corner_radius(&v)),
            panel_radius: literal("CornerRadius").and_then(|v| host::form::parse_corner_radius(&v)).filter(|r| kind == WindowKind::Flyout && *r > 0.0),
            border_color: color_of(root, "BorderColor"),
            title_bar_style: title_bar_style(root),
            title_bar_height: literal("TitleBarHeight").and_then(|v| v.parse::<f32>().ok()).filter(|v| *v > 0.0),
            title_bar_padding: literal("TitleBarPadding").and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite() && *v >= 0.0),
            title_bar_background: color_of(root, "TitleBarBackground"),
            title_bar_foreground: color_of(root, "TitleBarForeground"),
            accent: color_of(root, "AccentColor"),
            title_alignment: if literal("TitleAlignment").as_deref() == Some("Center") { TitleAlignment::Center } else { TitleAlignment::Left },
            caption_style: if literal("CaptionButtonStyle").as_deref() == Some("Windows") { ButtonStyle::Windows } else { ButtonStyle::Kubuno },
            caption_buttons: literal("CaptionButtons").map(|v| parse_caption_buttons(&v)).unwrap_or_default(),
            extend_content: flag("ExtendContentIntoTitleBar", false),
            show_title: flag("ShowTitle", true),
            right_to_left_layout: flag("RightToLeftLayout", false),
            is_mdi_container: flag("IsMdiContainer", false),
            splash_duration: literal("SplashDuration")
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(if kind == WindowKind::Splash { 3000.0 } else { 0.0 }),
            header_items: !HeaderSpec::read(root).is_empty() && !coloured_band(root),
        }
    }

    /// The properties of a view that writes none.
    pub fn read_default() -> Self {
        Self::defaults()
    }

    fn defaults() -> Self {
        Self { control_box: true, minimize_box: true, maximize_box: true, show_in_taskbar: true, show_icon: true, show_title: true, ..Self::default() }
    }

    /// The window's `Form` properties this frame (bindings resolved against `vm`), colours in the
    /// light theme — see [`FormSpec::options_in`].
    pub fn options(&self, vm: &dyn ViewModel) -> FormOptions {
        self.options_in(vm, None)
    }

    /// The window's `Form` properties this frame, theme colours (`TitleBarBackground="Primary"`)
    /// resolved in `theme` (the light theme when `None`).
    pub fn options_in(&self, vm: &dyn ViewModel, theme: Option<&kubuno_desktop_ui::Theme>) -> FormOptions {
        let light;
        let theme = match theme {
            Some(t) => t,
            None => {
                light = kubuno_desktop_ui::Theme::light();
                &light
            }
        };
        let hc = crate::style::high_contrast();
        let color = |c: &Option<crate::style::ColorValue>| c.as_ref().map(|c| c.resolve_with(theme, hc));
        // The band takes the window's own accent unless it has a colour of its own.
        // A band carrying the header's standard items takes the web header's look (`var(--body-bg)`, the text colour)
        // unless the view colours it: the items are drawn for a neutral ground, as on the web.
        let neutral = self.header_items && self.title_bar_background.is_none() && self.accent.is_none();
        let token = |name: &str| crate::style::parse_color(name).ok().flatten().map(|c| c.resolve_with(theme, hc));
        let header_ink = if neutral { token("TextPrimary") } else { None };
        let band = color(&self.title_bar_background).or_else(|| color(&self.accent)).or_else(|| if neutral { token("Background") } else { None });
        FormOptions {
            title: self.title.as_ref().map(|t| t.resolve(vm)).filter(|t| !t.is_empty()),
            icon: self.icon.clone(),
            start_position: self.start_position,
            location: self.location,
            border_style: self.border_style,
            control_box: self.control_box,
            minimize_box: self.minimize_box,
            maximize_box: self.maximize_box,
            show_in_taskbar: self.show_in_taskbar,
            top_most: self.top_most.as_ref().is_some_and(|t| t.resolve(vm)),
            opacity: self.opacity.as_ref().map(|o| (o.resolve(vm) / 100.0).clamp(0.0, 1.0)).unwrap_or(1.0),
            window_state: match self.window_state.as_ref().map(|s| s.resolve(vm)).as_deref() {
                Some("Minimized") => WindowState::Minimized,
                Some("Maximized") => WindowState::Maximized,
                _ => WindowState::Normal,
            },
            min_client_size: self.min_size,
            max_client_size: self.max_size,
            chrome: kubuno_desktop_controls::window_chrome::ChromeStyle {
                size: self.title_bar_style,
                height: self.title_bar_height,
                padding: self.title_bar_padding,
                background: band,
                foreground: color(&self.title_bar_foreground).or(header_ink),
                subtitle: self.subtitle.as_ref().map(|s| s.resolve(vm)).unwrap_or_default(),
                alignment: self.title_alignment,
                buttons: self.caption_style,
                show_icon: self.show_icon,
                show_title: self.show_title,
                help_button: self.help_button,
                commands: self.caption_buttons.clone(),
                extend_content: self.extend_content,
                right_to_left: self.right_to_left_layout,
                tool: false,
            },
            corner: self.corner,
            corner_radius: self.corner_radius,
            border_color: color(&self.border_color),
            backdrop: self.backdrop,
            transparency_key: color(&self.transparency_key),
            size_grip: self.size_grip,
            resize_border: self.resize_border,
            panel: self.panel_radius.map(host::FloatingPanel::new),
        }
    }
}

/// Where a child of the view's root goes in the window, besides the page (`TitleBar.Region`,
/// `ActionBar.Region`, `TitleBar.Drag`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChildPlace {
    pub title: TitleRegion,
    pub action: ActionRegion,
    /// `TitleBar.Drag`: the child drags the window.
    pub drag: bool,
}

/// A region of the title band (`TitleBar.Region`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TitleRegion {
    #[default]
    None,
    Left,
    Center,
    Right,
}

/// A side of the action bar (`ActionBar.Region`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActionRegion {
    #[default]
    None,
    Left,
    Right,
}

impl ChildPlace {
    /// The attached properties written on `child`.
    pub fn read(child: &Element) -> Self {
        let value = |name: &str| child.attribute(name).and_then(|a| a.value()).map(|v| v.trim().to_string()).unwrap_or_default();
        Self {
            title: match value("TitleBar.Region").as_str() {
                "Left" => TitleRegion::Left,
                "Center" => TitleRegion::Center,
                "Right" => TitleRegion::Right,
                _ => TitleRegion::None,
            },
            action: match value("ActionBar.Region").as_str() {
                "Left" => ActionRegion::Left,
                "Right" => ActionRegion::Right,
                _ => ActionRegion::None,
            },
            drag: value("TitleBar.Drag") == "true",
        }
    }
}

/// The band a surface that DRAWS a window (the designer) lays the title-bar regions out in, instead
/// of the host's (see [`set_design_chrome`]).
#[derive(Debug, Clone)]
pub struct DesignChrome {
    pub style: kubuno_desktop_controls::window_chrome::ChromeStyle,
    /// The whole window (the band is its top).
    pub bounds: Rect,
    pub has_icon: bool,
    pub buttons: kubuno_desktop_controls::window_chrome::SystemButtons,
}

thread_local! {
    static DESIGN_CHROME: std::cell::RefCell<Option<DesignChrome>> = const { std::cell::RefCell::new(None) };
    static DECLARED_SLOTS: std::cell::Cell<kubuno_desktop_controls::window_chrome::SlotWidths> =
        const { std::cell::Cell::new(kubuno_desktop_controls::window_chrome::SlotWidths { left: 0.0, center: 0.0, right: 0.0 }) };
}

/// The designer draws the window (its frame) itself: the title-bar regions of the view painted
/// next are laid out in `chrome`'s band, and nothing is declared to the host. `None` goes back to
/// the host's band.
pub fn set_design_chrome(chrome: Option<DesignChrome>) {
    DESIGN_CHROME.with(|d| *d.borrow_mut() = chrome);
    DECLARED_SLOTS.with(|s| s.set(Default::default()));
}

fn designing() -> bool {
    DESIGN_CHROME.with(|d| d.borrow().is_some())
}

/// The title band's layout for regions `widths` wide (declaring them): the designer's band, else
/// the host window's (`host::title_bar_layout`). `None` when the window has no Kubuno band.
pub fn title_bar_slots(widths: kubuno_desktop_controls::window_chrome::SlotWidths) -> Option<kubuno_desktop_controls::window_chrome::ChromeLayout> {
    DECLARED_SLOTS.with(|s| s.set(widths));
    match DESIGN_CHROME.with(|d| d.borrow().clone()) {
        Some(c) => Some(kubuno_desktop_controls::window_chrome::layout(&c.style, c.bounds, c.has_icon, c.buttons, widths)),
        None => host::title_bar_layout(widths),
    }
}

/// The region widths the view declared since the last [`set_design_chrome`] (the designer writes
/// the title between them).
pub fn declared_slots() -> kubuno_desktop_controls::window_chrome::SlotWidths {
    DECLARED_SLOTS.with(|s| s.get())
}

/// A control in the title band: it takes the pointer instead of dragging the window.
pub fn declare_title_bar_hole(rect: Rect) {
    if !designing() {
        host::add_title_bar_hole(rect);
    }
}

/// A `TitleBar.Drag` control: it drags the window.
pub fn declare_drag_area(rect: Rect) {
    if !designing() {
        host::add_title_bar_drag(rect);
    }
}

// ── The header's standard items (`ShowSearch`, `ShowNotifications`… on the view's root) ──────────────

/// The class of the header's standard cluster (bell and counter, settings, help, waffle, avatar), placed by its
/// element name: the framework never links it. An application that shows these items links the crate that
/// registers it (vskubuno `docs/SHELL-CONTROLS.md` §3, §5); without it, the items are reported and left out.
pub const HEADER_ACTIONS_CLASS: &str = "HeaderActions";

/// The `Show…` properties of the view's root that ask for the cluster, with the cluster's property of the same
/// name. In the web's order (bell, settings, help, waffle, avatar).
pub const HEADER_CLUSTER_PROPERTIES: [&str; 5] = ["ShowNotifications", "ShowSettings", "ShowHelp", "ShowWaffle", "ShowAccount"];

/// The view events the header's standard items raise, as the cluster names them (`OnNotificationsClicked`…).
pub const HEADER_CLUSTER_EVENTS: [&str; 3] = ["OnNotificationsClicked", "OnSettingsClicked", "OnHelpClicked"];

/// The side of a header button in the tall, 64 DIP header (`w-9 h-9`, 36 DIP circles), in a title bar of the usual
/// height (30: the caption buttons' size, `HeaderActions.Compact`), the space before the avatar (`ml-0.5`) and the
/// gap the cluster keeps before the caption buttons.
pub const HEADER_BUTTON: f32 = 36.0;
pub const HEADER_BUTTON_COMPACT: f32 = kubuno_desktop_controls::window_chrome::BUTTON;
pub const HEADER_AVATAR_GAP: f32 = 2.0;
pub const HEADER_TRAILING_GAP: f32 = 8.0;
/// From this title bar height on, the band is the web's tall header (`TitleBarHeight="64"`): 36 DIP buttons.
pub const TALL_HEADER_MIN: f32 = 56.0;

/// The header's standard items a view asks for on its root: `ShowSearch`, the cluster's `Show…` properties,
/// `UnreadCount` and the handlers of their events. Every one is off unless written (whatever the window kind: a
/// dialog, a tool window, a splash screen or a flyout gets them only when it asks), and a binding counts as "shown"
/// for the room it takes (the bound item still hides itself). They go at the end of the title bar's right region,
/// after the view's own `TitleBar.Region="Right"` controls, as the web's `HeaderActions` follows the module's
/// header slots; a window without a Kubuno title band (system chrome, borderless) does not show them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HeaderSpec {
    /// `ShowSearch`'s value (`"true"` or a binding); `None` when not shown.
    pub search: Option<String>,
    /// The cluster's `Show…` values, in [`HEADER_CLUSTER_PROPERTIES`] order (`None`: hidden).
    pub cluster: [Option<String>; 5],
    /// `UnreadCount` as written (a number or a binding).
    pub unread: Option<String>,
    /// `OnSearchClicked`'s handler.
    pub on_search: Option<String>,
    /// The cluster's event handlers, in [`HEADER_CLUSTER_EVENTS`] order.
    pub on_cluster: [Option<String>; 3],
    /// The title band's height (`TitleBarHeight`, else the window kind's band): the items are as tall as it, their
    /// buttons [`HEADER_BUTTON`] in a tall header, [`HEADER_BUTTON_COMPACT`] (the caption buttons' size) otherwise.
    pub band_height: f32,
    /// `RightToLeftLayout`: the cluster runs from the left, its avatar next to the caption buttons.
    pub right_to_left: bool,
    /// The band is coloured (`TitleBarBackground`, `AccentColor`, or a ribbon whose tab strip it continues): the items
    /// take the band's ink (`TitleBarForeground`, else `OnPrimary`) and the avatar its pale accent tint.
    pub band_ink: Option<String>,
}

impl HeaderSpec {
    /// The items `root` asks for (a value that is not literally `false` shows its item).
    pub fn read(root: &Element) -> Self {
        let raw = |name: &str| root.attribute(name).and_then(|a| a.value()).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let shown = |name: &str| raw(name).filter(|v| v != "false");
        Self {
            search: shown("ShowSearch"),
            cluster: HEADER_CLUSTER_PROPERTIES.map(shown),
            unread: raw("UnreadCount"),
            on_search: raw("OnSearchClicked"),
            on_cluster: HEADER_CLUSTER_EVENTS.map(raw),
            right_to_left: raw("RightToLeftLayout").as_deref() == Some("true"),
            band_ink: coloured_band(root).then(|| raw("TitleBarForeground").unwrap_or_else(|| "OnPrimary".into())),
            band_height: raw("TitleBarHeight").and_then(|v| v.parse::<f32>().ok()).filter(|v| *v > 0.0).unwrap_or_else(|| {
                let tool = raw("WindowKind").as_deref() == Some("ToolWindow") || raw("FormBorderStyle").is_some_and(|b| b.ends_with("ToolWindow"));
                if tool { kubuno_desktop_controls::window_chrome::TOOL_TITLEBAR_HEIGHT } else { title_bar_style(root).height() }
            }),
        }
    }

    /// Whether the band is the web's tall header (36 DIP buttons) rather than a title bar of the usual height.
    pub fn tall(&self) -> bool {
        self.band_height >= TALL_HEADER_MIN
    }

    /// The side of the header's buttons in this band.
    pub fn button(&self) -> f32 {
        if self.tall() { HEADER_BUTTON } else { HEADER_BUTTON_COMPACT.min(self.band_height) }
    }

    /// Whether the view shows any standard item.
    pub fn is_empty(&self) -> bool {
        self.search.is_none() && !self.has_cluster()
    }

    /// Whether the view shows one of the cluster's items (and needs [`HEADER_ACTIONS_CLASS`]).
    pub fn has_cluster(&self) -> bool {
        self.cluster.iter().any(Option::is_some)
    }

    /// The room the cluster takes: [`HeaderSpec::button`] per button, 2 more before the avatar and the gap before
    /// the caption buttons (the cluster's own width: 190 for the five in the tall header, 160 in a usual title bar).
    pub fn cluster_width(&self) -> f32 {
        let buttons = self.cluster.iter().filter(|v| v.is_some()).count() as f32;
        let account = self.cluster[4].is_some();
        buttons * self.button() + if account { HEADER_AVATAR_GAP } else { 0.0 } + HEADER_TRAILING_GAP
    }

    /// The search button's element: the web's magnifier (`w-9 h-9 rounded-full`, `Search size={18}`), an
    /// `IconButton` like the cluster's, whose click is the view's `SearchClicked`.
    pub fn search_xml(&self) -> Option<String> {
        let value = self.search.as_ref()?;
        let tip = if kubuno_desktop_resources::culture().starts_with("fr") { "Rechercher" } else { "Search" };
        let side = self.button();
        let glyph = if self.tall() { 18.0 } else { 16.0 };
        let mut xml = format!(r#"<IconButton x:Name="__kb_header_search" Icon="Search" ToolTip="{tip}" Diameter="{side}" Glyph="{glyph}" Width="{side}" Height="{side}""#);
        if value.starts_with('{') {
            xml.push_str(&format!(r#" Visible="{}""#, escape_attribute(value)));
        }
        if let Some(ink) = &self.band_ink {
            xml.push_str(&format!(r#" ForeColor="{}""#, escape_attribute(ink)));
        }
        if let Some(handler) = &self.on_search {
            xml.push_str(&format!(r#" OnClick="{}""#, escape_attribute(handler)));
        }
        xml.push_str("/>");
        Some(xml)
    }

    /// The cluster's element: `<HeaderActions>` with the view's `Show…`, `UnreadCount` and handlers when the class
    /// is registered; in the designer, a placeholder saying what is missing; `None` otherwise (nothing asked, or the
    /// class missing at run time — see [`header_class_missing_message`]).
    pub fn cluster_xml(&self, class_registered: bool, design: bool) -> Option<String> {
        if !self.has_cluster() {
            return None;
        }
        // As tall as the band: the cluster centres its buttons in it, right-aligned before the caption buttons.
        let size = format!(r#"Width="{}" Height="{}""#, self.cluster_width(), self.band_height);
        if !class_registered {
            return design.then(|| {
                format!(
                    r#"<{} {}="{HEADER_ACTIONS_CLASS}" {}="{}" {size}/>"#,
                    crate::tolerant::PLACEHOLDER_ELEMENT,
                    crate::tolerant::PLACEHOLDER_NAME_ATTRIBUTE,
                    crate::tolerant::PLACEHOLDER_REASON_ATTRIBUTE,
                    escape_attribute(&header_class_missing_message()),
                )
            });
        }
        let mut xml = format!("<{HEADER_ACTIONS_CLASS}");
        xml.push_str(if self.tall() { r#" Compact="false""# } else { r#" Compact="true""# });
        if self.right_to_left {
            xml.push_str(r#" RightToLeft="true""#);
        }
        if let Some(ink) = &self.band_ink {
            xml.push_str(&format!(r#" ForeColor="{}" AvatarTint="Accent""#, escape_attribute(ink)));
        }
        for (name, value) in HEADER_CLUSTER_PROPERTIES.iter().zip(&self.cluster) {
            xml.push_str(&format!(r#" {name}="{}""#, value.as_deref().map(escape_attribute).unwrap_or_else(|| "false".into())));
        }
        if let Some(unread) = &self.unread {
            xml.push_str(&format!(r#" UnreadCount="{}""#, escape_attribute(unread)));
        }
        for (event, handler) in HEADER_CLUSTER_EVENTS.iter().zip(&self.on_cluster) {
            if let Some(handler) = handler {
                xml.push_str(&format!(r#" {event}="{}""#, escape_attribute(handler)));
            }
        }
        xml.push_str(&format!(" {size}/>"));
        Some(xml)
    }
}

/// The elements that make a view's title band the web's tall header when they sit in it: the header's cluster and
/// its two menus (vskubuno `docs/SHELL-CONTROLS.md`).
pub const HEADER_MENU_CLASSES: [&str; 3] = [HEADER_ACTIONS_CLASS, "WaffleButton", "AccountButton"];

/// Whether the view hosts the header's menus in its title band: one of the header's standard items on its root
/// (`ShowSearch`, `ShowWaffle`, `ShowAccount`…), or a `HeaderActions` / `WaffleButton` / `AccountButton` in (or inside a
/// child placed in) a `TitleBar.Region`.
pub fn hosts_header_menus(root: &Element) -> bool {
    let shown = |name: &str| root.attribute(name).and_then(|a| a.value()).is_some_and(|v| !v.trim().is_empty() && v.trim() != "false");
    if shown("ShowSearch") || HEADER_CLUSTER_PROPERTIES.iter().any(|p| shown(p)) {
        return true;
    }
    root.syntax().children().filter_map(Element::cast).filter(|c| ChildPlace::read(c).title != TitleRegion::None).any(|c| {
        c.syntax().descendants().filter_map(Element::cast).any(|e| e.name().is_some_and(|n| HEADER_MENU_CLASSES.contains(&n.as_str())))
    })
}

/// The view's `TitleBarStyle`: as written (`Standard`, `Tall`), else `Tall` for a view hosting the header's menus
/// ([`hosts_header_menus`]: a main window with the waffle, the account avatar…) and `Standard` (32 DIP, Windows 11's)
/// for every other window. An explicit `TitleBarHeight` still wins over the style's height.
pub fn title_bar_style(root: &Element) -> kubuno_desktop_controls::window_chrome::TitleBarStyle {
    use kubuno_desktop_controls::window_chrome::TitleBarStyle;
    match root.attribute("TitleBarStyle").and_then(|a| a.value()).and_then(|v| TitleBarStyle::parse(&v)) {
        Some(style) => style,
        None if hosts_header_menus(root) => TitleBarStyle::Tall,
        None => TitleBarStyle::Standard,
    }
}

/// Whether the view's title band is coloured rather than neutral: its own `TitleBarBackground` or `AccentColor`, or a
/// `<Ribbon>` whose tab strip it continues (`TitleBarFollowsRibbon`, on by default — the Office-like title bar).
pub fn coloured_band(root: &Element) -> bool {
    let set = |name: &str| root.attribute(name).and_then(|a| a.value()).is_some_and(|v| !v.trim().is_empty());
    let follows_ribbon = root.attribute("TitleBarFollowsRibbon").and_then(|a| a.value()).is_none_or(|v| v.trim() != "false")
        && root.syntax().descendants().filter_map(Element::cast).any(|e| e.name().as_deref() == Some("Ribbon"));
    set("TitleBarBackground") || set("AccentColor") || follows_ribbon
}

/// Why the cluster is not shown: its class is not linked into the program.
pub fn header_class_missing_message() -> String {
    format!(
        "the title bar's ShowNotifications / ShowSettings / ShowHelp / ShowWaffle / ShowAccount need the `{HEADER_ACTIONS_CLASS}` user control: add the shared crate that provides it (the Kubuno shell controls) to the application's dependencies"
    )
}

/// Whether the header's cluster class is registered (an application linked it, or the language server found it).
pub fn header_class_registered() -> bool {
    crate::registry::lookup(&header_class_name()).is_some()
}

thread_local! {
    /// The cluster's class name, replaced by the tests (the registry is process-wide).
    static HEADER_CLASS: std::cell::RefCell<String> = std::cell::RefCell::new(HEADER_ACTIONS_CLASS.to_string());
}

/// The class the cluster is built from ([`HEADER_ACTIONS_CLASS`]; a test's own stand-in in this crate's tests).
pub(crate) fn header_class_name() -> String {
    HEADER_CLASS.with(|c| c.borrow().clone())
}

#[cfg(test)]
pub(crate) fn set_header_class_for_tests(name: &str) {
    HEADER_CLASS.with(|c| *c.borrow_mut() = name.to_string());
}

fn escape_attribute(value: &str) -> String {
    value.replace('"', "&quot;").replace('<', "&lt;")
}

/// The header's standard items of `root`, built: each node with its stable id (under
/// [`crate::design::HEADER_ITEM_PREFIX`]: not an element of the document, never selected in the designer) and its
/// size — the search button first, then the cluster. Empty when the view asks for none.
pub(crate) fn build_header_items(root: &Element, cx: &mut crate::props::BuildCx) -> Vec<(Box<dyn crate::node::ViewNode>, (f32, f32))> {
    let spec = HeaderSpec::read(root);
    if spec.is_empty() {
        return Vec::new();
    }
    let registered = header_class_registered();
    let design = crate::design::design_time();
    if spec.has_cluster() && !registered && !design {
        tracing::warn!("{}", header_class_missing_message());
    }
    let class = header_class_name();
    let cluster = spec.cluster_xml(registered, design).map(|xml| if class != HEADER_ACTIONS_CLASS { xml.replacen(&format!("<{HEADER_ACTIONS_CLASS} "), &format!("<{class} "), 1) } else { xml });
    let mut nodes = Vec::new();
    let sizes = [(spec.button(), spec.button()), (spec.cluster_width(), spec.band_height)];
    for ((part, xml), size) in [("search", spec.search_xml()), ("cluster", cluster)].into_iter().zip(sizes) {
        let Some(xml) = xml else { continue };
        // Inside a parent element: built as a child (not as a view's root, which would read the view's events).
        let parse = crate::syntax::parse(&format!("<Panel>{xml}</Panel>"));
        let Some(element) = crate::ast::Document::cast(parse.syntax()).and_then(|d| d.root_element()).and_then(|r| r.children().next()) else { continue };
        let id = format!("{}{part}", crate::design::HEADER_ITEM_PREFIX);
        match crate::compile::with_id_prefix(&id, || crate::compile::build_node(&element, cx, crate::registry::LayoutKind::DockAnchor)) {
            Ok(node) => nodes.push((node, size)),
            Err(e) => tracing::warn!("the title bar's {part} could not be built: {}", e.message),
        }
    }
    nodes
}

/// The view's `<ToolTip>` settings (WinForms' defaults when it has none).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToolTipSettings {
    pub initial_delay: u64,
    pub auto_pop_delay: u64,
    pub reshow_delay: u64,
    pub show_always: bool,
    pub active: bool,
}

impl Default for ToolTipSettings {
    fn default() -> Self {
        Self { initial_delay: 500, auto_pop_delay: 5000, reshow_delay: 100, show_always: false, active: true }
    }
}

impl ToolTipSettings {
    /// The first `<ToolTip>` of the view, else the defaults.
    pub fn read(root: &Element) -> Self {
        let Some(tip) = root.syntax().descendants().filter_map(Element::cast).find(|e| e.name().as_deref() == Some("ToolTip")) else {
            return Self::default();
        };
        let number = |name: &str, default: u64| {
            tip.attribute(name).and_then(|a| a.value()).and_then(|v| v.trim().parse::<f32>().ok()).map(|v| v.max(0.0) as u64).unwrap_or(default)
        };
        let flag = |name: &str, default: bool| tip.attribute(name).and_then(|a| a.value()).map(|v| v.trim() == "true").unwrap_or(default);
        let d = Self::default();
        Self {
            initial_delay: number("InitialDelay", d.initial_delay),
            auto_pop_delay: number("AutoPopDelay", d.auto_pop_delay),
            reshow_delay: number("ReshowDelay", d.reshow_delay),
            show_always: flag("ShowAlways", d.show_always),
            active: flag("Active", d.active),
        }
    }
}

/// One command of a menu (WinForms `ToolStripMenuItem`): a command, a separator (`<MenuSeparator>`,
/// `Text="-"` or `Kind="Separator"`), a section header (`<MenuHeader>`, `Kind="Header"`), or a
/// sub-menu (its item children).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MenuItemSpec {
    /// The `<MenuItem>`'s stable id (the sender of its Click).
    pub id: String,
    pub name: Option<String>,
    /// Its text, its `&` mnemonic included.
    pub text: String,
    pub enabled: bool,
    pub checked: bool,
    /// Shown (`Visible`).
    pub visible: bool,
    /// `ShortcutKeys`: the key that runs it while its menu is closed (`Ctrl+S`).
    pub shortcut: String,
    /// The Lucide glyph of its `Icon`.
    pub icon: String,
    /// A destructive command (the danger colour).
    pub danger: bool,
    /// `CheckOnClick`: choosing it toggles its check mark.
    pub check_on_click: bool,
    /// `RadioGroup`: choosing it checks it and unchecks the others of the group.
    pub radio_group: String,
    /// `Kind="Header"`: an inert section title.
    pub header: bool,
    /// Its `OnClick` handler.
    pub handler: Option<String>,
    /// Its `OnCheckedChanged` handler.
    pub on_checked_changed: Option<String>,
    /// Its `OnDropDownOpening` handler (a sub-menu about to open: fill it now).
    pub on_drop_down_opening: Option<String>,
    /// The bound properties (`Text`, `Enabled`, `Checked`, `Visible`, `ToolTip`), resolved when the
    /// menu opens.
    pub bindings: Vec<(&'static str, crate::binding::BindingSpec)>,
    /// Its sub-menu.
    pub children: Vec<MenuItemSpec>,
    /// Items read from a list when the sub-menu opens (`ItemsSource`).
    pub items_source: Option<crate::binding::BindingSpec>,
    /// An item made from a row of an `ItemsSource`: its key (the row's `Key`, else its text).
    pub row_key: Option<String>,
    /// `ToolTip`: shown when the pointer rests on the command.
    pub tooltip: String,
    /// `ShortcutKeyDisplayString`: the shortcut text shown instead of `ShortcutKeys`.
    pub shortcut_display: String,
    /// `ShowShortcutKeys="false"`: no shortcut text (the shortcut still works).
    pub hide_shortcut: bool,
    /// `Command`: the `<Command x:Name>` it runs and takes its text, icon, shortcut and state from.
    pub command: Option<String>,
    /// A `Checked` attribute is written (else a command's checked state shows).
    pub checked_written: bool,
    /// `<MenuItem.ItemTemplate>`: the item each row of its `ItemsSource` is made from.
    pub item_template: Option<Box<MenuItemSpec>>,
}

impl MenuItemSpec {
    /// A `Text` of a single dash (WinForms), `Kind="Separator"` or `<MenuSeparator>`: a separator line.
    pub fn is_separator(&self) -> bool {
        self.text.trim() == "-"
    }

    /// Whether choosing it does something (not a separator, not a header).
    pub fn is_command(&self) -> bool {
        !self.is_separator() && !self.header
    }

    /// The text shown in the shortcut column: `ShortcutKeyDisplayString`, else `ShortcutKeys`; empty
    /// with `ShowShortcutKeys="false"`.
    pub fn shortcut_text(&self) -> &str {
        if self.hide_shortcut {
            ""
        } else if !self.shortcut_display.is_empty() {
            &self.shortcut_display
        } else {
            &self.shortcut
        }
    }

    /// The item with its bindings read from `vm`, its `Command`'s state applied and its
    /// `ItemsSource` expanded.
    fn resolved(&self, vm: &dyn ViewModel, checked: &std::collections::HashMap<String, bool>, commands: &[crate::menus::CommandSpec]) -> Self {
        let mut item = self.clone();
        for (property, spec) in &self.bindings {
            // `{Res key}` reads the resource (vskubuno docs/RESOURCES.md), `{Binding P}` the view model.
            let Some(value) = crate::resources::get(vm, spec) else { continue };
            match (*property, value) {
                ("Text", v) => item.text = crate::binding::Row::new().with("v", v).text("v"),
                ("ToolTip", v) => item.tooltip = crate::binding::Row::new().with("v", v).text("v"),
                ("Enabled", crate::binding::Value::Bool(b)) => item.enabled = b,
                ("Checked", crate::binding::Value::Bool(b)) => item.checked = b,
                ("Visible", crate::binding::Value::Bool(b)) => item.visible = b,
                _ => {}
            }
        }
        if let Some(state) = checked.get(&self.id) {
            if !self.bindings.iter().any(|(p, _)| *p == "Checked") {
                item.checked = *state;
            }
        }
        // The command it runs: what the item does not write itself comes from it (RIBBON.md §4).
        if let Some(cmd) = self.command.as_deref().and_then(|n| commands.iter().find(|c| c.name == n)) {
            let cmd = cmd.resolved(vm, checked);
            if item.text.is_empty() {
                item.text = cmd.label.clone();
            }
            if item.icon.is_empty() {
                item.icon = cmd.icon.clone();
            }
            if item.shortcut.is_empty() {
                item.shortcut = cmd.shortcut.clone();
            }
            if item.tooltip.is_empty() {
                item.tooltip = cmd.tooltip.clone();
            }
            item.enabled &= cmd.enabled;
            if !item.checked_written && !self.bindings.iter().any(|(p, _)| *p == "Checked") {
                item.checked = cmd.checked;
            }
        }
        let mut children: Vec<MenuItemSpec> = self.children.iter().map(|c| c.resolved(vm, checked, commands)).collect();
        if let Some(spec) = &self.items_source {
            children.extend(items_from_list(&self.id, vm, spec, checked, self.item_template.as_deref()));
        }
        item.children = children.into_iter().filter(|c| c.visible).collect();
        item
    }
}

/// The items of a menu's `ItemsSource` list (fields `Text`, `Key`, `Icon`, `ShortcutKeys`,
/// `Checked`, `Enabled`, `Danger`, and `Kind` = `Separator` / `Header`), or made from its
/// `ItemTemplate` when it has one.
fn items_from_list(
    parent_id: &str,
    vm: &dyn ViewModel,
    spec: &crate::binding::BindingSpec,
    checked: &std::collections::HashMap<String, bool>,
    template: Option<&MenuItemSpec>,
) -> Vec<MenuItemSpec> {
    let Some(crate::binding::Value::List(rows)) = vm.get(&spec.path) else { return Vec::new() };
    rows.iter()
        .enumerate()
        .map(|(i, r)| {
            let id = format!("{parent_id}/row{i}");
            if let Some(t) = template {
                return from_template(t, r, id, checked);
            }
            let text = r.text("Text");
            let key = Some(r.text("Key")).filter(|k| !k.is_empty()).unwrap_or_else(|| text.clone());
            let kind = r.text("Kind");
            MenuItemSpec {
                checked: checked.get(&id).copied().unwrap_or(r.text("Checked") == "true"),
                id,
                name: None,
                text: if kind == "Separator" { "-".to_string() } else { text },
                enabled: r.text("Enabled") != "false",
                visible: true,
                shortcut: r.text("ShortcutKeys"),
                icon: crate::icon::resolve(&r.text("Icon")).map(str::to_string).unwrap_or_default(),
                danger: r.text("Danger") == "true",
                header: kind == "Header",
                row_key: Some(key),
                tooltip: r.text("ToolTip"),
                ..MenuItemSpec::default()
            }
        })
        .collect()
}

/// The item of row `r` made from a `<MenuItem.ItemTemplate>` (WPF's `ItemTemplate` for a bound
/// menu): its literal attributes copied, each `{Binding Field}` read from the row; its key is the
/// row's `Key`, else its text.
fn from_template(t: &MenuItemSpec, r: &crate::binding::Row, id: String, checked: &std::collections::HashMap<String, bool>) -> MenuItemSpec {
    let mut item = MenuItemSpec { id, bindings: Vec::new(), children: Vec::new(), item_template: None, items_source: None, ..t.clone() };
    for (property, spec) in &t.bindings {
        let v = r.text(&spec.path);
        match *property {
            "Text" => item.text = v,
            "ToolTip" => item.tooltip = v,
            "Enabled" => item.enabled = v != "false",
            "Visible" => item.visible = v != "false",
            "Checked" => item.checked = v == "true",
            _ => {}
        }
    }
    if let Some(state) = checked.get(&item.id) {
        item.checked = *state;
    }
    let key = r.text("Key");
    item.row_key = Some(if key.is_empty() { crate::common::mnemonic(&item.text).0 } else { key });
    item
}

/// A menu of the view: a `<ContextMenu x:Name="…">`, a menu of a `<MenuBar>` or the items of a
/// drop-down button.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MenuSpec {
    pub name: String,
    pub id: String,
    pub items: Vec<MenuItemSpec>,
    /// Its `OnOpening` handler.
    pub opening: Option<String>,
    /// `OwnerDraw`: its items are drawn by its `OnDrawItem` handler (EVT-8).
    pub owner_draw: bool,
    pub draw_item: Option<String>,
    /// Items read from a list when it opens (`ItemsSource`).
    pub items_source: Option<crate::binding::BindingSpec>,
    /// Its `OnItemClicked` handler: an item made from a row of `ItemsSource` was chosen.
    pub on_item_clicked: Option<String>,
    /// Its `OnClosed` handler.
    pub on_closed: Option<String>,
    /// The element that raises its `OnOpening` / `OnClosed`: `"ContextMenu"` (the default), the
    /// top-level `"MenuItem"` of a menu bar's menu, the button of a drop-down button; with its `x:Name`.
    pub sender: Option<(&'static str, Option<String>)>,
    /// The view's `<Command>`s, which its items name in their `Command`.
    pub commands: std::rc::Rc<Vec<crate::menus::CommandSpec>>,
    /// The item template of a menu bar's top-level item (its `ItemsSource` rows).
    pub item_template: Option<Box<MenuItemSpec>>,
}

impl MenuSpec {
    /// The menu as it opens: bindings read from `vm`, `ItemsSource` rows added, hidden items
    /// dropped; `checked` holds the check marks the user changed (`CheckOnClick`, `RadioGroup`).
    pub fn resolved(&self, vm: &dyn ViewModel, checked: &std::collections::HashMap<String, bool>) -> Self {
        let mut menu = self.clone();
        let mut items: Vec<MenuItemSpec> = self.items.iter().map(|i| i.resolved(vm, checked, &self.commands)).collect();
        if let Some(spec) = &self.items_source {
            items.extend(items_from_list(&self.id, vm, spec, checked, self.item_template.as_deref()));
        }
        menu.items = items.into_iter().filter(|i| i.visible).collect();
        menu
    }

    /// The element raising its `OnOpening` / `OnClosed`, and its name.
    pub fn sender_element(&self) -> (&'static str, Option<&str>) {
        match &self.sender {
            Some((element, name)) => (element, name.as_deref()),
            None => ("ContextMenu", Some(self.name.as_str())),
        }
    }
}

/// The element names a menu holds (`MENUS.md` §3): commands, separators and section titles.
pub const MENU_CHILDREN: &[&str] = &["MenuItem", "MenuSeparator", "MenuHeader"];

/// The items of a menu element (a `<ContextMenu>`, a `<MenuItem>`'s sub-menu, a drop-down button).
pub(crate) fn read_items(parent: &Element) -> Vec<MenuItemSpec> {
    parent.children().filter(|c| c.name().is_some_and(|n| MENU_CHILDREN.contains(&n.as_str()))).map(|c| read_item(&c)).collect()
}

/// One item of a menu: a `<MenuItem>` (a command, a separator or a header by its `Kind`), a
/// `<MenuSeparator>` or a `<MenuHeader>`.
pub(crate) fn read_item(item: &Element) -> MenuItemSpec {
    let attr = |e: &Element, name: &str| e.attribute(name).and_then(|a| a.value());
    let element = item.name().unwrap_or_default();
    let mut bindings = Vec::new();
    let mut literal = |name: &'static str| -> Option<String> {
        let raw = attr(item, name)?;
        if crate::binding::is_binding_expr(&raw) {
            if let Some(spec) = crate::binding::parse_binding(&raw) {
                bindings.push((name, spec));
            }
            return None;
        }
        Some(raw)
    };
    let text = literal("Text").unwrap_or_default();
    let enabled = literal("Enabled").is_none_or(|v| v.trim() != "false");
    let checked = literal("Checked").is_some_and(|v| v.trim() == "true");
    let visible = literal("Visible").is_none_or(|v| v.trim() != "false");
    let tooltip = literal("ToolTip").unwrap_or_default();
    let kind = match element.as_str() {
        "MenuSeparator" => "Separator".to_string(),
        "MenuHeader" => "Header".to_string(),
        _ => attr(item, "Kind").unwrap_or_default(),
    };
    MenuItemSpec {
        id: item.stable_id(),
        name: attr(item, "x:Name"),
        text: if kind == "Separator" { "-".to_string() } else { text },
        enabled,
        checked,
        visible,
        shortcut: attr(item, "ShortcutKeys").unwrap_or_default(),
        icon: crate::icon::attribute(item, "Icon").map(str::to_string).unwrap_or_default(),
        danger: attr(item, "Danger").is_some_and(|v| v.trim() == "true"),
        check_on_click: attr(item, "CheckOnClick").is_some_and(|v| v.trim() == "true"),
        radio_group: attr(item, "RadioGroup").unwrap_or_default(),
        header: kind == "Header",
        handler: attr(item, "OnClick").filter(|h| !h.is_empty()),
        on_checked_changed: attr(item, "OnCheckedChanged").filter(|h| !h.is_empty()),
        on_drop_down_opening: attr(item, "OnDropDownOpening").filter(|h| !h.is_empty()),
        bindings,
        children: read_items(item),
        items_source: attr(item, "ItemsSource").filter(|v| crate::binding::is_binding_expr(v)).and_then(|v| crate::binding::parse_binding(&v)),
        row_key: None,
        tooltip,
        shortcut_display: attr(item, "ShortcutKeyDisplayString").unwrap_or_default(),
        hide_shortcut: attr(item, "ShowShortcutKeys").is_some_and(|v| v.trim() == "false"),
        command: attr(item, "Command").map(|c| crate::menus::reference_name(&c)).filter(|c| !c.is_empty()),
        checked_written: item.attribute("Checked").is_some(),
        item_template: item_template_of(item),
    }
}

/// `<MenuItem.ItemTemplate><MenuItem …/></MenuItem.ItemTemplate>` (also on `<ContextMenu>`): the item
/// each row of the `ItemsSource` is made from.
fn item_template_of(owner: &Element) -> Option<Box<MenuItemSpec>> {
    owner
        .children()
        .find(|c| c.name().is_some_and(|n| n.ends_with(".ItemTemplate")))
        .and_then(|t| t.children().find(|c| c.name().as_deref() == Some("MenuItem")))
        .map(|t| Box::new(read_item(&t)))
}

/// Every menu of the view: its `<ContextMenu x:Name>`s, the menus of its menu bars (one per
/// top-level item, named [`crate::menus::bar_menu_name`]) and of its drop-down and split buttons that
/// hold their items ([`crate::menus::drop_down_menu_name`]). Every menu opens, and its shortcuts run,
/// the same way.
pub fn read_menus(root: &Element) -> Vec<MenuSpec> {
    let attr = |e: &Element, name: &str| e.attribute(name).and_then(|a| a.value());
    let commands = std::rc::Rc::new(crate::menus::read_commands(root));
    let mut out = Vec::new();
    for e in root.syntax().descendants().filter_map(Element::cast) {
        match e.name().as_deref() {
            Some("ContextMenu") => {
                let Some(name) = attr(&e, "x:Name").filter(|n| !n.is_empty()) else { continue };
                out.push(MenuSpec {
                    id: e.stable_id(),
                    name,
                    items: read_items(&e),
                    opening: attr(&e, "OnOpening").filter(|h| !h.is_empty()),
                    owner_draw: attr(&e, "OwnerDraw").is_some_and(|v| v.trim() == "true"),
                    draw_item: attr(&e, "OnDrawItem").filter(|h| !h.is_empty()),
                    items_source: attr(&e, "ItemsSource").filter(|v| crate::binding::is_binding_expr(v)).and_then(|v| crate::binding::parse_binding(&v)),
                    on_item_clicked: attr(&e, "OnItemClicked").filter(|h| !h.is_empty()),
                    on_closed: attr(&e, "OnClosed").filter(|h| !h.is_empty()),
                    sender: None,
                    commands: commands.clone(),
                    item_template: item_template_of(&e),
                });
            }
            Some("MenuBar") => {
                for top in e.children().filter(|c| c.name().as_deref() == Some("MenuItem")) {
                    let item = read_item(&top);
                    out.push(MenuSpec {
                        name: crate::menus::bar_menu_name(&item.id),
                        id: item.id.clone(),
                        opening: item.on_drop_down_opening.clone(),
                        items_source: item.items_source.clone(),
                        sender: Some(("MenuItem", item.name.clone())),
                        item_template: item.item_template.clone(),
                        items: item.children,
                        commands: commands.clone(),
                        ..MenuSpec::default()
                    });
                }
            }
            Some(button @ ("DropDownButton" | "SplitButton")) => {
                let items = read_items(&e);
                if items.is_empty() {
                    continue;
                }
                let id = e.stable_id();
                let element: &'static str = if button == "SplitButton" { "SplitButton" } else { "DropDownButton" };
                out.push(MenuSpec {
                    name: crate::menus::drop_down_menu_name(&id),
                    id,
                    items,
                    opening: attr(&e, "OnDropDownOpening").filter(|h| !h.is_empty()),
                    on_closed: attr(&e, "OnDropDownClosed").filter(|h| !h.is_empty()),
                    sender: Some((element, attr(&e, "x:Name"))),
                    commands: commands.clone(),
                    ..MenuSpec::default()
                });
            }
            _ => {}
        }
    }
    out
}

/// Where a menu opened from code ([`show_context_menu`]) appears.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuAnchor {
    /// At a point of the window's client area (DIP).
    Point(f32, f32),
    /// Below the element of this `x:Name` (at the pointer when it is not in view).
    Element(String),
    /// Below this rectangle of the client area (DIP): a drop-down button's.
    Below(Rect),
}

/// A menu to open at the next frame of the view that declares it.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuRequest {
    /// The menu's name ([`MenuSpec::name`]).
    pub name: String,
    pub anchor: MenuAnchor,
    /// Opened from the keyboard: its first command is hot and its mnemonics are underlined.
    pub keyboard: bool,
    /// A menu of a menu bar: the bar's element id and the index of its top-level item.
    pub bar: Option<(String, usize)>,
    /// The element it was opened from (stable id), the sender of its items' Click (`""`: none).
    pub owner: String,
}

impl MenuRequest {
    pub fn new(name: impl Into<String>, anchor: MenuAnchor) -> Self {
        Self { name: name.into(), anchor, keyboard: false, bar: None, owner: String::new() }
    }
}

thread_local! {
    static MENU_REQUESTS: std::cell::RefCell<Vec<MenuRequest>> = const { std::cell::RefCell::new(Vec::new()) };
    static OPEN_MENU: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The menu open now (set by the runtime after each frame's input).
pub(crate) fn set_open_menu(name: Option<String>) {
    OPEN_MENU.with(|m| *m.borrow_mut() = name);
}

/// Whether the menu `name` is open (a drop-down button shows itself pressed).
pub fn is_menu_open(name: &str) -> bool {
    OPEN_MENU.with(|m| m.borrow().as_deref() == Some(name))
}

/// Opens the `<ContextMenu x:Name="menu">` of the view at `anchor` (WinForms
/// `ContextMenuStrip.Show`), at the next frame of the view that declares it.
pub fn show_context_menu(menu: &str, anchor: MenuAnchor) {
    request_menu(MenuRequest::new(menu, anchor));
}

/// Opens a menu of the view at the next frame (see [`MenuRequest`]).
pub fn request_menu(request: MenuRequest) {
    MENU_REQUESTS.with(|r| r.borrow_mut().push(request));
    host::request_repaint_after(0);
}

/// Takes the pending request for one of `names` (the menus of the view asking).
pub(crate) fn take_menu_request(names: &[&str]) -> Option<MenuRequest> {
    MENU_REQUESTS.with(|r| {
        let mut r = r.borrow_mut();
        let i = r.iter().position(|m| names.contains(&m.name.as_str()))?;
        Some(r.remove(i))
    })
}


/// The tooltip under the pointer (WinForms' timing: initial delay, auto-pop, reshow).
#[derive(Default)]
pub struct TooltipState {
    /// The element whose tooltip is pending or shown, and since when the pointer rests on it.
    element: Option<String>,
    since: u64,
    /// When the last tooltip went away (a quick move to the next control reshows sooner).
    hidden_at: Option<u64>,
    shown: bool,
}

/// What [`TooltipState::update`] decides for this frame.
#[derive(Debug, Clone, PartialEq)]
pub enum TooltipShow {
    /// Nothing to show; repaint in this many ms to check again (a pending tooltip).
    Wait(Option<u64>),
    /// Show `text` below the pointer; repaint in this many ms to hide it (auto-pop).
    Show { text: String, hide_in: Option<u64> },
}

impl TooltipState {
    /// The frame's candidate (`Some((element, text))` for the deepest hovered element with a
    /// tooltip) at `now`, with `settings` and whether the window is active.
    pub fn update(&mut self, candidate: Option<(&str, &str)>, now: u64, settings: &ToolTipSettings, window_active: bool) -> TooltipShow {
        let usable = settings.active && (window_active || settings.show_always);
        let Some((element, text)) = candidate.filter(|_| usable) else {
            if self.shown {
                self.hidden_at = Some(now);
            }
            self.element = None;
            self.shown = false;
            return TooltipShow::Wait(None);
        };
        if self.element.as_deref() != Some(element) {
            if self.shown {
                self.hidden_at = Some(now);
            }
            self.element = Some(element.to_string());
            self.since = now;
            self.shown = false;
        }
        let quick = self.hidden_at.is_some_and(|h| now.saturating_sub(h) <= settings.initial_delay);
        let delay = if quick { settings.reshow_delay } else { settings.initial_delay };
        let rested = now.saturating_sub(self.since);
        if rested < delay {
            return TooltipShow::Wait(Some(delay - rested));
        }
        let shown_for = rested - delay;
        if settings.auto_pop_delay > 0 && shown_for >= settings.auto_pop_delay {
            if self.shown {
                self.hidden_at = Some(now);
            }
            self.shown = false;
            return TooltipShow::Wait(None);
        }
        self.shown = true;
        let hide_in = (settings.auto_pop_delay > 0).then(|| settings.auto_pop_delay - shown_for);
        TooltipShow::Show { text: text.to_string(), hide_in }
    }

    /// A button went down: the tooltip hides until the pointer rests on another control.
    pub fn press(&mut self, now: u64) {
        if self.shown {
            self.hidden_at = Some(now);
        }
        self.shown = false;
        self.since = u64::MAX / 2;
    }
}

/// Paints the tooltip `text` below the pointer, in a floating surface above everything.
pub fn paint_tooltip(c: &dyn kubuno_desktop_controls::ControlCanvas, frame: &host::Frame, text: &str) {
    let tip = kubuno_desktop_ui::display::Tooltip::new(text);
    let placed = tip.place_at_pointer(c, frame.mouse, frame.screen_area());
    const MARGIN: f32 = 12.0;
    let r = placed.rect;
    let bounds = Rect::new(r.left - MARGIN, r.top - MARGIN, r.right + MARGIN, r.bottom + MARGIN);
    let local = Rect::new(MARGIN, MARGIN, MARGIN + (r.right - r.left), MARGIN + (r.bottom - r.top));
    host::overlay(bounds, move |canvas| tip.paint(canvas, local, WidgetState::REST));
}

/// Paints the tooltip `text` below `at` (client DIP), in a floating surface above everything (a menu
/// command's `ToolTip`).
pub fn paint_tooltip_at(c: &dyn kubuno_desktop_controls::ControlCanvas, frame: &host::Frame, text: &str, at: (f32, f32)) {
    let tip = kubuno_desktop_ui::display::Tooltip::new(text);
    let placed = tip.place_at_pointer(c, at, frame.screen_area());
    const MARGIN: f32 = 12.0;
    let r = placed.rect;
    let bounds = Rect::new(r.left - MARGIN, r.top - MARGIN, r.right + MARGIN, r.bottom + MARGIN);
    let local = Rect::new(MARGIN, MARGIN, MARGIN + (r.right - r.left), MARGIN + (r.bottom - r.top));
    host::overlay(bounds, move |canvas| tip.paint(canvas, local, WidgetState::REST));
}

/// The nodes of an open menu, the ids of its rows with their paths, and the focused row ([`menu_access_nodes`]).
pub type MenuAccess = (Vec<host::access::AccessNode>, Vec<(u64, Vec<usize>)>, Option<u64>);

/// What assistive technology is told of an open menu: a `Menu` node per open level, a `MenuItem`
/// node per row (its name without the `&`, its mnemonic as access key, checked when it can be,
/// expanded when it opens a sub-menu), with the ids of the rows and their paths, and the hot row of
/// the deepest level (where the focus is).
pub fn menu_access_nodes(menu: &OpenMenu) -> MenuAccess {
    use host::access::{AccessNode, AccessRole};
    let mut nodes = Vec::new();
    let mut rows = Vec::new();
    let mut hot = None;
    let key = |path: &[usize]| format!("\u{1}menu:{}:{}", menu.spec.name, path.iter().map(usize::to_string).collect::<Vec<_>>().join("."));
    for (depth, level) in menu.levels.iter().enumerate() {
        let Some(panel) = level.panel else { continue };
        let Some(items) = menu.spec.items_at(&level.path) else { continue };
        let level_id = crate::common::access_id(&format!("{}#level", key(&level.path)));
        // A sub-menu hangs under the row that opened it.
        let parent = (depth > 0).then(|| crate::common::access_id(&key(&level.path)));
        nodes.push(AccessNode { id: level_id, parent, role: AccessRole::Menu, name: String::new(), bounds: (panel.left, panel.top, panel.right, panel.bottom), read_only: true, ..Default::default() });
        for (i, item) in items.iter().enumerate() {
            let mut path = level.path.clone();
            path.push(i);
            let id = crate::common::access_id(&key(&path));
            let r = level.menu.item_rect(panel, i).unwrap_or(panel);
            let (name, mnemonic) = crate::common::mnemonic(&item.text);
            let (role, name) = if item.is_separator() { (AccessRole::Separator, String::new()) } else if item.header { (AccessRole::Label, name) } else { (AccessRole::MenuItem, name) };
            let sub = !item.children.is_empty() || item.items_source.is_some() || item.on_drop_down_opening.is_some();
            let checkable = item.check_on_click || !item.radio_group.is_empty() || item.checked;
            nodes.push(AccessNode {
                id,
                parent: Some(level_id),
                role,
                name,
                description: item.tooltip.clone(),
                bounds: (r.left, r.top, r.right, r.bottom),
                disabled: !item.enabled,
                checked: (checkable && item.is_command()).then_some(item.checked),
                clickable: item.enabled && item.is_command(),
                read_only: true,
                access_key: mnemonic.map(|(k, _)| k.to_ascii_uppercase().to_string()),
                expanded: sub.then(|| menu.levels.get(depth + 1).is_some_and(|l| l.path == path)),
                ..Default::default()
            });
            if item.is_command() {
                rows.push((id, path));
            }
            if depth + 1 == menu.levels.len() && level.menu.hot_index == Some(i) {
                hot = Some(id);
            }
        }
    }
    (nodes, rows, hot)
}

/// One open level of a context menu: the root menu, or a sub-menu opened from a row of the level
/// before it. Each level is its own [`Menu`] (never with a sub-menu open inside it) and its own
/// floating surface, so sub-menus nest to any depth and no surface covers more than its card.
pub struct MenuLevel {
    /// The rows of this level.
    pub menu: Menu,
    /// The path of the item whose sub-menu this level is (rows of each level, from the root); empty
    /// for the root.
    pub path: Vec<usize>,
    /// Its panel last frame (client DIP), for the pointer.
    pub panel: Option<Rect>,
}

/// What the pointer or the keyboard did to an open menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    /// Nothing that concerns the caller.
    None,
    /// The menu closes (Escape, a click outside it).
    Close,
    /// The item at this path was chosen.
    Chosen(Vec<usize>),
    /// The sub-menu of the item at this path just opened (its `OnDropDownOpening` runs now).
    Opened(Vec<usize>),
    /// A menu of a menu bar: Left on its first level (`false`), or Right on a command without a
    /// sub-menu (`true`), moves to the bar's previous or next menu.
    Neighbour(bool),
}

/// An open context menu.
pub struct OpenMenu {
    /// The menu as the view declares it (read again when a sub-menu opens: `OnDropDownOpening`).
    pub source: MenuSpec,
    /// The menu as it opened (bindings read, list items added): what the rows are.
    pub spec: MenuSpec,
    /// The open levels, the root first.
    pub levels: Vec<MenuLevel>,
    /// Where it opened (client DIP).
    pub at: (f32, f32),
    /// The element it was opened on (stable id): the sender of its items' Click.
    pub owner: String,
    /// Opened from the keyboard: the mnemonics are underlined (WinForms' keyboard cues).
    pub keyboard: bool,
    /// A menu of a menu bar: the bar's element id and the index of its top-level item.
    pub bar: Option<(String, usize)>,
    /// The command the pointer rests on, and since when (ms): its `ToolTip` shows after a while.
    pub rest: Option<(Vec<usize>, u64)>,
}

/// The strip row of one menu item (a sub-menu's rows under it, for its chevron and its size).
fn strip_of(item: &MenuItemSpec) -> StripItem {
    if item.is_separator() {
        return kubuno_desktop_ui::lists::separator();
    }
    let (text, _) = crate::common::mnemonic(&item.text);
    if item.header {
        return kubuno_desktop_ui::lists::section(text);
    }
    let mut e = MenuEntry::new(text).checked(item.checked).enabled(item.enabled);
    let shortcut = item.shortcut_text();
    if !shortcut.is_empty() {
        e = e.shortcut_text(shortcut.to_string());
    }
    if !item.icon.is_empty() {
        e = e.icon(item.icon.clone());
    }
    if item.danger {
        e = e.danger();
    }
    if !item.children.is_empty() {
        e = e.submenu(item.children.iter().map(strip_of).collect());
    } else if item.items_source.is_some() || item.on_drop_down_opening.is_some() {
        // A sub-menu filled when it opens (`OnDropDownOpening`, `ItemsSource`): a sub-menu even while
        // it is empty, so that it can open; a dimmed row stands for its items until then.
        e = e.submenu(vec![MenuEntry::new("…").enabled(false).build()]);
    }
    e.build()
}

/// The rows of one level: its items, or the dimmed placeholder of a sub-menu still empty (filled
/// when it opens).
fn level_rows(items: &[MenuItemSpec]) -> Vec<StripItem> {
    if items.is_empty() {
        return vec![MenuEntry::new("…").enabled(false).build()];
    }
    items.iter().map(strip_of).collect()
}

impl MenuSpec {
    /// The items shown by the level at `path` (the root's items for an empty path).
    pub fn items_at(&self, path: &[usize]) -> Option<&[MenuItemSpec]> {
        let mut items: &[MenuItemSpec] = &self.items;
        for &i in path {
            items = &items.get(i)?.children;
        }
        Some(items)
    }

    /// The item at `path`.
    pub fn item_at(&self, path: &[usize]) -> Option<&MenuItemSpec> {
        let (last, parent) = path.split_last()?;
        self.items_at(parent)?.get(*last)
    }

    /// The item at `path`, to replace it.
    pub fn item_at_mut(&mut self, path: &[usize]) -> Option<&mut MenuItemSpec> {
        let (first, rest) = path.split_first()?;
        let mut item = self.items.get_mut(*first)?;
        for &i in rest {
            item = item.children.get_mut(i)?;
        }
        Some(item)
    }

    /// The declared item of id `id`, anywhere in the menu.
    pub fn find(&self, id: &str) -> Option<&MenuItemSpec> {
        fn walk<'a>(items: &'a [MenuItemSpec], id: &str) -> Option<&'a MenuItemSpec> {
            items.iter().find_map(|i| if i.id == id { Some(i) } else { walk(&i.children, id) })
        }
        walk(&self.items, id)
    }
}

impl OpenMenu {
    /// The menu `spec` (already resolved: [`MenuSpec::resolved`]) of `source`, opened at `at` on
    /// `owner`.
    pub fn new(source: MenuSpec, spec: MenuSpec, at: (f32, f32), owner: String) -> Self {
        let mut menu = Menu::with_items(spec.items.iter().map(strip_of).collect());
        menu.owner_draw = spec.owner_draw && spec.draw_item.is_some();
        Self { source, spec, levels: vec![MenuLevel { menu, path: Vec::new(), panel: None }], at, owner, keyboard: false, bar: None, rest: None }
    }

    /// Assistive technology pressed (or expanded) the row at `path`: its sub-menu opens, else it is
    /// chosen.
    pub fn access_click(&mut self, path: &[usize]) -> MenuAction {
        let Some((&row, parent)) = path.split_last() else { return MenuAction::None };
        let Some(level) = self.levels.iter().position(|l| l.path == parent) else { return MenuAction::None };
        if self.has_submenu(level, row) {
            if self.levels.get(level + 1).is_some_and(|l| l.path == path) {
                // Expanded already: collapse it.
                self.levels.truncate(level + 1);
                return MenuAction::None;
            }
            return self.open(level, row).map(MenuAction::Opened).unwrap_or(MenuAction::None);
        }
        MenuAction::Chosen(path.to_vec())
    }

    /// Makes the first command of the deepest level hot (a menu opened from the keyboard).
    pub fn hot_first(&mut self) {
        if let Some(level) = self.levels.last_mut() {
            level.menu.hot_index = (0..level.menu.items().len()).find(|&j| level.menu.is_actionable(j));
        }
    }

    /// The path of the hot row of the deepest level.
    pub fn hot_path(&self) -> Option<Vec<usize>> {
        let level = self.levels.last()?;
        let mut path = level.path.clone();
        path.push(level.menu.hot_index?);
        Some(path)
    }

    /// A letter typed while the menu is open (WinForms): the command of the deepest level whose
    /// mnemonic it is — else whose text starts with it — is chosen (its sub-menu opens) when it is the
    /// only one; with several, the next one becomes hot.
    pub fn mnemonic(&mut self, key: char) -> MenuAction {
        let key = key.to_lowercase().next().unwrap_or(key);
        let depth = self.levels.len() - 1;
        let path = self.levels[depth].path.clone();
        let Some(items) = self.spec.items_at(&path) else { return MenuAction::None };
        let menu = &self.levels[depth].menu;
        let actionable = |i: usize| menu.is_actionable(i);
        let mut matches: Vec<usize> = items.iter().enumerate().filter(|(i, it)| actionable(*i) && crate::menus::mnemonic_of(&it.text) == Some(key)).map(|(i, _)| i).collect();
        if matches.is_empty() {
            matches = items
                .iter()
                .enumerate()
                .filter(|(i, it)| actionable(*i) && crate::common::mnemonic(&it.text).0.chars().next().and_then(|c| c.to_lowercase().next()) == Some(key))
                .map(|(i, _)| i)
                .collect();
        }
        match matches.as_slice() {
            [] => MenuAction::None,
            [row] => {
                let row = *row;
                self.levels[depth].menu.hot_index = Some(row);
                if self.has_submenu(depth, row) {
                    match self.open(depth, row) {
                        Some(path) => {
                            self.hot_first();
                            MenuAction::Opened(path)
                        }
                        None => MenuAction::None,
                    }
                } else {
                    let mut chosen = path;
                    chosen.push(row);
                    MenuAction::Chosen(chosen)
                }
            }
            several => {
                let hot = self.levels[depth].menu.hot_index;
                let next = several.iter().copied().find(|&i| hot.is_some_and(|h| i > h)).unwrap_or(several[0]);
                self.levels[depth].menu.hot_index = Some(next);
                MenuAction::None
            }
        }
    }

    /// The root level's rows.
    pub fn root(&self) -> &Menu {
        &self.levels[0].menu
    }

    /// Rebuilds the rows of every level from [`Self::spec`] (a sub-menu refilled), keeping the hot
    /// rows; a level whose item disappeared closes with the levels after it.
    pub fn rebuild(&mut self) {
        let mut keep = 0;
        for level in &mut self.levels {
            let Some(items) = self.spec.items_at(&level.path) else { break };
            let (hot, viewport, owner_draw) = (level.menu.hot_index, level.menu.viewport, level.menu.owner_draw);
            level.menu = Menu::with_items(level_rows(items));
            level.menu.hot_index = hot.filter(|&h| h < items.len());
            level.menu.viewport = viewport;
            level.menu.owner_draw = owner_draw;
            keep += 1;
        }
        self.levels.truncate(keep.max(1));
    }

    /// Opens the sub-menu of row `row` of level `level` (closing the deeper ones). Returns its path
    /// when it was not already the open one.
    fn open(&mut self, level: usize, row: usize) -> Option<Vec<usize>> {
        let mut path = self.levels.get(level)?.path.clone();
        path.push(row);
        if self.levels.get(level + 1).is_some_and(|l| l.path == path) {
            return None;
        }
        let items = self.spec.item_at(&path).filter(|i| !i.children.is_empty() || i.items_source.is_some() || i.on_drop_down_opening.is_some())?.children.clone();
        self.levels.truncate(level + 1);
        self.levels[level].menu.hot_index = Some(row);
        let mut menu = Menu::with_items(level_rows(&items));
        menu.viewport = self.levels[level].menu.viewport;
        self.levels.push(MenuLevel { menu, path: path.clone(), panel: None });
        Some(path)
    }

    /// Whether the row `row` of level `level` opens a sub-menu.
    fn has_submenu(&self, level: usize, row: usize) -> bool {
        self.levels.get(level).is_some_and(|l| l.menu.submenu(row).is_some())
    }

    /// The keys of the frame, in order: what they did to the deepest open level.
    pub fn keys(&mut self, keys: &[u16]) -> MenuAction {
        use kubuno_desktop_controls::host::vk;
        for k in keys {
            let depth = self.levels.len() - 1;
            let hot = self.levels[depth].menu.hot_index;
            match *k {
                k if k == vk::ESCAPE || k == vk::LEFT => {
                    if depth == 0 {
                        if k == vk::ESCAPE {
                            return MenuAction::Close;
                        }
                        if self.bar.is_some() {
                            return MenuAction::Neighbour(false);
                        }
                    } else {
                        self.levels.pop();
                    }
                }
                k if k == vk::RIGHT || k == vk::ENTER || k == vk::SPACE => {
                    let on_submenu = hot.is_some_and(|h| self.levels[depth].menu.is_actionable(h) && self.has_submenu(depth, h));
                    if k == vk::RIGHT && !on_submenu && self.bar.is_some() {
                        return MenuAction::Neighbour(true);
                    }
                    let Some(row) = hot.filter(|&h| self.levels[depth].menu.is_actionable(h)) else { continue };
                    if self.has_submenu(depth, row) {
                        if let Some(path) = self.open(depth, row) {
                            if let Some(first) = (0..self.levels[depth + 1].menu.items().len()).find(|&j| self.levels[depth + 1].menu.is_actionable(j)) {
                                self.levels[depth + 1].menu.hot_index = Some(first);
                            }
                            return MenuAction::Opened(path);
                        }
                    } else if k != vk::RIGHT {
                        let mut path = self.levels[depth].path.clone();
                        path.push(row);
                        return MenuAction::Chosen(path);
                    }
                }
                k => {
                    let key = match k {
                        k if k == vk::UP => MenuKey::Up,
                        k if k == vk::DOWN => MenuKey::Down,
                        k if k == vk::HOME => MenuKey::Home,
                        k if k == vk::END => MenuKey::End,
                        _ => continue,
                    };
                    let menu = &mut self.levels[depth].menu;
                    let _ = menu.navigate(key);
                    // Each level is a flat menu: a sub-menu opens only through `open`.
                    menu.open_submenu = None;
                }
            }
        }
        MenuAction::None
    }

    /// Whether `(x, y)` is over one of its levels.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.level_at(x, y).is_some()
    }

    /// The deepest level whose panel holds `(x, y)`, with that panel.
    fn level_at(&self, x: f32, y: f32) -> Option<(usize, Rect)> {
        self.levels.iter().enumerate().rev().find_map(|(i, l)| l.panel.filter(|p| p.contains(x, y)).map(|p| (i, p)))
    }

    /// A press of a button at `(x, y)`: `None` outside every level (the menu closes).
    pub fn click(&mut self, x: f32, y: f32) -> Option<MenuAction> {
        let (level, panel) = self.level_at(x, y)?;
        let Some(row) = self.levels[level].menu.item_at(panel, x, y).filter(|&i| self.levels[level].menu.is_actionable(i)) else {
            return Some(MenuAction::None);
        };
        if self.has_submenu(level, row) {
            return Some(self.open(level, row).map(MenuAction::Opened).unwrap_or(MenuAction::None));
        }
        let mut path = self.levels[level].path.clone();
        path.push(row);
        Some(MenuAction::Chosen(path))
    }

    /// The pointer moved to `(x, y)`: the row under it becomes hot; resting on a row with a
    /// sub-menu opens it, on another row closes the deeper levels. Returns the path of the
    /// sub-menu that just opened.
    pub fn hover(&mut self, x: f32, y: f32) -> Option<Vec<usize>> {
        let (level, panel) = self.level_at(x, y)?;
        let row = self.levels[level].menu.item_at(panel, x, y).filter(|&i| self.levels[level].menu.is_actionable(i))?;
        self.levels[level].menu.hot_index = Some(row);
        if self.has_submenu(level, row) {
            return self.open(level, row);
        }
        if self.levels.len() > level + 1 {
            self.levels.truncate(level + 1);
        }
        None
    }

    /// Paints every level in its own floating surface, kept inside the monitor (`MenuDropdown`'s
    /// clamp; a sub-menu flips to the other side of its parent when it would not fit), and remembers
    /// the panels for the next frame's pointer.
    pub fn paint(&mut self, c: &dyn kubuno_desktop_controls::ControlCanvas, frame: &host::Frame, mut owner_draw: Option<&mut dyn kubuno_desktop_ui::graphics::OwnerDrawHandler>) {
        let area = frame.screen_area();
        const EDGE: f32 = kubuno_desktop_ui::lists::VIEWPORT_EDGE;
        for i in 0..self.levels.len() {
            let panel = if i == 0 {
                self.levels[0].menu.viewport = Some(area);
                let want = self.levels[0].menu.measure(c);
                let (ax, ay) = self.at;
                let x = ax.min(area.right - EDGE - want.width).max(area.left + EDGE);
                let top = ay.min(area.bottom - EDGE - want.height).max(area.top + EDGE);
                Rect::new(x, top, x + want.width, top + want.height)
            } else {
                let parent = &self.levels[i - 1];
                let (Some(parent_panel), Some(&row)) = (parent.panel, self.levels[i].path.last()) else {
                    self.levels.truncate(i);
                    break;
                };
                let Some(rect) = parent.menu.submenu_rect_in(c, parent_panel, row) else {
                    self.levels.truncate(i);
                    break;
                };
                self.levels[i].menu.viewport = Some(area);
                rect
            };
            // The keyboard cues: the mnemonics are underlined in a menu opened from the keyboard, and
            // while Alt is held.
            let underlines: Vec<Option<usize>> = match self.spec.items_at(&self.levels[i].path) {
                Some(items) if self.keyboard || frame.mods.alt => items.iter().map(|it| crate::common::mnemonic(&it.text).1.map(|(_, at)| at)).collect(),
                _ => Vec::new(),
            };
            let level = &mut self.levels[i];
            level.panel = Some(panel);
            level.menu.open_submenu = None;
            level.menu.mnemonics = underlines;
            let pb = level.menu.paint_bounds(c, panel);
            let rebase = |r: Rect| Rect::new(r.left - pb.left, r.top - pb.top, r.right - pb.left, r.bottom - pb.top);
            let mut snapshot = level.menu.clone();
            snapshot.viewport = Some(rebase(area));
            let local = rebase(panel);
            // Owner-drawn items are drawn now by the menu's handler and replayed in the popup (EVT-8).
            let recorded = match owner_draw.as_deref_mut() {
                Some(handler) if level.menu.owner_draw => level.menu.record_items(local, handler),
                _ => kubuno_desktop_ui::graphics::owner_draw::RecordedItems::new(),
            };
            host::popup(pb, move |canvas| crate::owner_draw::replay_in_popup(recorded, || snapshot.paint(canvas, local, WidgetState::REST)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Document;

    fn root(src: &str) -> Element {
        let parse = crate::syntax::parse(src);
        Document::cast(parse.syntax()).and_then(|d| d.root_element()).unwrap()
    }

    #[test]
    fn window_kinds_preset_the_form_and_the_view_wins() {
        let vm = crate::binding::MapViewModel::default();
        let dialog = FormSpec::read(&root(r#"<Panel WindowKind="Dialog"/>"#), None).options(&vm);
        assert_eq!(dialog.border_style, FormBorderStyle::FixedDialog);
        assert!(!dialog.minimize_box && !dialog.maximize_box && !dialog.show_in_taskbar);
        assert_eq!(dialog.start_position, StartPosition::CenterParent);
        let splash = FormSpec::read(&root(r#"<Panel WindowKind="Splash"/>"#), None);
        assert_eq!(splash.splash_duration, 3000.0);
        let splash = splash.options(&vm);
        assert_eq!(splash.border_style, FormBorderStyle::None);
        assert!(splash.top_most);
        let flyout = FormSpec::read(&root(r#"<Panel WindowKind="Flyout"/>"#), None).options(&vm);
        assert_eq!(flyout.backdrop, Some(host::Backdrop::Acrylic));
        assert_eq!(flyout.corner, host::CornerPreference::Round);
        assert_eq!(flyout.panel, None, "a plain flyout keeps DWM's material and corners");
        let panel = FormSpec::read(&root(r#"<Panel WindowKind="Flyout" CornerRadius="28"/>"#), None).options(&vm);
        assert_eq!(panel.panel, Some(host::FloatingPanel::new(28.0)));
        let not_a_flyout = FormSpec::read(&root(r#"<Panel WindowKind="Dialog" CornerRadius="28"/>"#), None).options(&vm);
        assert_eq!(not_a_flyout.panel, None, "only a flyout floats");
        let tool = FormSpec::read(&root(r#"<Panel WindowKind="ToolWindow" FormBorderStyle="FixedToolWindow"/>"#), None).options(&vm);
        assert_eq!(tool.border_style, FormBorderStyle::FixedToolWindow, "the view's own property wins");
        assert!(tool.effective_chrome().tool);
    }

    #[test]
    fn every_window_kind_has_its_corner_radius() {
        let vm = crate::binding::MapViewModel::default();
        let radius = |xml: &str| FormSpec::read(&root(xml), None).options(&vm).corner_radius();
        assert_eq!(radius(r#"<Panel/>"#), 8.0, "a main window is rounded like Windows 11's");
        assert_eq!(radius(r#"<Panel WindowKind="Dialog"/>"#), 8.0);
        assert_eq!(radius(r#"<Panel WindowKind="ToolWindow"/>"#), 8.0);
        assert_eq!(radius(r#"<Panel WindowKind="MdiChild"/>"#), 8.0);
        assert_eq!(radius(r#"<Panel WindowKind="Splash"/>"#), 8.0, "a splash screen is rounded though borderless");
        assert_eq!(radius(r#"<Panel WindowKind="Flyout"/>"#), 8.0);
        assert_eq!(radius(r#"<Panel FormBorderStyle="None"/>"#), 0.0, "a borderless window is square");
        assert_eq!(radius(r#"<Panel CornerRadius="0"/>"#), 0.0);
        assert_eq!(radius(r#"<Panel CornerRadius="24" CornerPreference="DoNotRound"/>"#), 24.0, "CornerRadius wins");
        assert_eq!(radius(r#"<Panel CornerRadius="-4"/>"#), 8.0, "an invalid radius is ignored");
        assert_eq!(radius(r#"<Panel FormBorderStyle="None" CornerRadius="12"/>"#), 12.0);
        // A flyout given a radius floats at it; `0` keeps a plain, square flyout.
        let square = FormSpec::read(&root(r#"<Panel WindowKind="Flyout" CornerRadius="0"/>"#), None).options(&vm);
        assert_eq!((square.panel, square.corner_radius()), (None, 0.0));
        assert_eq!(radius(r#"<Panel WindowKind="Flyout" CornerRadius="28"/>"#), 28.0);
    }

    #[test]
    fn the_title_bar_properties_reach_the_chrome() {
        let vm = crate::binding::MapViewModel::default();
        let r = root(r##"<Panel Subtitle="draft" TitleBarHeight="52" TitleBarPadding="8" TitleBarBackground="#102030" TitleAlignment="Center" CaptionButtonStyle="Windows" HelpButton="true" ShowIcon="false" ExtendContentIntoTitleBar="true" CaptionButtons="pin:Bookmark:Pin; !bell*:Bell" CornerPreference="Round" BorderColor="Danger"/>"##);
        let form = FormSpec::read(&r, None).options(&vm);
        let c = &form.chrome;
        assert_eq!(c.subtitle, "draft");
        assert_eq!(c.band_height(), 52.0);
        assert_eq!(c.padding, Some(8.0));
        assert!(c.background.is_some_and(|b| (b.r - 16.0 / 255.0).abs() < 0.01));
        assert_eq!(c.alignment, kubuno_desktop_controls::window_chrome::TitleAlignment::Center);
        assert_eq!(c.buttons, kubuno_desktop_controls::window_chrome::ButtonStyle::Windows);
        assert!(c.help_button && !c.show_icon && c.extend_content);
        assert_eq!(c.commands.len(), 2);
        assert_eq!((c.commands[0].id.as_str(), c.commands[0].glyph.as_str(), c.commands[0].tooltip.as_str()), ("pin", "Bookmark", "Pin"));
        assert!(!c.commands[1].enabled && c.commands[1].checked && c.commands[1].id == "bell");
        assert_eq!(form.corner, host::CornerPreference::Round);
        assert!(form.border_color.is_some());
    }

    #[test]
    fn children_are_placed_in_the_window_by_their_attached_properties() {
        let r = root(r#"<Panel><TextField TitleBar.Region="Center"/><Button ActionBar.Region="Right" TitleBar.Drag="true"/><Label/></Panel>"#);
        let places: Vec<ChildPlace> = r.children().map(|c| ChildPlace::read(&c)).collect();
        assert_eq!(places[0].title, TitleRegion::Center);
        assert_eq!(places[1].action, ActionRegion::Right);
        assert!(places[1].drag);
        assert_eq!(places[2], ChildPlace::default());
        // The designer lays the regions out in its own band.
        set_design_chrome(Some(DesignChrome {
            style: kubuno_desktop_controls::window_chrome::ChromeStyle::default(),
            bounds: Rect::new(0.0, 0.0, 600.0, 400.0),
            has_icon: false,
            buttons: kubuno_desktop_controls::window_chrome::SystemButtons::default(),
        }));
        let l = title_bar_slots(kubuno_desktop_controls::window_chrome::SlotWidths { left: 0.0, center: 200.0, right: 0.0 }).expect("a band");
        assert_eq!((l.center.left, l.center.right), (200.0, 400.0));
        assert_eq!(declared_slots().center, 200.0);
        set_design_chrome(None);
    }

    #[test]
    fn the_form_properties_are_read_from_the_root() {
        let r = root(
            r#"<Panel Title="Demo" StartPosition="CenterScreen" FormBorderStyle="FixedDialog" MaximizeBox="false" ShowInTaskbar="false" TopMost="true" Opacity="80" WindowState="Maximized" MinimumSize="400, 300" AcceptButton="ok" CancelButton="cancel" KeyPreview="true" Icon="app.ico"/>"#,
        );
        let spec = FormSpec::read(&r, Some(std::path::Path::new("C:/app/src")));
        let vm = crate::binding::MapViewModel::new();
        let o = spec.options(&vm);
        assert_eq!(o.title.as_deref(), Some("Demo"));
        assert_eq!(o.start_position, StartPosition::CenterScreen);
        assert_eq!(o.border_style, FormBorderStyle::FixedDialog);
        assert!(!o.maximize_box && o.minimize_box && o.control_box && !o.show_in_taskbar && o.top_most);
        assert!((o.opacity - 0.8).abs() < 1e-6);
        assert_eq!(o.window_state, WindowState::Maximized);
        assert_eq!(o.min_client_size, Some((400.0, 300.0)));
        assert!(o.icon.unwrap().replace('\\', "/").ends_with("C:/app/src/app.ico"));
        assert_eq!((spec.accept_button.as_deref(), spec.cancel_button.as_deref(), spec.key_preview), (Some("ok"), Some("cancel"), true));
        // Nothing written: a normal form.
        let d = FormSpec::read(&root(r#"<Panel/>"#), None).options(&vm);
        assert_eq!(d, FormOptions::default());
    }

    #[test]
    fn a_bound_title_and_opacity_follow_the_view_model() {
        let r = root(r#"<Panel Title="{Binding Caption}" Opacity="{Binding Fade}"/>"#);
        let spec = FormSpec::read(&r, None);
        let mut vm = crate::binding::MapViewModel::new();
        vm.set("Caption", crate::binding::Value::Str("Saved".into()));
        vm.set("Fade", crate::binding::Value::F32(50.0));
        let o = spec.options(&vm);
        assert_eq!(o.title.as_deref(), Some("Saved"));
        assert!((o.opacity - 0.5).abs() < 1e-6);
    }

    #[test]
    fn tooltip_settings_and_menus_are_read() {
        let r = root(
            r#"<Panel><ToolTip x:Name="tips" InitialDelay="200" AutoPopDelay="0"/><ContextMenu x:Name="edit" OnOpening="opening"><MenuItem Text="&amp;Copy" ShortcutKeys="Ctrl+C" OnClick="copy"/><MenuItem Text="-"/><MenuItem Text="Paste" Enabled="false"/></ContextMenu></Panel>"#,
        );
        let t = ToolTipSettings::read(&r);
        assert_eq!((t.initial_delay, t.auto_pop_delay, t.reshow_delay), (200, 0, 100));
        let menus = read_menus(&r);
        assert_eq!(menus.len(), 1);
        let m = &menus[0];
        assert_eq!((m.name.as_str(), m.opening.as_deref()), ("edit", Some("opening")));
        assert_eq!(m.items.len(), 3);
        assert_eq!((m.items[0].text.as_str(), m.items[0].handler.as_deref(), m.items[0].shortcut.as_str()), ("&Copy", Some("copy"), "Ctrl+C"));
        assert!(m.items[1].is_separator() && !m.items[2].enabled);
        let open = OpenMenu::new(m.clone(), m.clone(), (10.0, 10.0), "0".into());
        assert_eq!(open.root().items().len(), 3);
        assert!(!open.root().is_actionable(1) && !open.root().is_actionable(2) && open.root().is_actionable(0));
    }

    #[test]
    fn sub_menus_icons_radio_groups_bindings_and_list_items_are_read() {
        let r = root(
            r#"<Panel><ContextMenu x:Name="view" ItemsSource="{Binding Recent}" OnItemClicked="recent"><MenuItem Text="Sort" Icon="Search"><MenuItem Text="Name" RadioGroup="sort" Checked="true"/><MenuItem Text="Date" RadioGroup="sort"/></MenuItem><MenuItem Kind="Separator"/><MenuItem Text="Hidden files" CheckOnClick="true" Checked="{Binding ShowHidden, Mode=TwoWay}"/><MenuItem Text="Delete" Danger="true" Visible="{Binding CanDelete}"/></ContextMenu></Panel>"#,
        );
        let m = read_menus(&r).remove(0);
        assert_eq!(m.items[0].children.len(), 2);
        assert_eq!(m.items[0].icon, "Search");
        assert!(m.items[1].is_separator());
        assert!(m.items[2].check_on_click && m.items[2].bindings.iter().any(|(p, _)| *p == "Checked"));
        let vm = crate::binding::MapViewModel::new()
            .with("ShowHidden", crate::binding::Value::Bool(true))
            .with("CanDelete", crate::binding::Value::Bool(false))
            .with("Recent", crate::binding::Value::from(vec![crate::binding::Row::new().with("Text", crate::binding::Value::Str("a.txt".into()))]));
        let resolved = m.resolved(&vm, &std::collections::HashMap::new());
        assert!(resolved.items[2].checked, "the binding is read when the menu opens");
        assert!(!resolved.items.iter().any(|i| i.text == "Delete"), "hidden");
        assert_eq!(resolved.items.last().and_then(|i| i.row_key.as_deref()), Some("a.txt"));
        let mut open = OpenMenu::new(m.clone(), resolved, (10.0, 10.0), "0".into());
        assert!(open.root().submenu(0).is_some(), "the first row has a sub-menu");
        open.levels[0].panel = Some(Rect::new(10.0, 10.0, 210.0, 200.0));
        let row = open.root().item_rect(Rect::new(10.0, 10.0, 210.0, 200.0), 0).expect("row");
        assert_eq!(open.hover(50.0, (row.top + row.bottom) / 2.0), Some(vec![0]), "resting on it opens the sub-menu");
        assert_eq!(open.levels.len(), 2);
        assert_eq!(open.spec.item_at(&[0, 1]).map(|i| i.text.as_str()), Some("Date"));
    }

    #[test]
    fn sub_menus_nest_to_any_depth() {
        use kubuno_desktop_controls::host::vk;
        let r = root(
            r#"<Panel><ContextMenu x:Name="deep"><MenuItem Text="One"><MenuItem Text="Two"><MenuItem Text="Three"><MenuItem Text="Leaf" OnClick="leaf"/></MenuItem></MenuItem></MenuItem><MenuItem Text="Other"/></ContextMenu></Panel>"#,
        );
        let m = read_menus(&r).remove(0);
        let mut open = OpenMenu::new(m.clone(), m.clone(), (0.0, 0.0), "0".into());
        assert_eq!(open.keys(&[vk::DOWN]), MenuAction::None);
        assert_eq!(open.keys(&[vk::RIGHT]), MenuAction::Opened(vec![0]));
        assert_eq!(open.keys(&[vk::RIGHT]), MenuAction::Opened(vec![0, 0]));
        assert_eq!(open.keys(&[vk::ENTER]), MenuAction::Opened(vec![0, 0, 0]));
        assert_eq!(open.levels.len(), 4);
        assert_eq!(open.keys(&[vk::ENTER]), MenuAction::Chosen(vec![0, 0, 0, 0]));
        assert_eq!(open.spec.item_at(&[0, 0, 0, 0]).and_then(|i| i.handler.as_deref()), Some("leaf"));
        // Left closes the deepest level only; Escape at the root closes the menu.
        assert_eq!(open.keys(&[vk::LEFT, vk::LEFT]), MenuAction::None);
        assert_eq!(open.levels.len(), 2);
        assert_eq!(open.keys(&[vk::LEFT, vk::ESCAPE]), MenuAction::Close);
    }

    #[test]
    fn a_tooltip_waits_shows_then_pops_like_winforms() {
        let s = ToolTipSettings::default();
        let mut t = TooltipState::default();
        assert_eq!(t.update(Some(("a", "Hi")), 1000, &s, true), TooltipShow::Wait(Some(500)));
        assert_eq!(t.update(Some(("a", "Hi")), 1400, &s, true), TooltipShow::Wait(Some(100)));
        assert_eq!(t.update(Some(("a", "Hi")), 1500, &s, true), TooltipShow::Show { text: "Hi".into(), hide_in: Some(5000) });
        assert_eq!(t.update(Some(("a", "Hi")), 6500, &s, true), TooltipShow::Wait(None), "auto-pop");
        // Another control soon after: the reshow delay.
        assert_eq!(t.update(Some(("b", "Other")), 6600, &s, true), TooltipShow::Wait(Some(100)));
        assert!(matches!(t.update(Some(("b", "Other")), 6700, &s, true), TooltipShow::Show { .. }));
        // An inactive window shows none, unless ShowAlways.
        assert_eq!(t.update(Some(("b", "Other")), 6800, &s, false), TooltipShow::Wait(None));
        let always = ToolTipSettings { show_always: true, ..s };
        let mut t = TooltipState::default();
        t.update(Some(("a", "Hi")), 0, &always, false);
        assert!(matches!(t.update(Some(("a", "Hi")), 600, &always, false), TooltipShow::Show { .. }));
        // A press hides it until the next control.
        t.press(700);
        assert!(matches!(t.update(Some(("a", "Hi")), 800, &always, false), TooltipShow::Wait(_)));
    }
}

#[cfg(test)]
#[path = "window_header_tests.rs"]
mod header_tests;
