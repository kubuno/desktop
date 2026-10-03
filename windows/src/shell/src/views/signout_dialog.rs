//! Code-behind of the sign-out dialog (`signout_dialog.kbview`): shown when an account is signed out while
//! some of its changes were not sent yet (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §10, §18.5). It only answers;
//! the window runs the choice ([`crate::services::session::sign_out_instance`]) off the UI thread.
//!
//! The answer travels as a `DialogResult`: `Ok` = « Envoyer d'abord » (the default), `Yes` = « Exporter »,
//! `Ignore` = « Supprimer quand même », `Cancel` = keep the account.

use kubuno_desktop::prelude::*;

use crate::services::session::SignOutChoice;
use crate::Resources;

/// The choice a closed dialog carries (`None`: cancelled).
pub fn choice_of(result: DialogResult) -> Option<SignOutChoice> {
    match result {
        DialogResult::Ok => Some(SignOutChoice::SendFirst),
        DialogResult::Yes => Some(SignOutChoice::Export),
        DialogResult::Ignore => Some(SignOutChoice::Discard),
        _ => None,
    }
}

/// The message of the dialog: first asked, or asked again after « Envoyer d'abord » left changes behind.
pub fn message(unsent: u32, account: &str, retry: bool) -> String {
    let text = if retry { Resources::signout_still_unsent() } else { Resources::signout_text() };
    text.replace("{0}", &unsent.to_string()).replace("{1}", account)
}

/// The sign-out dialog (see the module doc).
#[kubuno_desktop::view("signout_dialog.kbview")]
pub struct SignOutDialog {}

impl SignOutDialog {
    pub fn new(unsent: u32, account: &str, retry: bool) -> Self {
        let mut dialog = Self::default();
        dialog.initialize_component();
        dialog.set_text(Resources::signout_title().to_string());
        dialog.message.set_text(message(unsent, account, retry));
        dialog
    }

    fn answer(&mut self, result: DialogResult) {
        self.set_dialog_result(result);
        self.close();
    }

    fn send_click(&mut self) {
        self.answer(DialogResult::Ok);
    }

    fn export_click(&mut self) {
        self.answer(DialogResult::Yes);
    }

    fn discard_click(&mut self) {
        self.answer(DialogResult::Ignore);
    }

    fn cancel_click(&mut self) {
        self.answer(DialogResult::Cancel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choices_map_to_dialog_results() {
        assert_eq!(choice_of(DialogResult::Ok), Some(SignOutChoice::SendFirst));
        assert_eq!(choice_of(DialogResult::Yes), Some(SignOutChoice::Export));
        assert_eq!(choice_of(DialogResult::Ignore), Some(SignOutChoice::Discard));
        assert_eq!(choice_of(DialogResult::Cancel), None);
        assert_eq!(choice_of(DialogResult::None), None, "closing the dialog keeps the account");
    }

    #[test]
    fn the_dialog_says_how_many_changes_wait() {
        kubuno_desktop::resources::set_culture("fr");
        let d = SignOutDialog::new(3, "cloud.exemple.fr", false);
        assert_eq!(d.get_text(), "Modifications non envoyées");
        assert!(d.message.get_text().starts_with("3 modification(s) de « cloud.exemple.fr »"));
        assert_eq!(Resources::signout_send(), "Envoyer d'abord");
        assert_eq!(Resources::signout_export(), "Exporter");
        assert_eq!(Resources::signout_discard(), "Supprimer quand même");
        assert!(message(2, "x", true).contains("n'ont pas pu être envoyées"));
    }
}
