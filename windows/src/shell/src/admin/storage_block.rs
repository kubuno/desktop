//! Code-behind of the user control `StorageBlock` (`storage_block.kbcontrol`, see its comment): one card of
//! the console's storage page, an item of its Repeater. Its `Kind` (`overview`, `states`, `units`,
//! `categories`) picks which of its parts show; the page hands it the figures already worded.

use kubuno_desktop::views::prelude::*;
use kubuno_desktop::{Row, Rows, Value};

/// One card of the storage page (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "storage_block.kbcontrol")]
#[category("Kubuno")]
pub struct StorageBlock {
    base: UserControlCore,
    /// `overview`, `states`, `units` or `categories`.
    #[property(bindable, on_change = "parts_changed")]
    #[category("Appearance")]
    pub kind: String,
    /// The card's title.
    #[property(bindable)]
    #[category("Data")]
    pub block_title: String,
    /// The card's subtitle (« seuil 90 % »).
    #[property(bindable)]
    #[category("Data")]
    pub block_subtitle: String,
    /// The overview's figure: the space the accounts use.
    #[property(bindable, on_change = "parts_changed")]
    #[category("Data")]
    pub hero: String,
    /// What it is measured against (« sur 3.64 To »).
    #[property(bindable, on_change = "parts_changed")]
    #[category("Data")]
    pub hero_of: String,
    /// The volume's composition (a `StackedBar`'s segments).
    #[property(bindable, on_change = "parts_changed")]
    #[category("Data")]
    pub volume_segments: String,
    #[property(bindable)]
    #[category("Data")]
    pub volume_total: f32,
    /// The volume's path, or why there is none.
    #[property(bindable)]
    #[category("Data")]
    pub volume_path: String,
    /// The number of accounts.
    #[property(bindable)]
    #[category("Data")]
    pub accounts: String,
    /// The space allocated to them.
    #[property(bindable)]
    #[category("Data")]
    pub allocated: String,
    /// The accounts' states (a `StackedBar`'s segments) and what they add up to.
    #[property(bindable)]
    #[category("Data")]
    pub state_segments: String,
    #[property(bindable)]
    #[category("Data")]
    pub state_total: f32,
    /// What the states mean.
    #[property(bindable)]
    #[category("Data")]
    pub states_text: String,
    /// The units (fields `Name`, `Accounts`, `Size`, `Share` out of 10000).
    #[property(bindable)]
    #[category("Data")]
    pub units: Rows,
    /// The categories (fields `Name`, `Billed`, `NotBilled`, `Share`, `Objects`, `Size`, `Ruled`).
    #[property(bindable)]
    #[category("Data")]
    pub categories: Rows,
    /// The volume bar's accessible name (« 812.00 Go sur 3.64 To »).
    #[property(bindable)]
    #[browsable(false)]
    pub volume_name: String,
    #[property(bindable)]
    #[browsable(false)]
    pub is_overview: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub has_volume: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub is_states: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub is_units: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub is_categories: bool,
}

impl StorageBlock {
    fn parts_changed(&mut self) {
        let kind = if self.kind.is_empty() { "overview" } else { self.kind.as_str() };
        self.is_overview = kind == "overview";
        self.is_states = kind == "states";
        self.is_units = kind == "units";
        self.is_categories = kind == "categories";
        self.has_volume = self.is_overview && !self.volume_segments.is_empty();
        self.volume_name = format!("{} {}", self.hero, self.hero_of).trim().to_string();
    }
}

#[kubuno_desktop::views::event_handlers]
impl StorageBlock {
    fn storage_block_load(&mut self) {
        if self.design_mode() && self.kind.is_empty() && self.hero.is_empty() {
            self.block_title = "Aperçu".into();
            self.hero = "812.00 Go".into();
            self.hero_of = "sur 3.64 To".into();
            self.volume_segments = "Primary=812;TextTertiary=310".into();
            self.volume_total = 3725.0;
            self.volume_path = "/var/lib/kubuno".into();
            self.accounts = "248".into();
            self.allocated = "2.42 To".into();
            self.units = Rows::from(vec![Row::new()
                .with("Name", Value::Str("Siège".into()))
                .with("Accounts", Value::Str(" · 120 compte(s)".into()))
                .with("Size", Value::Str("512.00 Go".into()))
                .with("Share", Value::F32(10_000.0))]);
        }
        self.parts_changed();
    }
}
