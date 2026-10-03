//! Code-behind of the user control `LoginPage` (`login_page.kbcontrol`): the sign-in form. It holds
//! the four fields; the window signs in (`Command`: `submit`, `browse`, `cancel`) and reads them
//! back with [`LoginPage::fields`], shows the attempt with [`LoginPage::set_busy`] /
//! [`LoginPage::set_error`], and a picked folder with [`LoginPage::set_folder`].

use kubuno_desktop::views::prelude::*;

use crate::model::events::CommandEventArgs;
use crate::Resources;

/// What the user typed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoginFields {
    pub server: String,
    pub login: String,
    pub password: String,
    pub folder: String,
    /// The two-factor code, once the server asked for it (`needs_code`).
    pub code: String,
    pub needs_code: bool,
}

impl LoginFields {
    /// Whether the form can be submitted at all — an empty field would only earn a round trip
    /// and a server error.
    pub fn is_complete(&self) -> bool {
        let server = self.server.trim();
        let secret = if self.needs_code { !self.code.trim().is_empty() } else { !self.password.is_empty() };
        !server.is_empty() && server != "https://" && !self.login.trim().is_empty() && secret && !self.folder.trim().is_empty()
    }
}

/// The sign-in form (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "login_page.kbcontrol", default_event = "Command")]
#[category("Kubuno")]
pub struct LoginPage {
    base: UserControlCore,
    #[property(bindable)]
    #[category("Data")]
    pub server: String,
    #[property(bindable)]
    #[category("Data")]
    pub login: String,
    #[property(bindable)]
    #[category("Data")]
    #[browsable(false)]
    pub password: String,
    #[property(bindable)]
    #[category("Data")]
    pub folder: String,
    /// The two-factor code (shown once the server asks for it, in place of the password).
    #[property(bindable)]
    #[category("Data")]
    pub code: String,
    /// The server asked for the two-factor code: the code row replaces the password row.
    #[property(bindable)]
    #[category("Behavior")]
    pub needs_code: bool,
    /// The password row is shown (`!needs_code`).
    #[property(bindable)]
    #[category("Behavior")]
    pub ask_password: bool,
    /// « Se connecter », or « Connexion… » while the attempt runs.
    #[property(bindable)]
    #[category("Appearance")]
    pub submit_text: String,
    /// The form is complete and no attempt is running.
    #[property(bindable)]
    #[category("Behavior")]
    pub can_submit: bool,
    /// Opened to add an account to an existing set: « Annuler » goes back.
    #[property(bindable)]
    #[category("Behavior")]
    pub cancellable: bool,
    /// Why the last attempt failed.
    #[property(bindable)]
    #[category("Data")]
    pub error: String,
    #[property(bindable)]
    #[category("Behavior")]
    pub has_error: bool,
    busy: bool,
    /// Occurs when the user asks for something: `submit`, `browse`, `cancel`.
    #[event]
    #[category("Action")]
    pub command: Event<CommandEventArgs>,
}

/// A sensible default folder, which the user can still change.
pub fn default_folder() -> String {
    std::env::var_os("USERPROFILE").map(|h| std::path::PathBuf::from(h).join("Kubuno").to_string_lossy().into_owned()).unwrap_or_default()
}

impl LoginPage {
    /// A fresh form: `https://` in the server field, the default folder.
    pub fn reset(&mut self, cancellable: bool) {
        self.server = "https://".into();
        self.login.clear();
        self.password.clear();
        self.folder = default_folder();
        self.code.clear();
        self.needs_code = false;
        self.ask_password = true;
        self.cancellable = cancellable;
        self.busy = false;
        self.set_error("");
        // The first field takes the caret, as the page always did.
        self.set_active_control(Some("server"));
    }

    /// What the user typed.
    pub fn fields(&self) -> LoginFields {
        LoginFields {
            server: self.server.clone(),
            login: self.login.clone(),
            password: self.password.clone(),
            folder: self.folder.clone(),
            code: self.code.clone(),
            needs_code: self.needs_code,
        }
    }

    /// The server asked for the two-factor code: the code row replaces the password row and takes the caret.
    pub fn ask_code(&mut self) {
        self.code.clear();
        self.needs_code = true;
        self.ask_password = false;
        self.busy = false;
        self.set_error("");
        self.set_active_control(Some("code"));
    }

    pub fn set_busy(&mut self, busy: bool) {
        self.busy = busy;
        if busy {
            self.set_error("");
        }
        self.update();
    }

    pub fn set_error(&mut self, error: &str) {
        self.error = error.to_string();
        self.has_error = !error.is_empty();
        self.update();
    }

    pub fn set_folder(&mut self, folder: &str) {
        self.folder = folder.to_string();
        self.update();
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    /// The submit button's text and state.
    fn update(&mut self) {
        self.submit_text = if self.busy {
            Resources::signing_in()
        } else if self.needs_code {
            Resources::login_verify()
        } else {
            Resources::sign_in()
        }
        .to_string();
        self.can_submit = self.fields().is_complete() && !self.busy;
    }
}

#[kubuno_desktop::views::event_handlers]
impl LoginPage {
    fn login_page_load(&mut self) {
        if self.server.is_empty() {
            self.reset(self.design_mode());
        }
        self.update();
    }

    fn field_text_changed(&mut self) {
        // Typing again clears the last failure.
        if self.has_error {
            self.set_error("");
        }
        self.update();
    }

    fn browse_click(&mut self) {
        self.raise_command(CommandEventArgs::new("browse"));
    }

    fn submit_click(&mut self) {
        if self.can_submit {
            self.raise_command(CommandEventArgs::new("submit"));
        }
    }

    fn cancel_click(&mut self) {
        self.raise_command(CommandEventArgs::new("cancel"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_form_must_be_complete() {
        let mut f = LoginFields { server: "https://".into(), login: "camille".into(), password: "x".into(), folder: "C:\\K".into(), ..LoginFields::default() };
        assert!(!f.is_complete(), "« https:// » alone is no server");
        f.server = "https://cloud.exemple.fr".into();
        assert!(f.is_complete());
        f.password.clear();
        assert!(!f.is_complete());
        // The second step needs the code, not the password.
        f.needs_code = true;
        assert!(!f.is_complete());
        f.code = "123456".into();
        assert!(f.is_complete());
    }
}
