//! Code-behind of the confirmation dialog (`confirm_dialog.kbview`), shown inside the window over
//! a veil (`View::show_in_window`): the dialog only answers yes or no; what a « yes » runs is the
//! [`ConfirmAction`] the window keeps, applied when the dialog closes with `DialogResult::Ok`.

use kubuno::prelude::*;

use crate::Resources;

/// What a confirmed dialog runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    /// Delete a cross-module label on the server.
    DeleteLabel { instance: String, label_id: String },
    /// Disconnect an account from the accounts page: its credentials and sync state go, the
    /// downloaded files stay on disk.
    RemoveAccount { id: String },
    /// Sign out of the CURRENT account, from the account panel.
    Logout,
}

/// How serious a confirmation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Something is destroyed: a red disc and a red confirming action.
    Danger,
    /// Nothing is destroyed but something stops: an amber disc.
    Warning,
}

/// The words of one confirmation.
#[derive(Debug, Clone, PartialEq)]
pub struct Confirmation {
    pub title: String,
    pub message: String,
    pub confirm: String,
    pub severity: Severity,
}

impl Confirmation {
    /// « Supprimer l'étiquette » — the danger variant: labels span every module, so removing one
    /// unlabels items the user may not have in view.
    pub fn delete_label(name: &str) -> Self {
        Self {
            title: Resources::delete_label_title().to_string(),
            message: Resources::delete_label_text().replace("{0}", name),
            confirm: Resources::delete().to_string(),
            severity: Severity::Danger,
        }
    }

    /// Disconnecting an account — a warning: nothing is deleted on the server and the local files
    /// stay, but the sync stops.
    pub fn disconnect(account: &str) -> Self {
        Self {
            title: Resources::disconnect_title().to_string(),
            message: Resources::disconnect_text().replace("{0}", account),
            confirm: Resources::disconnect().to_string(),
            severity: Severity::Warning,
        }
    }
}

/// The confirmation dialog (see the module doc).
#[kubuno::view("confirm_dialog.kbview")]
pub struct ConfirmDialog {}

impl ConfirmDialog {
    pub fn new(c: &Confirmation) -> Self {
        let mut dialog = Self::default();
        dialog.initialize_component();
        dialog.set_text(c.title.clone());
        dialog.message.set_text(c.message.clone());
        dialog.confirm.set_text(c.confirm.clone());
        let (glyph, disc, variant) = match c.severity {
            Severity::Danger => ("Trash2", "Danger", "Danger"),
            Severity::Warning => ("AlertTriangle", "Warning", "Primary"),
        };
        dialog.glyph.set_property("Name", glyph);
        dialog.glyph.set_property("Disc", disc);
        dialog.confirm.set_property("Variant", variant);
        dialog
    }

    fn cancel_click(&mut self) {
        self.set_dialog_result(DialogResult::Cancel);
        self.close();
    }

    fn confirm_click(&mut self) {
        self.set_dialog_result(DialogResult::Ok);
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmations_say_what_happens() {
        kubuno::resources::set_culture("fr");
        let c = Confirmation::delete_label("Urgent");
        assert_eq!(c.title, "Supprimer l'étiquette");
        assert!(c.message.contains("« Urgent »"));
        assert_eq!((c.confirm.as_str(), c.severity), ("Supprimer", Severity::Danger));
        let c = Confirmation::disconnect("cloud.exemple.fr");
        assert_eq!((c.confirm.as_str(), c.severity), ("Déconnecter", Severity::Warning));
    }

    #[test]
    fn the_dialog_shows_the_confirmation() {
        kubuno::resources::set_culture("fr");
        let d = ConfirmDialog::new(&Confirmation::disconnect("cloud.exemple.fr"));
        assert_eq!(d.get_text(), "Déconnecter le compte");
        assert!(d.message.get_text().contains("cloud.exemple.fr"));
        assert_eq!(d.confirm.get_text(), "Déconnecter");
    }
}
