//! Code-behind of the user control `AudiencesSection` (`admin_audiences.kbcontrol`, see its comment): the
//! directory's target audiences. [`load`] reads them (`GET /api/v1/admin/audiences`, off the UI
//! thread); the search filters them here (a handful of entries).

use kubuno::ui::buttons::IconButton;
use kubuno::ui::{Canvas, Rect, Widget, WidgetState};
use kubuno::views::component::Shared;
use kubuno::views::events::{CellEventArgs, DrawItemEventArgs, TextChangedEventArgs};
use kubuno::views::prelude::*;
use kubuno::{Row, Rows, Value};
use windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_TRAILING;

use crate::admin::SectionState;
use crate::model::events::ItemCommandEventArgs;
use crate::Resources;

const NAME: usize = 0;
const APPLIED: usize = 4;
const EDIT: usize = 5;
const GLOBE: f32 = 14.0;
const PENCIL: f32 = 32.0;
const PENCIL_GLYPH: f32 = 16.0;

/// One audience.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudienceInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    /// The seeded « everyone » audience: no member list, everybody is in it.
    pub everyone: bool,
    pub members: i64,
    pub reach: i64,
    /// How many (unit × module) pairs offer it.
    pub applied: i64,
}

/// The audiences of the instance.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudiencesData {
    pub audiences: Vec<AudienceInfo>,
}

/// Reads the audiences of instance `id` (blocking: run it off the UI thread).
pub fn load(id: &str) -> anyhow::Result<AudiencesData> {
    let audiences = crate::services::backend::admin_audiences(id)?
        .into_iter()
        .map(|a| AudienceInfo {
            id: a.id,
            name: a.name,
            description: a.description.filter(|d| !d.trim().is_empty()).unwrap_or_else(|| "—".into()),
            everyone: a.is_everyone,
            members: a.member_count,
            reach: a.reach,
            applied: a.applied_to,
        })
        .collect();
    Ok(AudiencesData { audiences })
}

/// « nulle part », « 1 endroit », « 9 endroits ».
pub fn applied_text(n: i64) -> String {
    match n {
        0 => Resources::audiences_nowhere().to_string(),
        1 => Resources::audiences_place().replace("{0}", "1"),
        n => Resources::audiences_places().replace("{0}", &n.to_string()),
    }
}

/// The audiences whose name or description holds `query` (any case).
pub fn filter<'a>(audiences: &'a [AudienceInfo], query: &str) -> Vec<&'a AudienceInfo> {
    let q = query.trim().to_lowercase();
    audiences.iter().filter(|a| q.is_empty() || a.name.to_lowercase().contains(&q) || a.description.to_lowercase().contains(&q)).collect()
}

/// The table's rows.
pub fn rows(audiences: &[&AudienceInfo]) -> Rows {
    Rows::from(
        audiences
            .iter()
            .map(|a| {
                Row::new()
                    .with("Id", Value::Str(a.id.clone()))
                    .with("Name", Value::Str(a.name.clone()))
                    .with("Description", Value::Str(a.description.clone()))
                    // « everyone » has no member list: everybody is in it.
                    .with("Members", Value::Str(if a.everyone { "—".into() } else { a.members.to_string() }))
                    .with("Reach", Value::Str(a.reach.to_string()))
                    .with("Applied", Value::Str(applied_text(a.applied)))
            })
            .collect::<Vec<_>>(),
    )
}

/// The target audiences (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "admin_audiences.kbcontrol")]
#[category("Kubuno")]
pub struct AudiencesSection {
    base: UserControlCore,
    /// What to show (bound by the console page).
    #[property(bindable, on_change = "state_changed")]
    #[category("Data")]
    pub state: Shared<SectionState<AudiencesData>>,
    /// Occurs when the section asks for the web console.
    #[event]
    #[category("Action")]
    pub command: Event<ItemCommandEventArgs>,
    #[property(bindable)]
    #[browsable(false)]
    pub rows: Rows,
    #[property(bindable)]
    #[browsable(false)]
    pub count_text: String,
    #[property(bindable)]
    #[browsable(false)]
    pub loading: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub error_text: String,
    audiences: Vec<AudienceInfo>,
    /// The ones shown (the search's), in order.
    shown: Vec<AudienceInfo>,
    query: String,
}

impl AudiencesSection {
    fn state_changed(&mut self) {
        let state = self.state.clone();
        self.loading = state.loading;
        self.error_text = state.error.clone();
        if let Some(data) = state.data.as_ref() {
            self.show(data);
        }
    }

    /// Lists the audiences.
    pub fn show(&mut self, data: &AudiencesData) {
        self.audiences = data.audiences.clone();
        let n = self.audiences.len();
        self.count_text = if n == 1 { Resources::audiences_count_one() } else { Resources::audiences_count() }.replace("{0}", &n.to_string());
        self.refresh();
    }

    fn refresh(&mut self) {
        let shown = filter(&self.audiences, &self.query);
        self.rows = rows(&shown);
        self.shown = shown.into_iter().cloned().collect();
    }
}

#[kubuno::views::event_handlers]
impl AudiencesSection {
    fn audiences_section_load(&mut self) {
        if self.design_mode() && self.audiences.is_empty() {
            self.show(&design_data());
        }
    }

    fn search_text_changed(&mut self, e: &TextChangedEventArgs) {
        self.query = e.new.clone();
        self.refresh();
    }

    fn new_audience_click(&mut self) {
        self.raise_command(ItemCommandEventArgs::new("open-web", "", "/admin/audiences?action=create"));
    }

    fn table_cell_click(&mut self, e: &CellEventArgs) {
        if e.column_index == EDIT {
            if let Some(id) = self.shown.get(e.row_index).map(|a| a.id.clone()) {
                let path = format!("/admin/audiences/{id}");
                self.raise_command(ItemCommandEventArgs::new("open-web", &id, &path));
            }
        }
    }

    fn table_draw_item(&mut self, e: &mut DrawItemEventArgs) {
        let (Some(index), Some(column)) = (e.index, e.sub_index) else {
            e.draw_default = true;
            return;
        };
        let Some(a) = self.shown.get(index).cloned() else {
            e.draw_default = true;
            return;
        };
        let cell = e.bounds;
        let c: &dyn Canvas = e.graphics();
        let t = c.theme().clone();
        let f = c.formats();
        match column {
            // The « everyone » audience carries a globe before its name.
            NAME if a.everyone => {
                let cy = (cell.top + cell.bottom) / 2.0;
                let globe = Rect::new(cell.left, cy - GLOBE / 2.0, cell.left + GLOBE, cy + GLOBE / 2.0);
                c.vector_icon("Globe", &globe, GLOBE, &t.text_tertiary);
                c.text_ellipsis(&a.name, &Rect::new(globe.right + 4.0, cell.top, cell.right, cell.bottom), &f.body, &t.text_primary);
            }
            // Offered nowhere reads quieter than a real count.
            APPLIED if a.applied == 0 => c.text_aligned(&applied_text(0), &cell, &f.body, &t.text_tertiary, DWRITE_TEXT_ALIGNMENT_TRAILING),
            EDIT => {
                let cx = (cell.left + cell.right) / 2.0;
                let cy = (cell.top + cell.bottom) / 2.0;
                IconButton::plain("PenLine", PENCIL, PENCIL_GLYPH).paint(c, Rect::new(cx - PENCIL / 2.0, cy - PENCIL / 2.0, cx + PENCIL / 2.0, cy + PENCIL / 2.0), WidgetState::REST);
            }
            _ => e.draw_default = true,
        }
    }
}

/// What the designer shows: the sample instance's audiences.
pub fn design_data() -> AudiencesData {
    let a = |id: &str, name: &str, description: &str, everyone: bool, members: i64, reach: i64, applied: i64| AudienceInfo {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        everyone,
        members,
        reach,
        applied,
    };
    AudiencesData {
        audiences: vec![
            a("everyone", "Tout le monde", "Chaque compte actif de l'instance", true, 0, 231, 9),
            a("paris", "Équipe Paris", "Bureaux de Paris", false, 3, 64, 4),
            a("direction", "Direction", "—", false, 1, 8, 2),
            a("pilot", "Pilote Tableurs", "Testeurs de la nouvelle version", false, 6, 6, 0),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_search_filters_names_and_descriptions() {
        let d = design_data();
        assert_eq!(filter(&d.audiences, "").len(), 4);
        assert_eq!(filter(&d.audiences, "PARIS").len(), 1);
        assert_eq!(filter(&d.audiences, "testeurs").len(), 1);
        let r = rows(&filter(&d.audiences, ""));
        assert_eq!(r.first().map(|row| row.text("Members")), Some("—".to_string()));
        assert_eq!(applied_text(0), Resources::audiences_nowhere());
    }
}
