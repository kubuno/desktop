//! Code-behind of the link dialog (`link_dialog.kbview`): the address for the selection's link.

use kubuno::prelude::*;

#[kubuno::view("link_dialog.kbview")]
pub struct LinkDialog {
    #[bind]
    url: String,
}

impl LinkDialog {
    pub fn new(current: &str) -> Self {
        let mut dialog = Self { url: current.to_string(), ..Self::default() };
        dialog.initialize_component();
        dialog
    }

    /// Asks for an address; `None` when cancelled (an empty answer removes the link).
    pub fn ask(owner: &dyn AsForm, current: &str) -> Option<String> {
        let mut d = Self::new(current);
        (d.show_dialog(owner) == DialogResult::Ok).then(|| d.url.trim().to_string())
    }

    fn ok_click(&mut self) {
        self.set_dialog_result(DialogResult::Ok);
        self.close();
    }

    fn cancel_click(&mut self) {
        self.set_dialog_result(DialogResult::Cancel);
        self.close();
    }
}
