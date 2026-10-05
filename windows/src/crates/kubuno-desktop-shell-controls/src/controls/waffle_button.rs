//! Code-behind of the user control `WaffleButton` (`waffle_button.kbcontrol`, see its comment): the
//! header's waffle, which opens the app launcher ([`crate::WaffleMenu`]) in a
//! [`kubuno_desktop::popup::Popup`] — a floating window of its own, so the launcher may extend beyond a small
//! app window.
//!
//! Drop it in a view's header and give it the app's [`LauncherService`] — once for every button of
//! the thread with [`set_default_launcher`], or per button with [`WaffleButton::set_service`]. The
//! service lists the apps and the favourites, opens an app and saves the favourites; the panel closes
//! after a launch, on Escape (an edit is abandoned first) and on a click outside.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use kubuno_desktop::popup::Popup;
use kubuno_desktop::views::prelude::*;
use kubuno_desktop::prelude::Custom;
use kubuno_desktop::{DockStyle, Form};

use crate::controls::header_popup::{self, PopupAnchor, PopupState};
use crate::controls::waffle_menu::{self, AppEventArgs, ContentHeightEventArgs, EditModeEventArgs, WaffleMenu, MAX_HEIGHT};
use crate::model::launcher::LauncherService;
use crate::ShellControlsResources;

thread_local! {
    static DEFAULT_LAUNCHER: RefCell<Option<Rc<dyn LauncherService>>> = const { RefCell::new(None) };
}

/// The service every [`WaffleButton`] of this UI thread uses unless it was given its own: an app
/// sets it once (and again when its apps or favourites change).
pub fn set_default_launcher(service: Rc<dyn LauncherService>) {
    DEFAULT_LAUNCHER.with(|d| *d.borrow_mut() = Some(service));
}

fn default_launcher() -> Option<Rc<dyn LauncherService>> {
    DEFAULT_LAUNCHER.with(|d| d.borrow().clone())
}

/// The header's waffle (see the module doc).
#[derive(UserControl)]
#[user_control(view = "waffle_button.kbcontrol")]
#[category("Kubuno")]
#[toolbox(icon = "layout-grid")]
#[default_event("PopupOpened")]
pub struct WaffleButton {
    base: UserControlCore,
    /// What the panel hangs from: `Button` (under the button, right edges aligned) or `Window` (the
    /// window's top-right corner, as the shell's header does).
    #[property]
    #[default_value("Button")]
    #[category("Layout")]
    pub popup_anchor: String,
    /// `Button`: the gap between the button and the panel (4 by default, the web's `sideOffset`);
    /// `Window`: the panel's top, below the window's client top.
    #[property]
    #[default_value(4.0)]
    #[category("Layout")]
    pub popup_offset: f32,
    /// `Window`: the gap between the panel and the window's right edge.
    #[property]
    #[default_value(8.0)]
    #[category("Layout")]
    pub popup_margin: f32,
    /// The room kept free under the panel (above the screen's or the window's bottom).
    #[property]
    #[default_value(12.0)]
    #[category("Layout")]
    pub popup_bottom_gap: f32,
    /// The round button's diameter: 36 in a 64-DIP header, 30 in a title bar (the caption buttons' size).
    #[property(bindable)]
    #[default_value(36.0)]
    #[category("Appearance")]
    pub button_diameter: f32,
    /// Its glyph's size.
    #[property(bindable)]
    #[default_value(18.0)]
    #[category("Appearance")]
    pub button_glyph: f32,
    /// Occurs when the launcher opens.
    #[event]
    #[category("Action")]
    pub popup_opened: Event<EmptyEventArgs>,
    service: Option<Rc<dyn LauncherService>>,
    popup: PopupState,
}

impl Default for WaffleButton {
    fn default() -> Self {
        Self {
            base: UserControlCore::default(),
            popup_anchor: "Button".into(),
            popup_offset: 4.0,
            popup_margin: 8.0,
            popup_bottom_gap: 12.0,
            button_diameter: 36.0,
            button_glyph: 18.0,
            popup_opened: Event::default(),
            service: None,
            popup: PopupState::default(),
        }
    }
}

impl WaffleButton {
    /// This button's own service (else the thread's [`set_default_launcher`]).
    pub fn set_service(&mut self, service: Rc<dyn LauncherService>) {
        self.service = Some(service);
    }

    fn service(&self) -> Option<Rc<dyn LauncherService>> {
        self.service.clone().or_else(default_launcher)
    }

    /// Opens the launcher under the button (see the module doc); nothing when it is open.
    pub fn open(&mut self) {
        if !self.popup.may_open() {
            return;
        }
        let Some(service) = self.service() else {
            kubuno_desktop::tracing::warn!("[WaffleButton] no LauncherService: set_default_launcher or set_service first");
            return;
        };
        let owner = kubuno_desktop::popup::current_window();
        let content = waffle_menu::content_height(&service.apps(), &service.favorites(), false);
        let anchor = PopupAnchor::parse(&self.popup_anchor);
        let size = (waffle_menu::WIDTH, content.min(MAX_HEIGHT));
        let Some(spot) = header_popup::spot(owner, anchor, self.bounds(), size, self.popup_offset, self.popup_margin, self.popup_bottom_gap) else { return };
        // Outside the edit mode at most 580; editing takes the room it needs, so both zones show while dragging.
        let fit = move |content: f32, editing: bool| content.min(if editing { spot.room } else { MAX_HEIGHT.min(spot.room) });

        let menu = Custom::<WaffleMenu>::new().dock(DockStyle::Fill).name("menu");
        let popup = Popup::new(&menu, waffle_menu::WIDTH, fit(content, false)).back_color(&header_popup::tint()).title(ShellControlsResources::launcher_apps());
        let form: Form = popup.form().clone();
        // The menu's data, once its window is open.
        {
            let (menu, service) = (menu.clone(), service.clone());
            form.load().subscribe(move |_form, _e| {
                menu.with(|m| m.set_service(service.clone()));
            });
        }
        // An app opened (by the service): the panel's work is done.
        {
            let form = form.clone();
            menu.on::<AppEventArgs>("OnAppLaunched").subscribe(move |_menu, _e| form.close());
        }
        // The panel follows the menu's height, and the edit mode's cap.
        let (content, editing) = (Rc::new(Cell::new(content)), Rc::new(Cell::new(false)));
        {
            let (form, content, editing) = (form.clone(), content.clone(), editing.clone());
            menu.on::<EditModeEventArgs>("OnEditModeChanged").subscribe(move |_menu, e| {
                editing.set(e.editing);
                form.set_client_size(waffle_menu::WIDTH, fit(content.get(), e.editing));
            });
        }
        {
            let (form, content, editing) = (form.clone(), content.clone(), editing.clone());
            menu.on::<ContentHeightEventArgs>("OnContentHeightChanged").subscribe(move |_menu, e| {
                content.set(e.height);
                form.set_client_size(waffle_menu::WIDTH, fit(e.height, editing.get()));
            });
        }
        // Escape abandons an edit first, as on the web; then closes.
        let escape_menu = menu.clone();
        let popup = popup.on_escape(move || escape_menu.with(|m| if m.editing { m.escape(); false } else { true }).unwrap_or(true));
        self.popup.track(&form);
        popup.show_at(owner, spot.x, spot.y);
        self.raise_popup_opened(EmptyEventArgs);
    }
}

#[kubuno_desktop::views::event_handlers]
impl WaffleButton {
    fn button_click(&mut self) {
        self.open();
    }
}
