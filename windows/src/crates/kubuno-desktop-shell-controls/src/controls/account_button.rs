//! Code-behind of the user control `AccountButton` (`account_button.kbcontrol`, see its comment): the
//! header's avatar, which opens the account panel ([`crate::AccountMenu`]) in a
//! [`kubuno_desktop::popup::Popup`] — a floating window of its own, so the panel may extend beyond a small
//! app window.
//!
//! Drop it in a view's header and give it the app's [`AccountService`] — once for every button of the
//! thread with [`set_default_accounts`], or per button with [`AccountButton::set_service`]. The service
//! says who is signed in, lists the other accounts, and hears what the user picks
//! ([`AccountService::act`]); the panel then closes, as it does on Escape, on its close button and on a
//! click outside. The avatar's look follows `UserName` / `Initials` / `AvatarPath` (bindable), which
//! the service fills when they are not bound.

use std::cell::RefCell;
use std::rc::Rc;

use kubuno_desktop::popup::Popup;
use kubuno_desktop::prelude::Custom;
use kubuno_desktop::views::prelude::*;
use kubuno_desktop::{DockStyle, Form};

use crate::controls::account_menu::{self, AccountEventArgs, AccountMenu};
use crate::controls::header_popup::{self, PopupAnchor, PopupState};
use crate::model::account::{AccountAction, AccountService};
use crate::ShellControlsResources;

thread_local! {
    static DEFAULT_ACCOUNTS: RefCell<Option<Rc<dyn AccountService>>> = const { RefCell::new(None) };
    /// Bumped by every [`set_default_accounts`]: the buttons that took their look from the thread's
    /// service take it again when they next paint.
    static GENERATION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The service every [`AccountButton`] of this UI thread uses unless it was given its own: an app
/// sets it once (and again when its accounts change).
pub fn set_default_accounts(service: Rc<dyn AccountService>) {
    DEFAULT_ACCOUNTS.with(|d| *d.borrow_mut() = Some(service));
    GENERATION.with(|g| g.set(g.get() + 1));
    kubuno_desktop::controls::host::request_repaint_after(1);
}

fn default_accounts() -> Option<Rc<dyn AccountService>> {
    DEFAULT_ACCOUNTS.with(|d| d.borrow().clone())
}

/// The header's avatar (see the module doc).
#[derive(UserControl)]
#[kubuno(overrides(Control))]
#[user_control(view = "account_button.kbcontrol")]
#[category("Kubuno")]
#[toolbox(icon = "circle-user")]
#[default_event("PopupOpened")]
pub struct AccountButton {
    base: UserControlCore,
    /// The name the avatar stands for (its accessible name).
    #[property(bindable)]
    #[category("Appearance")]
    pub user_name: String,
    /// What the avatar shows without a photo.
    #[property(bindable)]
    #[category("Appearance")]
    pub initials: String,
    /// The photo, a file on disk; empty for the initials.
    #[property(bindable)]
    #[category("Appearance")]
    pub avatar_path: String,
    /// The avatar's diameter: 36 in a 64-DIP header, 30 in a title bar (the caption buttons' size).
    #[property(bindable)]
    #[default_value(36.0)]
    #[category("Appearance")]
    pub avatar_size: f32,
    /// The avatar's tint: `Primary` (the accent, initials in white: on a light band) or `Accent` (the pale
    /// accent, initials in the accent: on an accent-coloured title band, where `Primary` would vanish).
    #[property(bindable)]
    #[default_value("Primary")]
    #[category("Appearance")]
    pub avatar_tint: String,
    /// What the panel hangs from: `Button` (under the avatar, right edges aligned) or `Window` (the
    /// window's top-right corner, as the shell's header does).
    #[property]
    #[default_value("Button")]
    #[category("Layout")]
    pub popup_anchor: String,
    /// `Button`: the gap between the avatar and the panel; `Window`: the panel's top, below the
    /// window's client top.
    #[property]
    #[default_value(4.0)]
    #[category("Layout")]
    pub popup_offset: f32,
    /// `Window`: the gap between the panel and the window's right edge.
    #[property]
    #[default_value(8.0)]
    #[category("Layout")]
    pub popup_margin: f32,
    /// The room kept free under the panel.
    #[property]
    #[default_value(16.0)]
    #[category("Layout")]
    pub popup_bottom_gap: f32,
    /// Occurs when the panel opens.
    #[event]
    #[category("Action")]
    pub popup_opened: Event<EmptyEventArgs>,
    service: Option<Rc<dyn AccountService>>,
    popup: PopupState,
    /// The look came from the service (not from bound properties), at this generation of the thread's.
    look_generation: Option<u64>,
}

impl Default for AccountButton {
    fn default() -> Self {
        Self {
            base: UserControlCore::default(),
            user_name: String::new(),
            initials: String::new(),
            avatar_path: String::new(),
            avatar_size: 36.0,
            avatar_tint: "Primary".into(),
            popup_anchor: "Button".into(),
            popup_offset: 4.0,
            popup_margin: 8.0,
            popup_bottom_gap: 16.0,
            popup_opened: Event::default(),
            service: None,
            popup: PopupState::default(),
            look_generation: None,
        }
    }
}

impl AccountButton {
    /// This button's own service (else the thread's [`set_default_accounts`]); the avatar takes the
    /// user's look from it unless its properties are bound.
    pub fn set_service(&mut self, service: Rc<dyn AccountService>) {
        self.service = Some(service);
        self.take_look();
    }

    fn service(&self) -> Option<Rc<dyn AccountService>> {
        self.service.clone().or_else(default_accounts)
    }

    /// The avatar's look from the service, unless its properties were set (bound or written): taken
    /// again whenever the thread's service changes ([`set_default_accounts`]) or [`Self::set_service`] runs.
    fn take_look(&mut self) {
        let from_service = self.look_generation.is_some();
        if !from_service && (!self.user_name.is_empty() || !self.initials.is_empty()) {
            return;
        }
        if let Some(user) = self.service().map(|s| s.user()) {
            self.user_name = user.name;
            self.initials = user.initials;
            self.avatar_path = user.avatar.unwrap_or_default();
            self.look_generation = Some(GENERATION.with(|g| g.get()));
        }
    }

    /// Takes the look again when the thread's service changed since.
    fn follow_service(&mut self) {
        if self.service.is_none() && self.look_generation.is_some_and(|g| g != GENERATION.with(|c| c.get())) {
            self.take_look();
        }
    }

    /// Opens the account panel under the avatar (see the module doc); nothing when it is open.
    pub fn open(&mut self) {
        if !self.popup.may_open() {
            return;
        }
        let Some(service) = self.service() else {
            kubuno_desktop::tracing::warn!("[AccountButton] no AccountService: set_default_accounts or set_service first");
            return;
        };
        let owner = kubuno_desktop::popup::current_window();
        let content = account_menu::content_height(&service.accounts(), true, service.can_administer());
        let anchor = PopupAnchor::parse(&self.popup_anchor);
        let Some(spot) = header_popup::spot(owner, anchor, self.bounds(), (account_menu::WIDTH, content), self.popup_offset, self.popup_margin, self.popup_bottom_gap) else { return };

        let menu = Custom::<AccountMenu>::new().dock(DockStyle::Fill).name("menu");
        let popup = Popup::new(&menu, account_menu::WIDTH, content.min(spot.room)).back_color(&header_popup::tint()).title(ShellControlsResources::account_tip());
        let form: Form = popup.form().clone();
        {
            let (menu, service) = (menu.clone(), service.clone());
            form.load().subscribe(move |_form, _e| {
                menu.with(|m| m.set_service(service.clone()));
            });
        }
        // Every pick goes to the service, then the panel closes.
        let pick = |event: &'static str, action: fn(&AccountEventArgs) -> AccountAction| {
            let (form, service) = (form.clone(), service.clone());
            menu.on::<AccountEventArgs>(event).subscribe(move |_menu, e| {
                service.act(action(e));
                form.close();
            });
        };
        pick("OnOpenAccount", |e| AccountAction::OpenAccount(e.id.clone()));
        pick("OnRemoveAccount", |e| AccountAction::RemoveAccount(e.id.clone()));
        let simple = |event: &'static str, action: AccountAction| {
            let (form, service) = (form.clone(), service.clone());
            menu.on::<EmptyEventArgs>(event).subscribe(move |_menu, _e| {
                service.act(action.clone());
                form.close();
            });
        };
        simple("OnManageAccount", AccountAction::ManageAccount);
        simple("OnAddAccount", AccountAction::AddAccount);
        simple("OnOpenLabels", AccountAction::OpenLabels);
        simple("OnOpenAdmin", AccountAction::OpenAdmin);
        simple("OnSignOut", AccountAction::SignOut);
        simple("OnChangeAvatar", AccountAction::ChangeAvatar);
        {
            let form = form.clone();
            menu.on::<EmptyEventArgs>("OnCloseRequested").subscribe(move |_menu, _e| form.close());
        }
        // The panel follows the accounts' card folding and unfolding.
        {
            let (form, room) = (form.clone(), spot.room);
            menu.on::<crate::controls::waffle_menu::ContentHeightEventArgs>("OnContentHeightChanged").subscribe(move |_menu, e| {
                form.set_client_size(account_menu::WIDTH, e.height.min(room));
            });
        }
        self.popup.track(&form);
        popup.show_at(owner, spot.x, spot.y);
        self.raise_popup_opened(EmptyEventArgs);
    }
}

impl Control for AccountButton {
    /// Follows the thread's service before painting (see [`set_default_accounts`]).
    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        self.follow_service();
        self.base_mut().on_paint(e);
    }
}

#[kubuno_desktop::views::event_handlers]
impl AccountButton {
    fn account_button_load(&mut self) {
        if self.design_mode() && self.initials.is_empty() {
            let (user, _, _) = account_menu::design_data();
            self.user_name = user.name;
            self.initials = user.initials;
        }
        self.take_look();
    }

    fn avatar_click(&mut self) {
        self.open();
    }
}
