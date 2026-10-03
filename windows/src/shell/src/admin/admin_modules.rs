//! Code-behind of the user control `ModulesSection` (`admin_modules.kbcontrol`, see its comment): the
//! installed modules. [`load`] reads them and the instance's default module
//! (`GET /api/v1/admin/modules`, off the UI thread); the ⋮ asks the window to switch a module's
//! service (`set-module`), which shows the change at once and confirms it with a reload.

use kubuno::ui::buttons::IconButton;
use kubuno::ui::display::{Badge, BadgeVariant};
use kubuno::ui::metrics::space;
use kubuno::ui::{Canvas, Rect, Widget, WidgetState};
use kubuno::views::component::Shared;
use kubuno::views::events::{CellEventArgs, DrawItemEventArgs, TextChangedEventArgs};
use kubuno::views::prelude::*;
use kubuno::{Row, Rows, Value};

use crate::admin::SectionState;
use crate::model::events::ItemCommandEventArgs;
use crate::Resources;

const APP: usize = 0;
const STATUS: usize = 1;
const MENU: usize = 2;
const LOGO: f32 = 28.0;
const LINE: f32 = 20.0;
const META_LINE: f32 = 16.0;
const BUTTON: f32 = 32.0;
const GLYPH: f32 = 16.0;

/// One installed module.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModuleInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    /// Its icon: its brand logo when it ships one, else its glyph.
    pub icon: &'static str,
    pub enabled: bool,
    /// The module the instance opens on.
    pub is_default: bool,
}

/// The installed modules.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModulesData {
    pub modules: Vec<ModuleInfo>,
}

/// A module's icon: its coloured brand logo when it ships one, else its glyph — the web launcher's rule.
fn icon_for(m: &kubuno_sync::AdminModule) -> &'static str {
    crate::services::apps::logo_for(&m.id).or_else(|| m.icon.as_deref().and_then(kubuno::views::icon::glyph)).unwrap_or("Package")
}

/// Reads the modules of instance `id` (blocking: run it off the UI thread).
pub fn load(id: &str) -> anyhow::Result<ModulesData> {
    let (modules, default) = crate::services::backend::admin_modules_and_default(id)?;
    // The default module is named by its route (`/drive`).
    let default = default.map(|d| d.trim_matches('/').split('/').next().unwrap_or_default().to_string());
    let mut modules: Vec<ModuleInfo> = modules
        .iter()
        .map(|m| ModuleInfo {
            id: m.id.clone(),
            name: m.display_name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| m.id.clone()),
            description: m.description.clone().unwrap_or_default(),
            version: m.version.clone().unwrap_or_default(),
            icon: icon_for(m),
            enabled: m.is_enabled,
            is_default: default.as_deref() == Some(m.id.as_str()),
        })
        .collect();
    modules.sort_by_key(|m| m.name.to_lowercase());
    Ok(ModulesData { modules })
}

fn status_label(enabled: bool) -> String {
    if enabled { Resources::modules_on() } else { Resources::modules_off() }.to_string()
}

/// The modules whose name or description holds `query`.
pub fn filter<'a>(modules: &'a [ModuleInfo], query: &str) -> Vec<&'a ModuleInfo> {
    let q = query.trim().to_lowercase();
    modules.iter().filter(|m| q.is_empty() || m.name.to_lowercase().contains(&q) || m.description.to_lowercase().contains(&q)).collect()
}

/// The table's rows.
pub fn rows(modules: &[&ModuleInfo]) -> Rows {
    Rows::from(
        modules
            .iter()
            .map(|m| Row::new().with("Id", Value::Str(m.id.clone())).with("Name", Value::Str(m.name.clone())).with("Status", Value::Str(status_label(m.enabled))))
            .collect::<Vec<_>>(),
    )
}

/// The installed modules (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "admin_modules.kbcontrol")]
#[category("Kubuno")]
pub struct ModulesSection {
    base: UserControlCore,
    /// What to show (bound by the console page).
    #[property(bindable, on_change = "state_changed")]
    #[category("Data")]
    pub state: Shared<SectionState<ModulesData>>,
    /// Occurs when the section asks the window to switch a module, or for the web console.
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
    modules: Vec<ModuleInfo>,
    /// The ones shown (the filter's), in the table's rows' order.
    shown: Vec<ModuleInfo>,
    query: String,
}

impl ModulesSection {
    fn state_changed(&mut self) {
        let state = self.state.clone();
        self.loading = state.loading;
        self.error_text = state.error.clone();
        if let Some(data) = state.data.as_ref() {
            self.show(data);
        }
    }

    /// Lists the modules.
    pub fn show(&mut self, data: &ModulesData) {
        self.modules = data.modules.clone();
        let n = self.modules.len();
        self.count_text = if n == 1 { Resources::modules_count_one() } else { Resources::modules_count() }.replace("{0}", &n.to_string());
        self.refresh();
    }

    fn refresh(&mut self) {
        let shown = filter(&self.modules, &self.query);
        self.rows = rows(&shown);
        self.shown = shown.into_iter().cloned().collect();
    }

    /// Switches module `id`'s service (shown at once; the window confirms it).
    fn flip(&mut self, index: usize) {
        let Some(m) = self.shown.get(index).cloned() else { return };
        let next = !m.enabled;
        if let Some(own) = self.modules.iter_mut().find(|x| x.id == m.id) {
            own.enabled = next;
        }
        self.refresh();
        self.raise_command(ItemCommandEventArgs::new("set-module", &m.id, if next { "true" } else { "false" }));
    }
}

#[kubuno::views::event_handlers]
impl ModulesSection {
    fn modules_section_load(&mut self) {
        if self.design_mode() && self.modules.is_empty() {
            self.show(&design_data());
        }
    }

    fn marketplace_click(&mut self) {
        self.raise_command(ItemCommandEventArgs::new("open-web", "", "/admin/marketplace"));
    }

    fn filter_text_changed(&mut self, e: &TextChangedEventArgs) {
        self.query = e.new.clone();
        self.refresh();
    }

    fn table_cell_click(&mut self, e: &CellEventArgs) {
        if e.column_index == MENU {
            self.flip(e.row_index);
        }
    }

    fn table_draw_item(&mut self, e: &mut DrawItemEventArgs) {
        let (Some(index), Some(column)) = (e.index, e.sub_index) else {
            e.draw_default = true;
            return;
        };
        let Some(m) = self.shown.get(index).cloned() else {
            e.draw_default = true;
            return;
        };
        let cell = e.bounds;
        let c: &dyn Canvas = e.graphics();
        let t = c.theme().clone();
        let f = c.formats();
        let cy = (cell.top + cell.bottom) / 2.0;
        match column {
            APP => {
                // The logo, then the name, its version and « Par défaut » over the description — dimmed
                // while the service is off.
                let off = !m.enabled;
                let logo = Rect::new(cell.left, cy - LOGO / 2.0, cell.left + LOGO, cy + LOGO / 2.0);
                c.vector_icon(m.icon, &logo, LOGO, if off { &t.text_tertiary } else { &t.text_secondary });
                let left = logo.right + space::MD;
                let stack = if m.description.is_empty() { LINE } else { LINE + META_LINE };
                let top = cell.top + ((cell.bottom - cell.top) - stack) / 2.0;
                let name_w = (c.measure(&m.name, &f.body) + 6.0).min((cell.right - left).max(0.0));
                c.text_ellipsis(&m.name, &Rect::new(left, top, left + name_w, top + LINE), &f.body, if off { &t.text_tertiary } else { &t.text_primary });
                let mut x = left + name_w + space::SM;
                if !m.version.is_empty() {
                    let label = format!("v{}", m.version);
                    let w = c.measure(&label, &f.caption).min((cell.right - x).max(0.0));
                    c.text_ellipsis(&label, &Rect::new(x, top, x + w, top + LINE), &f.caption, &t.text_tertiary);
                    x += w + space::SM;
                }
                if m.is_default {
                    let badge = Badge::new(Resources::modules_default()).variant(BadgeVariant::Primary);
                    let size = badge.measure(c);
                    let right = (x + size.width).min(cell.right);
                    if right > x + 8.0 {
                        let mid = top + LINE / 2.0;
                        badge.paint(c, Rect::new(x, mid - size.height / 2.0, right, mid + size.height / 2.0), WidgetState::REST);
                    }
                }
                if !m.description.is_empty() {
                    c.text_ellipsis(&m.description, &Rect::new(left, top + LINE, cell.right, top + LINE + META_LINE), &f.caption, &t.text_tertiary);
                }
            }
            STATUS => {
                // A dot and the words, as one badge: green while the service runs.
                let room = (cell.right - cell.left).max(0.0);
                let badge = Badge::new(status_label(m.enabled)).variant(if m.enabled { BadgeVariant::Success } else { BadgeVariant::Default }).dot(true).max_width(room);
                let size = badge.measure(c);
                badge.paint(c, Rect::new(cell.left, cy - size.height / 2.0, cell.left + size.width.min(room), cy + size.height / 2.0), WidgetState::REST);
            }
            MENU => {
                let cx = (cell.left + cell.right) / 2.0;
                IconButton::plain("MoreVertical", BUTTON, GLYPH).paint(c, Rect::new(cx - BUTTON / 2.0, cy - BUTTON / 2.0, cx + BUTTON / 2.0, cy + BUTTON / 2.0), WidgetState::REST);
            }
            _ => e.draw_default = true,
        }
    }
}

/// What the designer shows: a few of the sample instance's modules.
pub fn design_data() -> ModulesData {
    let m = |id: &str, name: &str, description: &str, icon: &'static str, enabled: bool, is_default: bool| ModuleInfo {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        version: "0.9.4".into(),
        icon,
        enabled,
        is_default,
    };
    ModulesData {
        modules: vec![
            m("drive", "Drive", "Fichiers, partage et synchronisation", "DriveLogo", true, true),
            m("mail", "Courrier", "Messagerie IMAP/SMTP", "MailLogo", true, false),
            m("tasks", "Tâches", "Listes et tableaux", "ListChecks", false, false),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_switches_a_module_and_the_filter_keeps_its_rows() {
        let mut s = ModulesSection::default();
        s.show(&design_data());
        assert_eq!(s.rows.len(), 3);
        s.query = "messagerie".into();
        s.refresh();
        assert_eq!(s.shown.len(), 1);
        s.flip(0);
        assert!(!s.modules.iter().find(|m| m.id == "mail").is_some_and(|m| m.enabled));
        assert_eq!(s.rows.first().map(|r| r.text("Status")), Some(status_label(false)));
    }
}
