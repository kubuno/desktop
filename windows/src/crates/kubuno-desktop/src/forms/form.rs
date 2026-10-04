//! [`Form`]: a window's content and state — a `.kbview` view, controls created in code, or both.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use kubuno_desktop_views::binding::Value;
use kubuno_desktop_views::events::{CloseReason, EmptyEventArgs, FormClosedEventArgs, FormClosingEventArgs};

use super::compose::Synthetic;
use super::{
    AsControl, Backdrop, Button, CaptionButtonStyle, CaptionCommand, Control, ControlCollection, ControlEvent, CornerPreference, DialogResult, FormBorderStyle, NameCounters,
    StartPosition, TitleAlignment, WindowKind,
};

/// Where a form's `.kbview` comes from (what `#[kubuno_desktop::view]` records).
#[derive(Debug, Clone)]
pub(crate) struct Source {
    /// The file, for hot reload in debug builds.
    pub path: Option<String>,
    /// The text: embedded at compile time, then what hot reload read.
    pub text: String,
    /// Its name in messages.
    pub display: String,
}

/// The state a form's handles share.
pub(crate) struct FormShared {
    /// The root element (the window's surface): its properties are the form's (`Title`,
    /// `DesignWidth`…), the controls created in code are its children.
    pub root: Control,
    pub source: RefCell<Option<Source>>,
    /// The fields `#[kubuno_desktop::view]` generated, by `x:Name`.
    pub members: RefCell<HashMap<String, Control>>,
    /// Handles the form made for the elements of its view that no field stands for (a named element
    /// of a view written by hand, an element with handlers): by `x:Name` or by element path.
    pub hidden: RefCell<HashMap<String, Control>>,
    /// The controls of the composed view, by index (`__kb.<index>.<Property>` bindings).
    pub store: RefCell<Vec<Control>>,
    /// The handlers of the composed view (`OnClick="__kb3"`).
    pub synthetic: RefCell<Vec<Synthetic>>,
    /// A new composition is needed (a control added, a literal property changed).
    pub dirty: Cell<bool>,
    /// A bound value changed: the next frame shows it.
    pub changed: Cell<bool>,
    pub close_request: Cell<Option<CloseReason>>,
    pub dialog_result: Cell<DialogResult>,
    /// Open as a modal dialog (`show_dialog`).
    pub modal: Cell<bool>,
    /// Open in a window.
    pub open: Cell<bool>,
    /// The window (`HWND`), while open.
    pub hwnd: Cell<isize>,
    pub names: RefCell<NameCounters>,
    /// The runtime's `UiDispatcher<V>` of the open window (typed by the view: see `View::dispatcher`).
    pub dispatcher: RefCell<Option<Rc<dyn std::any::Any>>>,
    /// The named components of the open window's view (the printing handles reach theirs here).
    pub scope: RefCell<Option<kubuno_desktop_views::scope::ComponentScope>>,
    /// The form that owns this one (`Owner`): its window stays above the owner's.
    pub owner: RefCell<Option<std::rc::Weak<FormShared>>>,
    /// The MDI parent (`MdiParent`): the form opens inside its client area.
    pub mdi_parent: RefCell<Option<std::rc::Weak<FormShared>>>,
    /// Windows to open inside this one at its next frame (MDI children, in-window dialogs).
    pub pending_inner: RefCell<Vec<crate::application::InnerRequest>>,
    /// The forms open inside this one (MDI children, in-window dialogs), bottom first.
    pub inner: RefCell<Vec<Form>>,
    /// A `LayoutMdi` asked for, applied at the next frame.
    pub layout_mdi: Cell<Option<crate::application::MdiLayout>>,
    /// Message handlers to install on the window when it opens (`Form::on_message`).
    pub message_handlers: RefCell<Vec<(u32, crate::application::MessageHandler)>>,
    /// Open without showing the window (`start_hidden`).
    pub start_hidden: Cell<bool>,
    /// `Some(visible)`: show or hide the window at the next frame.
    pub visibility: Cell<Option<bool>>,
    /// The page area of the open window as of its last frame (`ClientSize` follows the window).
    pub live_size: Cell<Option<(f32, f32)>>,
}

impl FormShared {
    /// A generated name for a control of `element` added in code: `button1`, `textField2`… (Windows
    /// Forms' designer names), not used by any other control of the form.
    pub fn next_name(&self, element: &str) -> String {
        let mut stem: String = element.chars().take(1).flat_map(char::to_lowercase).collect();
        stem.push_str(element.get(1..).unwrap_or(""));
        let mut names = self.names.borrow_mut();
        let counter = names.entry(stem.clone()).or_insert(0);
        loop {
            *counter += 1;
            let candidate = format!("{stem}{counter}");
            let taken = self.members.borrow().contains_key(&candidate)
                || self.hidden.borrow().contains_key(&candidate)
                || self.root.controls().find(&candidate).is_some();
            if !taken {
                return candidate;
            }
        }
    }
}

/// A window's content and state — Windows Forms' `Form`: a `.kbview` view (a struct with
/// `#[kubuno_desktop::view]` derefs to its form), controls created in code ([`Form::controls`]), or both.
/// A cheap, clonable handle.
///
/// ```no_run
/// use kubuno_desktop::prelude::*;
///
/// let form = Form::new().text("Settings").client_size(420.0, 180.0).start_position(StartPosition::CenterScreen);
/// let ok = Button::new().text("OK").bounds(316.0, 128.0, 88.0, 36.0).anchor(Anchor::BOTTOM | Anchor::RIGHT);
/// form.controls().add(&ok);
/// form.set_accept_button(&ok);
/// ```
#[derive(Clone)]
pub struct Form {
    pub(crate) shared: Rc<FormShared>,
}

impl Default for Form {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Form {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Form({:?})", self.get_text())
    }
}

impl PartialEq for Form {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.shared, &other.shared)
    }
}

impl Form {
    /// An empty form of 800 × 450 DIP (a `<Panel>` surface where each control has its position).
    pub fn new() -> Self {
        let root = Control::new("Panel");
        let shared = Rc::new(FormShared {
            root: root.clone(),
            source: RefCell::new(None),
            members: RefCell::new(HashMap::new()),
            hidden: RefCell::new(HashMap::new()),
            store: RefCell::new(Vec::new()),
            synthetic: RefCell::new(Vec::new()),
            dirty: Cell::new(true),
            changed: Cell::new(false),
            close_request: Cell::new(None),
            dialog_result: Cell::new(DialogResult::None),
            modal: Cell::new(false),
            open: Cell::new(false),
            hwnd: Cell::new(0),
            names: RefCell::new(HashMap::new()),
            dispatcher: RefCell::new(None),
            scope: RefCell::new(None),
            owner: RefCell::new(None),
            mdi_parent: RefCell::new(None),
            pending_inner: RefCell::new(Vec::new()),
            inner: RefCell::new(Vec::new()),
            layout_mdi: Cell::new(None),
            message_handlers: RefCell::new(Vec::new()),
            start_hidden: Cell::new(false),
            visibility: Cell::new(None),
            live_size: Cell::new(None),
        });
        // The root has no name of its own: it is the form.
        *root.0.form.borrow_mut() = Rc::downgrade(&shared);
        root.0.from_view.set(true);
        Self { shared }
    }

    /// The root control (the surface: `Panel`, or the root element of the view).
    pub fn root(&self) -> &Control {
        &self.shared.root
    }

    // ── Builders ─────────────────────────────────────────────────────────────────────────────

    /// Its title (`Text`).
    pub fn text(self, text: impl Into<String>) -> Self {
        self.set_text(text);
        self
    }

    /// The size of its page area (the window's client area), in DIP.
    pub fn client_size(self, width: f32, height: f32) -> Self {
        self.set_client_size(width, height);
        self
    }

    /// Where the window opens.
    pub fn start_position(self, position: StartPosition) -> Self {
        self.set_start_position(position);
        self
    }

    /// The window's border and caption.
    pub fn form_border_style(self, style: FormBorderStyle) -> Self {
        self.set_form_border_style(style);
        self
    }

    /// Whether the caption shows the maximize button.
    pub fn maximize_box(self, shown: bool) -> Self {
        self.root().set_property("MaximizeBox", shown);
        self
    }

    /// Whether the caption shows the minimize button.
    pub fn minimize_box(self, shown: bool) -> Self {
        self.root().set_property("MinimizeBox", shown);
        self
    }

    /// Any property of the view's root (`"TopMost"`, `"ShowInTaskbar"`, `"Opacity"`…).
    pub fn property(self, name: &str, value: impl Into<Value>) -> Self {
        self.root().set_property(name, value);
        self
    }

    // ── Properties ───────────────────────────────────────────────────────────────────────────

    /// Its title (the root's `Title`).
    pub fn get_text(&self) -> String {
        self.root().string("Title")
    }

    pub fn set_text(&self, text: impl Into<String>) {
        self.root().set_property("Title", Value::Str(text.into()));
    }

    /// The size of its page area, in DIP: the open window's current one (WinForms' `ClientSize`
    /// follows the window), else the view's `DesignWidth` × `DesignHeight`.
    pub fn get_client_size(&self) -> (f32, f32) {
        if self.shared.open.get() {
            if let Some(size) = self.shared.live_size.get() {
                return size;
            }
        }
        (self.root().number("DesignWidth").unwrap_or(800.0), self.root().number("DesignHeight").unwrap_or(450.0))
    }

    /// Sets the size of its page area: the window opens at that size, and a window already open is
    /// resized to it (WinForms' `ClientSize`), keeping its top-left corner.
    pub fn set_client_size(&self, width: f32, height: f32) {
        self.root().set_property("DesignWidth", width);
        self.root().set_property("DesignHeight", height);
        if let Some(hwnd) = self.handle() {
            kubuno_desktop_controls::host::resize_page(hwnd, width, height);
        }
    }

    pub fn set_start_position(&self, position: StartPosition) {
        self.root().set_property("StartPosition", format!("{position:?}"));
    }

    pub fn set_form_border_style(&self, style: FormBorderStyle) {
        self.root().set_property("FormBorderStyle", format!("{style:?}"));
    }

    /// The button Enter clicks (`AcceptButton`).
    pub fn set_accept_button(&self, button: &Button) {
        self.root().set_property("AcceptButton", button.get_name());
    }

    /// The button Escape clicks (`CancelButton`).
    pub fn set_cancel_button(&self, button: &Button) {
        self.root().set_property("CancelButton", button.get_name());
    }

    /// Its top-level controls created in code (`controls().add(&button)`). The controls of its
    /// `.kbview` are its fields, or [`Form::control`].
    pub fn controls(&self) -> ControlCollection {
        self.root().controls()
    }

    /// The control named `name`: an element of the view, or a control added in code.
    pub fn control(&self, name: &str) -> Option<Control> {
        if let Some(c) = self.shared.members.borrow().get(name) {
            return Some(c.clone());
        }
        if let Some(c) = self.shared.hidden.borrow().get(name) {
            return Some(c.clone());
        }
        self.controls().find(name)
    }

    /// Adds a control created in code inside the view's element named `container` (a `Panel`,
    /// a `GroupBox`…); `false` when the view has no such element.
    pub fn add_to(&self, container: &str, control: &impl AsControl) -> bool {
        match self.control(container) {
            Some(parent) => {
                parent.controls().add(control);
                true
            }
            None => false,
        }
    }

    // ── Window ───────────────────────────────────────────────────────────────────────────────

    /// Closes the window, as its close button would (`FormClosing` may cancel it).
    pub fn close(&self) {
        self.shared.close_request.set(Some(CloseReason::UserClosing));
        self.shared.changed.set(true);
    }

    /// How the dialog was closed (see [`DialogResult`]).
    pub fn dialog_result(&self) -> DialogResult {
        self.shared.dialog_result.get()
    }

    /// Sets the dialog's result; on a modal dialog, anything but `None` closes it (Windows Forms).
    pub fn set_dialog_result(&self, result: DialogResult) {
        self.shared.dialog_result.set(result);
        if result != DialogResult::None && self.shared.modal.get() {
            self.close();
        }
    }

    /// Whether the form is open in a window.
    pub fn is_open(&self) -> bool {
        self.shared.open.get()
    }

    /// Whether it is open as a modal dialog.
    pub fn is_modal(&self) -> bool {
        self.shared.modal.get()
    }

    /// The window's handle (`HWND` as an integer), while open.
    pub fn handle(&self) -> Option<isize> {
        Some(self.shared.hwnd.get()).filter(|h| *h != 0)
    }

    // ── Window kind and chrome ───────────────────────────────────────────────────────────────

    /// The kind of window (`WindowKind`): a main window, a dialog, a tool window, a splash screen,
    /// a flyout or an MDI document — a preset of the border, buttons and placement.
    pub fn window_kind(self, kind: WindowKind) -> Self {
        self.set_window_kind(kind);
        self
    }

    pub fn set_window_kind(&self, kind: WindowKind) {
        self.root().set_property("WindowKind", format!("{kind:?}"));
    }

    /// The window's icon — its title bar, the task bar, Alt+Tab: a name of the Kubuno icon set
    /// (`"FileText"`), or an image file (`.ico`, `.svg`, `.png`…, [`super::IconSource::file`]).
    pub fn set_icon(&self, icon: impl Into<super::IconSource>) {
        self.root().set_property("Icon", icon.into());
    }

    /// Builder: its icon (see [`Form::set_icon`]).
    pub fn icon(self, icon: impl Into<super::IconSource>) -> Self {
        self.set_icon(icon);
        self
    }

    /// The smaller text after the title in the title bar.
    pub fn set_subtitle(&self, subtitle: impl Into<String>) {
        self.root().set_property("Subtitle", Value::Str(subtitle.into()));
    }

    /// The title bar's height class (`TitleBarStyle`): `Standard` (32 DIP, Windows 11's — the default) or
    /// `Tall` (64 DIP, the web's header — the default of a window showing the header's menus).
    pub fn set_title_bar_style(&self, style: kubuno_desktop_controls::window_chrome::TitleBarStyle) {
        use kubuno_desktop_controls::window_chrome::TitleBarStyle;
        let name = match style {
            TitleBarStyle::Standard => "Standard",
            TitleBarStyle::Tall => "Tall",
        };
        self.root().set_property("TitleBarStyle", Value::Str(name.into()));
    }

    /// The title bar's exact height, in DIP, over its style (`TitleBarHeight`; unset: 32, or 64 for a
    /// `Tall` title bar).
    pub fn set_title_bar_height(&self, height: f32) {
        self.root().set_property("TitleBarHeight", height);
    }

    /// The title bar's side insets, in DIP (`TitleBarPadding`; 8 in a band under 40 DIP, 16 in a taller one): where its icon or its
    /// left controls start, and where Kubuno-style caption buttons end.
    pub fn set_title_bar_padding(&self, padding: f32) {
        self.root().set_property("TitleBarPadding", padding);
    }

    /// The title bar's colours: a theme colour name (`"Primary"`, `"Surface"`…, following the theme)
    /// or a colour of your own (`"#1A73E8"`); empty keeps the default.
    pub fn set_title_bar_colors(&self, background: &str, foreground: &str) {
        self.root().set_property("TitleBarBackground", background.to_string());
        self.root().set_property("TitleBarForeground", foreground.to_string());
    }

    /// This window's own accent colour (its title bar and accent controls).
    pub fn set_accent_color(&self, color: &str) {
        self.root().set_property("AccentColor", color.to_string());
    }

    /// Where the title sits in the band.
    pub fn set_title_alignment(&self, alignment: TitleAlignment) {
        self.root().set_property("TitleAlignment", format!("{alignment:?}"));
    }

    /// Kubuno's rounded caption buttons, or Windows' wide ones.
    pub fn set_caption_button_style(&self, style: CaptionButtonStyle) {
        self.root().set_property("CaptionButtonStyle", format!("{style:?}"));
    }

    /// Buttons of your own in the title bar, next to minimise; a click raises
    /// [`Form::caption_button_click`] with the button's id.
    pub fn set_caption_buttons(&self, buttons: &[CaptionCommand]) {
        let text: Vec<String> = buttons
            .iter()
            .map(|b| {
                let id = format!("{}{}{}", if b.enabled { "" } else { "!" }, b.id, if b.checked { "*" } else { "" });
                format!("{id}:{}:{}", b.glyph, b.tooltip)
            })
            .collect();
        self.root().set_property("CaptionButtons", text.join("; "));
    }

    /// The help button of the title bar (`HelpButton`); a click raises
    /// [`Form::help_button_clicked`].
    pub fn set_help_button(&self, shown: bool) {
        self.root().set_property("HelpButton", shown);
    }

    /// The header's standard items in the title bar, as on the web's header (`ShowSearch`, `ShowNotifications`,
    /// `ShowSettings`, `ShowHelp`, `ShowWaffle`, `ShowAccount`): at the end of its right region, the search button
    /// then the bell, settings, help, waffle and avatar. All off by default. The bell, settings, help, waffle and
    /// avatar are the `HeaderActions` user control, which the application must link (the Kubuno shell controls); the
    /// waffle and the avatar open their menus themselves.
    pub fn set_header_items(&self, items: HeaderItems) {
        let root = self.root();
        root.set_property("ShowSearch", items.search);
        root.set_property("ShowNotifications", items.notifications);
        root.set_property("ShowSettings", items.settings);
        root.set_property("ShowHelp", items.help);
        root.set_property("ShowWaffle", items.waffle);
        root.set_property("ShowAccount", items.account);
    }

    /// The counter on the title bar's notifications bell (`UnreadCount`; 0 shows none).
    pub fn set_unread_count(&self, unread: u32) {
        self.root().set_property("UnreadCount", unread as f32);
    }

    /// The view draws under the title bar (`ExtendContentIntoTitleBar`).
    pub fn set_extend_content_into_title_bar(&self, on: bool) {
        self.root().set_property("ExtendContentIntoTitleBar", on);
    }

    /// The window's corners as a preset (`Round` 8 DIP, `RoundSmall` 4, `DoNotRound` square;
    /// `Default`: rounded at 8 with a title bar, square without). [`Form::set_corner_radius`] wins.
    pub fn set_corner_preference(&self, corner: CornerPreference) {
        self.root().set_property("CornerPreference", format!("{corner:?}"));
    }

    /// The radius of the window's corners, in DIP (`CornerRadius`; 8 by default, 0 for square
    /// corners). A maximised, snapped or full-screen window is square whatever it says. Drawn by
    /// DWM on Windows 11 when it is one of its radii (8, 4), by Kubuno otherwise and on Windows 10
    /// (`kubuno_desktop_controls::host::frame`); the radius a window opens with decides which, a later change
    /// keeps the path (DWM then draws its nearest radius).
    pub fn set_corner_radius(&self, radius: f32) {
        self.root().set_property("CornerRadius", radius.max(0.0));
    }

    /// Builder: the radius of its corners (see [`Form::set_corner_radius`]).
    pub fn corner_radius(self, radius: f32) -> Self {
        self.set_corner_radius(radius);
        self
    }

    /// The system material behind the window (Mica, Mica Alt, Acrylic).
    pub fn set_backdrop(&self, backdrop: Backdrop) {
        self.root().set_property("Backdrop", format!("{backdrop:?}"));
    }

    /// The window's 1 px border colour (empty: the title bar's).
    pub fn set_border_color(&self, color: &str) {
        self.root().set_property("BorderColor", color.to_string());
    }

    // ── Owner, MDI ───────────────────────────────────────────────────────────────────────────

    /// The form that owns this one (`Owner`): its window stays above the owner's and is minimised
    /// with it. Set it before [`Form::show`].
    pub fn set_owner(&self, owner: &dyn AsForm) {
        *self.shared.owner.borrow_mut() = Some(Rc::downgrade(&owner.as_form().shared));
    }

    /// The form that owns this one.
    pub fn owner(&self) -> Option<Form> {
        self.shared.owner.borrow().as_ref().and_then(std::rc::Weak::upgrade).map(|shared| Form { shared })
    }

    /// The open forms this one owns (`OwnedForms`).
    pub fn owned_forms(&self) -> Vec<Form> {
        crate::Application::open_forms().into_iter().filter(|f| f.owner().as_ref() == Some(self)).collect()
    }

    /// Makes the form the parent of MDI documents (`IsMdiContainer`).
    pub fn set_is_mdi_container(&self, on: bool) {
        self.root().set_property("IsMdiContainer", on);
    }

    /// The MDI parent (`MdiParent`): [`Form::show`] then opens the form inside the parent's client
    /// area, in the in-window `FloatingWindow` look.
    pub fn set_mdi_parent(&self, parent: &dyn AsForm) {
        *self.shared.mdi_parent.borrow_mut() = Some(Rc::downgrade(&parent.as_form().shared));
    }

    pub fn mdi_parent(&self) -> Option<Form> {
        self.shared.mdi_parent.borrow().as_ref().and_then(std::rc::Weak::upgrade).map(|shared| Form { shared })
    }

    /// The MDI documents open in this form, bottom first (`MdiChildren`).
    pub fn mdi_children(&self) -> Vec<Form> {
        self.shared.inner.borrow().clone()
    }

    /// The active MDI document (`ActiveMdiChild`): the topmost one.
    pub fn active_mdi_child(&self) -> Option<Form> {
        self.shared.inner.borrow().last().cloned()
    }

    /// Arranges the MDI documents (`LayoutMdi`), at the next frame.
    pub fn layout_mdi(&self, layout: crate::application::MdiLayout) {
        self.shared.layout_mdi.set(Some(layout));
        self.shared.changed.set(true);
    }

    /// Brings an MDI document to the front (`Activate`).
    pub fn activate(&self) {
        if let Some(parent) = self.mdi_parent() {
            let mut inner = parent.shared.inner.borrow_mut();
            if let Some(i) = inner.iter().position(|f| f == self) {
                let me = inner.remove(i);
                inner.push(me);
            }
            parent.shared.changed.set(true);
        }
    }

    // ── Window messages, visibility ──────────────────────────────────────────────────────────

    /// Handles window message `msg` of the form's window (a tray callback, `WM_COPYDATA`,
    /// `WM_SETTINGCHANGE`…): the handler runs before the host's own handling, and returning
    /// `Some(result)` ends the message. Installed when the window opens.
    pub fn on_message(&self, msg: u32, handler: impl FnMut(&kubuno_desktop_controls::host::MessageArgs) -> Option<isize> + 'static) {
        self.shared.message_handlers.borrow_mut().push((msg, Box::new(handler)));
    }

    /// Opens the window hidden (a tray application); [`Form::set_visible`] shows it later.
    pub fn start_hidden(self, hidden: bool) -> Self {
        self.shared.start_hidden.set(hidden);
        self
    }

    /// Shows or hides the open window (`Visible`), at the next frame.
    pub fn set_visible(&self, visible: bool) {
        self.shared.visibility.set(Some(visible));
        self.shared.changed.set(true);
        kubuno_desktop_controls::host::request_repaint_after(1);
        if let Some(hwnd) = self.handle() {
            kubuno_desktop_controls::host::set_window_visible(hwnd, visible);
        }
    }

    /// Hides the window (`Hide()`); it stays open.
    pub fn hide(&self) {
        self.set_visible(false);
    }

    // ── Events ───────────────────────────────────────────────────────────────────────────────

    /// `ResizeBegin`: the user starts moving or resizing the window.
    pub fn resize_begin(&self) -> ControlEvent<'_, Form, EmptyEventArgs> {
        self.root().on("OnResizeBegin")
    }

    /// `ResizeEnd`.
    pub fn resize_end(&self) -> ControlEvent<'_, Form, EmptyEventArgs> {
        self.root().on("OnResizeEnd")
    }

    /// A double-click on the title bar.
    pub fn title_bar_double_click(&self) -> ControlEvent<'_, Form, EmptyEventArgs> {
        self.root().on("OnTitleBarDoubleClick")
    }

    /// `DpiChanged`: the window moved to a display of another scale.
    pub fn dpi_changed(&self) -> ControlEvent<'_, Form, kubuno_desktop_views::events::DpiChangedEventArgs> {
        self.root().on("OnDpiChanged")
    }

    /// `HelpButtonClicked`.
    pub fn help_button_clicked(&self) -> ControlEvent<'_, Form, EmptyEventArgs> {
        self.root().on("OnHelpButtonClicked")
    }

    /// `SearchClicked`: the title bar's search button ([`Form::set_header_items`]).
    pub fn search_clicked(&self) -> ControlEvent<'_, Form, EmptyEventArgs> {
        self.root().on("OnSearchClicked")
    }

    /// `NotificationsClicked`: the title bar's notifications bell.
    pub fn notifications_clicked(&self) -> ControlEvent<'_, Form, EmptyEventArgs> {
        self.root().on("OnNotificationsClicked")
    }

    /// `SettingsClicked`: the title bar's settings button.
    pub fn settings_clicked(&self) -> ControlEvent<'_, Form, EmptyEventArgs> {
        self.root().on("OnSettingsClicked")
    }

    /// `HelpClicked`: the title bar's header help button (not the caption's `?`, [`Form::help_button_clicked`]).
    pub fn help_clicked(&self) -> ControlEvent<'_, Form, EmptyEventArgs> {
        self.root().on("OnHelpClicked")
    }

    /// One of the window's own caption buttons ([`Form::set_caption_buttons`]) was clicked.
    pub fn caption_button_click(&self) -> ControlEvent<'_, Form, kubuno_desktop_views::events::CaptionButtonEventArgs> {
        self.root().on("OnCaptionButtonClick")
    }

    /// `MdiChildActivate`: another MDI document became active, or one closed.
    pub fn mdi_child_activate(&self) -> ControlEvent<'_, Form, EmptyEventArgs> {
        self.root().on("OnMdiChildActivate")
    }

    /// `Load`: once, when the form's window is created and before it is first shown — a form
    /// started hidden ([`Form::start_hidden`]) gets it too, without being shown.
    pub fn load(&self) -> ControlEvent<'_, Form, EmptyEventArgs> {
        self.root().on("OnLoad")
    }

    /// `Shown`: once, at the first frame the window is actually on screen.
    pub fn shown(&self) -> ControlEvent<'_, Form, EmptyEventArgs> {
        self.root().on("OnShown")
    }

    pub fn activated(&self) -> ControlEvent<'_, Form, EmptyEventArgs> {
        self.root().on("OnActivated")
    }

    pub fn deactivate(&self) -> ControlEvent<'_, Form, EmptyEventArgs> {
        self.root().on("OnDeactivate")
    }

    /// `FormClosing`: set `e.cancel` to keep the window open.
    pub fn form_closing(&self) -> ControlEvent<'_, Form, FormClosingEventArgs> {
        self.root().on("OnFormClosing")
    }

    pub fn form_closed(&self) -> ControlEvent<'_, Form, FormClosedEventArgs> {
        self.root().on("OnFormClosed")
    }

    /// Opens the form in a window of its own and returns at once (Windows Forms' `Show()`).
    pub fn show(self) {
        crate::View::show(self);
    }

    /// Opens the form as a modal dialog owned by `owner`; returns how it was closed.
    pub fn show_dialog(&mut self, owner: &dyn AsForm) -> DialogResult {
        crate::View::show_dialog(self, owner)
    }
}

/// Anything that has a form: a [`Form`], a struct with `#[kubuno_desktop::view]` — what a dialog's owner is.
pub trait AsForm {
    fn as_form(&self) -> &Form;
}

impl<V: crate::View> AsForm for V {
    fn as_form(&self) -> &Form {
        self.form()
    }
}

/// Which of the web header's standard items a window shows in its title bar ([`Form::set_header_items`]): all off by
/// default. `HeaderItems { waffle: true, account: true, ..HeaderItems::default() }` is the web's search mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HeaderItems {
    /// The search button (a magnifier): [`Form::search_clicked`].
    pub search: bool,
    /// The notifications bell and its counter ([`Form::set_unread_count`]): [`Form::notifications_clicked`].
    pub notifications: bool,
    /// The settings button: [`Form::settings_clicked`].
    pub settings: bool,
    /// The header's help button: [`Form::help_clicked`].
    pub help: bool,
    /// The apps launcher (the waffle).
    pub waffle: bool,
    /// The account avatar.
    pub account: bool,
}

impl HeaderItems {
    /// Every item, as the web's main header shows them.
    pub const ALL: Self = Self { search: true, notifications: true, settings: true, help: true, waffle: true, account: true };
}
