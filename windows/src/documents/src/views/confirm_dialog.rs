//! Code-behind of the confirmation dialog (`confirm_dialog.kbview`): a title, a message and two
//! answers. Never a system message box: the document's questions look like the rest of Kubuno.

use kubuno_desktop::prelude::*;

#[kubuno_desktop::view("confirm_dialog.kbview")]
pub struct ConfirmDialog {}

impl ConfirmDialog {
    pub fn new(title: &str, message: &str, confirm: &str, decline: &str) -> Self {
        let mut dialog = Self::default();
        dialog.initialize_component();
        dialog.set_text(title.to_string());
        dialog.message.set_text(message.to_string());
        dialog.confirm.set_text(confirm.to_string());
        dialog.cancel.set_text(decline.to_string());
        dialog
    }

    /// Shows it over `owner`; `true` when the first answer was chosen.
    pub fn ask(owner: &dyn AsForm, title: &str, message: &str, confirm: &str, decline: &str) -> bool {
        let mut d = Self::new(title, message, confirm, decline);
        d.show_dialog(owner) == DialogResult::Ok
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
