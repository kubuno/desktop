//! Code-behind of the user control `AdminSectionHeader` (`admin_section_header.kbcontrol`, see its
//! comment): the head of every section of the administration console — a breadcrumb, the title, an
//! inline count and an optional introduction, all set as properties by the section that uses it.

use kubuno_desktop::views::prelude::*;

/// How tall the header is: the breadcrumb (20), the title's line (40), and the introduction line with
/// its gap (4 + 22) when there is one.
pub fn height(intro: bool) -> f32 {
    20.0 + 40.0 + if intro { 26.0 } else { 0.0 }
}

/// A console section's header (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "admin_section_header.kbcontrol")]
#[category("Kubuno")]
pub struct AdminSectionHeader {
    base: UserControlCore,
    /// The breadcrumb's first segment (« Annuaire »); empty for no breadcrumb.
    #[property(bindable, on_change = "parts_changed")]
    #[category("Appearance")]
    pub parent: String,
    /// The section's title, also the breadcrumb's last segment.
    #[property(bindable)]
    #[category("Appearance")]
    pub title: String,
    /// What follows the title (« 5 groupes »); empty for nothing.
    #[property(bindable, on_change = "parts_changed")]
    #[category("Appearance")]
    pub count: String,
    /// One line under the title; empty for none.
    #[property(bindable, on_change = "parts_changed")]
    #[category("Appearance")]
    pub intro: String,
    #[property(bindable)]
    #[browsable(false)]
    pub has_trail: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub has_intro: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub has_count: bool,
}

impl AdminSectionHeader {
    fn parts_changed(&mut self) {
        self.has_trail = !self.parent.is_empty();
        self.has_intro = !self.intro.is_empty();
        self.has_count = !self.count.is_empty();
    }
}

#[kubuno_desktop::views::event_handlers]
impl AdminSectionHeader {
    fn admin_section_header_load(&mut self) {
        if self.design_mode() && self.title.is_empty() {
            self.parent = "Annuaire".into();
            self.title = "Utilisateurs".into();
            self.count = "248 utilisateurs".into();
        }
        self.parts_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trail_and_the_intro_show_when_they_are_set() {
        let mut h = AdminSectionHeader { parent: "Annuaire".into(), ..AdminSectionHeader::default() };
        h.parts_changed();
        assert!(h.has_trail && !h.has_intro);
        h.intro = "Réglages de l'instance — lecture seule.".into();
        h.parts_changed();
        assert!(h.has_intro);
        assert_eq!(height(false), 60.0);
        assert_eq!(height(true), 86.0);
    }
}
