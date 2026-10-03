//! Code-behind of the user control `StorageSection` (`admin_storage.kbcontrol`, see its comment): the
//! console's « Stockage » section, read only. [`load`] reads the storage overview
//! (`GET /api/v1/admin/storage/overview`, off the UI thread) and words it into four cards ([`blocks`]):
//! the overview, the accounts' states, the split by organisational unit (when the server sends one) and
//! the split by category. Nothing is invented: a figure the server does not send is left out.

use kubuno::ui::containers::Card;
use kubuno::views::component::Shared;
use kubuno::views::prelude::*;
use kubuno::{Row, Rows, Value};

use crate::admin::SectionState;
use crate::admin::admin_dashboard::group_digits;
use crate::model::events::ItemCommandEventArgs;
use crate::controls::stacked_bar::segments_text;
use crate::controls::status_presenter::StatusMode;
use crate::model::view_model::format_size;
use crate::Resources;

/// The overview card's body: the figure, the volume's bar and path, then the two figures under a rule.
pub const OVERVIEW_BODY_H: f32 = 138.0;
/// The states card's body: the bar over one line.
pub const STATES_BODY_H: f32 = 50.0;
/// One unit's row.
pub const UNIT_ROW_H: f32 = 44.0;
/// One category's row: its name over its badge.
pub const CATEGORY_ROW_H: f32 = 54.0;
/// A magnitude bar's value is out of this.
const SHARE_SCALE: f32 = 10_000.0;
/// The magnitude bars' widths (a row that holds something shows at least a round cap).
const UNIT_BAR_W: f32 = 902.0;
const CATEGORY_BAR_W: f32 = 434.0;
/// The bars' thickness.
const BAR_H: f32 = crate::controls::stacked_bar::BAR_H;

/// One unit's row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UnitLine {
    pub name: String,
    pub accounts: String,
    pub size: String,
    pub share: f32,
}

/// One category's row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CategoryLine {
    pub name: String,
    pub billed: bool,
    pub objects: String,
    pub size: String,
    pub share: f32,
}

/// One card, worded.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Block {
    pub kind: &'static str,
    pub title: String,
    pub subtitle: String,
    pub hero: String,
    pub hero_of: String,
    pub volume_segments: String,
    pub volume_total: f32,
    pub volume_path: String,
    pub accounts: String,
    pub allocated: String,
    pub state_segments: String,
    pub state_total: f32,
    pub states_text: String,
    pub units: Vec<UnitLine>,
    pub categories: Vec<CategoryLine>,
    /// The card's height: its chrome (the design system's `Card`) and its body.
    pub height: f32,
}

/// The storage page's cards.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StorageData {
    pub blocks: Vec<Block>,
}

/// How much a card titled `title` (with `subtitle`) takes around its body.
pub fn chrome(title: &str, subtitle: &str) -> f32 {
    let mut card = Card::titled(title);
    if !subtitle.is_empty() {
        card = card.with_subtitle(subtitle);
    }
    let card = card.on_layer();
    let probe = kubuno::ui::Rect::new(0.0, 0.0, 400.0, 1000.0);
    let body = card.body_rect(probe);
    (body.top - probe.top) + (probe.bottom - body.bottom)
}

/// A row's share of the largest, out of [`SHARE_SCALE`], lifted to a round cap when it holds something.
fn share(used: u64, max: u64, bar_w: f32) -> f32 {
    if max == 0 || used == 0 {
        return 0.0;
    }
    let fraction = (used as f64 / max as f64).clamp(0.0, 1.0) as f32;
    (fraction.max(BAR_H / bar_w) * SHARE_SCALE).round()
}

/// A readable name for a category id (the raw id for those this build does not know, as the web).
pub fn category_label(id: &str) -> String {
    match id {
        "content" => Resources::storage_cat_content(),
        "trash" => Resources::storage_cat_trash(),
        "versions" => Resources::storage_cat_versions(),
        "retention" => Resources::storage_cat_retention(),
        "thumbnails" => Resources::storage_cat_thumbnails(),
        "index" => Resources::storage_cat_index(),
        "cache" => Resources::storage_cat_cache(),
        "staging" => Resources::storage_cat_staging(),
        "system" => Resources::storage_cat_system(),
        "delegated" => Resources::storage_cat_delegated(),
        other => other,
    }
    .to_string()
}

/// The overview worded into its cards.
pub fn blocks(o: &kubuno_sync::StorageOverview) -> Vec<Block> {
    let mut out = Vec::new();

    let title = Resources::storage_overview().to_string();
    let mut overview = Block {
        kind: "overview",
        height: chrome(&title, "") + OVERVIEW_BODY_H,
        title,
        hero: format_size(o.used_bytes),
        accounts: group_digits(o.accounts),
        allocated: format_size(o.allocated_bytes),
        volume_path: Resources::storage_no_volume().to_string(),
        ..Block::default()
    };
    if let Some(v) = o.volume.as_ref() {
        overview.hero_of = Resources::storage_of().replace("{0}", &format_size(v.total_bytes));
        let accounts = o.used_bytes.min(v.used_bytes);
        let other = v.used_bytes.saturating_sub(o.used_bytes);
        overview.volume_segments = segments_text(&[("Primary", accounts as f64), ("TextTertiary", other as f64)]);
        overview.volume_total = v.total_bytes as f32;
        overview.volume_path = v.path.clone();
    }
    out.push(overview);

    let s = o.quota_states;
    let title = Resources::storage_states().to_string();
    let subtitle = Resources::storage_threshold().replace("{0}", &o.warn_percent.to_string());
    out.push(Block {
        kind: "states",
        height: chrome(&title, &subtitle) + STATES_BODY_H,
        title,
        subtitle,
        state_segments: segments_text(&[("Success", s.ok.max(0) as f64), ("Warning", s.near.max(0) as f64), ("Danger", s.full.max(0) as f64)]),
        state_total: o.accounts.max(1) as f32,
        states_text: if s.full == 0 && s.near == 0 {
            Resources::storage_all_ok().to_string()
        } else {
            Resources::storage_some_full().replace("{0}", &s.full.to_string()).replace("{1}", &s.near.to_string())
        },
        ..Block::default()
    });

    if !o.by_unit.is_empty() {
        let max = o.by_unit.iter().map(|u| u.used_bytes).max().unwrap_or(0).max(o.used_bytes);
        let title = Resources::storage_units().to_string();
        out.push(Block {
            kind: "units",
            height: chrome(&title, "") + o.by_unit.len() as f32 * UNIT_ROW_H,
            title,
            units: o
                .by_unit
                .iter()
                .map(|u| UnitLine {
                    name: u.unit_name.clone().unwrap_or_else(|| Resources::storage_no_unit().to_string()),
                    accounts: Resources::storage_unit_accounts().replace("{0}", &u.accounts.to_string()),
                    size: format_size(u.used_bytes),
                    share: share(u.used_bytes, max, UNIT_BAR_W),
                })
                .collect(),
            ..Block::default()
        });
    }

    // The categories that hold something, largest first.
    let mut cats: Vec<&kubuno_sync::StorageCategory> = o.categories.iter().filter(|c| c.used_bytes > 0).collect();
    cats.sort_by_key(|c| std::cmp::Reverse(c.used_bytes));
    let max = cats.first().map(|c| c.used_bytes).unwrap_or(0);
    let title = Resources::storage_categories().to_string();
    out.push(Block {
        kind: "categories",
        height: chrome(&title, "") + cats.len().max(1) as f32 * CATEGORY_ROW_H,
        title,
        categories: cats
            .iter()
            .map(|c| CategoryLine {
                name: category_label(&c.category),
                billed: c.billable,
                objects: Resources::storage_objects().replace("{0}", &group_digits(c.object_count)),
                size: format_size(c.used_bytes),
                share: share(c.used_bytes, max, CATEGORY_BAR_W),
            })
            .collect(),
        ..Block::default()
    });
    out
}

/// Reads the storage overview of instance `id` (blocking: run it off the UI thread).
pub fn load(id: &str) -> anyhow::Result<StorageData> {
    Ok(StorageData { blocks: blocks(&crate::services::backend::admin_storage(id)?) })
}

/// The cards as the Repeater's rows.
pub fn rows(blocks: &[Block]) -> Rows {
    Rows::from(
        blocks
            .iter()
            .map(|b| {
                let units = Rows::from(
                    b.units
                        .iter()
                        .map(|u| {
                            Row::new()
                                .with("Name", Value::Str(u.name.clone()))
                                .with("Accounts", Value::Str(u.accounts.clone()))
                                .with("Size", Value::Str(u.size.clone()))
                                .with("Share", Value::F32(u.share))
                        })
                        .collect::<Vec<_>>(),
                );
                let last = b.categories.len().saturating_sub(1);
                let categories = Rows::from(
                    b.categories
                        .iter()
                        .enumerate()
                        .map(|(i, c)| {
                            Row::new()
                                .with("Name", Value::Str(c.name.clone()))
                                .with("Billed", Value::Bool(c.billed))
                                .with("NotBilled", Value::Bool(!c.billed))
                                .with("Objects", Value::Str(c.objects.clone()))
                                .with("Size", Value::Str(c.size.clone()))
                                .with("Share", Value::F32(c.share))
                                .with("Ruled", Value::Bool(i < last))
                        })
                        .collect::<Vec<_>>(),
                );
                Row::new()
                    .with("Kind", Value::Str(b.kind.to_string()))
                    .with("Title", Value::Str(b.title.clone()))
                    .with("Subtitle", Value::Str(b.subtitle.clone()))
                    .with("Hero", Value::Str(b.hero.clone()))
                    .with("HeroOf", Value::Str(b.hero_of.clone()))
                    .with("VolumeSegments", Value::Str(b.volume_segments.clone()))
                    .with("VolumeTotal", Value::F32(b.volume_total))
                    .with("VolumePath", Value::Str(b.volume_path.clone()))
                    .with("Accounts", Value::Str(b.accounts.clone()))
                    .with("Allocated", Value::Str(b.allocated.clone()))
                    .with("StateSegments", Value::Str(b.state_segments.clone()))
                    .with("StateTotal", Value::F32(b.state_total))
                    .with("StatesText", Value::Str(b.states_text.clone()))
                    .with("Units", Value::List(units))
                    .with("Categories", Value::List(categories))
                    .with("Height", Value::F32(b.height))
            })
            .collect::<Vec<_>>(),
    )
}

/// The storage section (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "admin_storage.kbcontrol")]
#[category("Kubuno")]
pub struct StorageSection {
    base: UserControlCore,
    /// What to show (bound by the console page).
    #[property(bindable, on_change = "state_changed")]
    #[category("Data")]
    pub state: Shared<SectionState<StorageData>>,
    /// Occurs when the section asks the window for something (it asks for nothing: read only).
    #[event]
    #[category("Action")]
    pub command: Event<ItemCommandEventArgs>,
    #[property(bindable)]
    #[browsable(false)]
    pub blocks: Rows,
    #[property(bindable)]
    #[browsable(false)]
    pub status_mode: String,
    #[property(bindable)]
    #[browsable(false)]
    pub error_text: String,
}

impl StorageSection {
    fn state_changed(&mut self) {
        let state = self.state.clone();
        if let Some(data) = state.data.as_ref() {
            self.blocks = rows(&data.blocks);
        }
        self.error_text = state.error.clone();
        self.status_mode = StatusMode::of(!self.blocks.is_empty(), state.loading, &state.error).name().into();
    }
}

#[kubuno::views::event_handlers]
impl StorageSection {
    fn storage_section_load(&mut self) {
        if self.design_mode() && self.blocks.is_empty() {
            if let Ok(o) = crate::services::backend::sample_storage() {
                self.blocks = rows(&blocks(&o));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_overview_reads_as_four_cards() {
        let o = crate::services::backend::sample_storage().expect("sample");
        let b = blocks(&o);
        let kinds: Vec<&str> = b.iter().map(|b| b.kind).collect();
        assert_eq!(kinds, ["overview", "states", "units", "categories"]);
        // Largest first; the largest fills its bar, every other row shows at least a cap.
        assert_eq!(b[3].categories[0].share, SHARE_SCALE);
        assert!(b[3].categories.iter().all(|c| c.share > 0.0));
        // Each card is as tall as its chrome and its rows, the subtitled one a bit taller.
        assert_eq!(b[2].height, chrome(&b[2].title, "") + 5.0 * UNIT_ROW_H);
        assert!(chrome("x", "y") > chrome("x", ""));
        assert_eq!(rows(&b).len(), 4);
    }

    #[test]
    fn no_category_still_leaves_room_for_its_message() {
        let b = blocks(&kubuno_sync::StorageOverview::default());
        let kinds: Vec<&str> = b.iter().map(|b| b.kind).collect();
        assert_eq!(kinds, ["overview", "states", "categories"]);
        assert_eq!(b[2].height, chrome(&b[2].title, "") + CATEGORY_ROW_H);
        assert!(b[0].volume_segments.is_empty());
    }
}
