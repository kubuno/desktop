//! Code-behind of the user control `StatCard` (`stat_card.kbcontrol`, see its comment): one figure of the
//! console's dashboard.

use kubuno::views::prelude::*;

/// A share is a whole number out of this (the bar's `Maximum`): a hundredth of a percent.
pub const SHARE_SCALE: f32 = 10_000.0;

/// One figure of the dashboard (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "stat_card.kbcontrol")]
#[category("Kubuno")]
pub struct StatCard {
    base: UserControlCore,
    /// What is counted (the card's title).
    #[property(bindable)]
    #[category("Appearance")]
    pub label: String,
    /// The glyph at the end of the title (a Kubuno icon name).
    #[property(bindable)]
    #[category("Appearance")]
    pub glyph: String,
    /// The figure, already formatted.
    #[property(bindable)]
    #[category("Data")]
    pub value: String,
    /// The line under it; empty when the card shows a bar.
    #[property(bindable, on_change = "parts_changed")]
    #[category("Data")]
    pub sub: String,
    /// The bar's share, out of [`SHARE_SCALE`]; negative for no bar.
    #[property(bindable, on_change = "parts_changed")]
    #[category("Data")]
    pub share: f32,
    #[property(bindable)]
    #[browsable(false)]
    pub has_sub: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub has_bar: bool,
}

impl StatCard {
    fn parts_changed(&mut self) {
        self.has_bar = self.share >= 0.0;
        self.has_sub = !self.has_bar && !self.sub.is_empty();
    }
}

#[kubuno::views::event_handlers]
impl StatCard {
    fn stat_card_load(&mut self) {
        if self.design_mode() && self.label.is_empty() {
            self.label = "Utilisateurs total".into();
            self.glyph = "Users".into();
            self.value = "248".into();
            self.sub = "+6 cette semaine".into();
            self.share = -1.0;
        }
        self.parts_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_shows_its_bar_or_its_line() {
        let mut c = StatCard { sub: "+3".into(), share: -1.0, ..StatCard::default() };
        c.parts_changed();
        assert!(c.has_sub && !c.has_bar);
        c.share = 9_300.0;
        c.parts_changed();
        assert!(c.has_bar && !c.has_sub);
    }
}
