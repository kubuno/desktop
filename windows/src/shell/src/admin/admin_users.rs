//! Code-behind of the user control `UsersSection` (`admin_users.kbcontrol`, see its comment): the
//! directory's users. [`load`] asks the server for a page (`GET /api/v1/admin/users`, off the UI
//! thread); [`UsersSection::show`] shows it. The table's own text draws the plain columns; this file
//! draws the user cell (a name over an address), the role and state badges, and the pencil.

use kubuno::ui::buttons::IconButton;
use kubuno::ui::display::{Badge, BadgeVariant};
use kubuno::ui::{Canvas, Rect, Widget, WidgetState};
use kubuno::views::component::Shared;
use kubuno::views::events::{CellEventArgs, DrawItemEventArgs, NumericValueChangedEventArgs, TextChangedEventArgs};
use kubuno::views::prelude::*;
use kubuno::{Row, Rows, Value};

use crate::admin::{SectionRequest, SectionState};
use crate::model::events::ItemCommandEventArgs;
use crate::Resources;

/// How many accounts a page holds (the API's `limit`).
pub const PAGE: u32 = 50;

/// The columns this section draws itself.
const USER: usize = 0;
const ROLE: usize = 2;
const STATUS: usize = 5;
const EDIT: usize = 6;
/// A body line over a meta line, centred in the row.
const LINE: f32 = 20.0;
const META_LINE: f32 = 16.0;
/// The pencil: a round button in its column.
const PENCIL: f32 = 32.0;
const PENCIL_GLYPH: f32 = 16.0;

/// One account as the table shows it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserRow {
    pub id: String,
    pub name: String,
    pub email: String,
    pub unit: String,
    pub role: Option<String>,
    pub active: bool,
    pub quota: String,
    pub last_login: String,
}

/// A page of accounts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsersData {
    pub users: Vec<UserRow>,
    pub total: i64,
    pub page: u32,
}

/// Who an account is: its display name, else its username, else « — ».
fn name_of(u: &kubuno_sync::AdminUser) -> String {
    u.display_name.as_deref().filter(|s| !s.trim().is_empty()).or(u.username.as_deref()).filter(|s| !s.trim().is_empty()).unwrap_or("—").to_string()
}

/// The role's wording, as the web writes it; an unknown role is shown raw.
pub fn role_label(role: Option<&str>) -> String {
    match role {
        Some("admin") => Resources::users_role_admin().to_string(),
        Some("user") => Resources::users_role_user().to_string(),
        Some("guest") => Resources::users_role_guest().to_string(),
        Some(other) if !other.is_empty() => other.to_string(),
        _ => "—".to_string(),
    }
}

/// The role badge's tone: an administrator stands out in danger, an account in the primary tone, a
/// guest or anything else in the quiet default.
pub fn role_variant(role: Option<&str>) -> BadgeVariant {
    match role {
        Some("admin") => BadgeVariant::Danger,
        Some("user") => BadgeVariant::Primary,
        _ => BadgeVariant::Default,
    }
}

fn status_label(active: bool) -> String {
    if active { Resources::users_active() } else { Resources::users_inactive() }.to_string()
}

/// Days since the civil date (Howard Hinnant's algorithm), for a relative date without a date library.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn parse_ymd(s: &str) -> Option<(i64, i64, i64)> {
    Some((s.get(0..4)?.parse().ok()?, s.get(5..7)?.parse().ok()?, s.get(8..10)?.parse().ok()?))
}

/// The web's « il y a N jours » / « Jamais », at a day's granularity, `today` in days since 1970.
pub fn last_login(s: Option<&str>, today: i64) -> String {
    match s.and_then(parse_ymd) {
        None => Resources::users_never().to_string(),
        Some((y, m, d)) => match today - days_from_civil(y, m, d) {
            n if n <= 0 => Resources::users_today().to_string(),
            1 => Resources::users_one_day().to_string(),
            n => Resources::users_days().replace("{0}", &n.to_string()),
        },
    }
}

fn today() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() / 86_400).unwrap_or(0) as i64
}

/// Reads page `request.page` of the accounts of instance `id` matching `request.query` (blocking: run it
/// off the UI thread), with their units' names.
pub fn load(id: &str, request: &SectionRequest) -> anyhow::Result<UsersData> {
    let page = crate::services::backend::admin_users(id, request.page * PAGE, PAGE, &request.query)?;
    let units: std::collections::HashMap<String, String> = crate::services::backend::admin_org_units(id).unwrap_or_default().into_iter().map(|u| (u.id, u.name)).collect();
    let today = today();
    let users = page
        .users
        .iter()
        .map(|u| UserRow {
            id: u.id.clone(),
            name: name_of(u),
            email: u.email.clone(),
            unit: u.org_unit_id.as_ref().and_then(|id| units.get(id)).cloned().unwrap_or_else(|| "—".into()),
            role: u.role.clone(),
            active: u.is_active,
            quota: format!("{} / {}", crate::model::view_model::format_size(u.used_bytes), crate::model::view_model::format_size(u.quota_bytes)),
            last_login: last_login(u.last_login_at.as_deref(), today),
        })
        .collect();
    Ok(UsersData { users, total: page.total, page: request.page })
}

/// The table's rows: what each column shows as text.
pub fn rows(users: &[UserRow]) -> Rows {
    Rows::from(
        users
            .iter()
            .map(|u| {
                Row::new()
                    .with("Id", Value::Str(u.id.clone()))
                    .with("Name", Value::Str(u.name.clone()))
                    .with("Unit", Value::Str(u.unit.clone()))
                    .with("Role", Value::Str(role_label(u.role.as_deref())))
                    .with("Quota", Value::Str(u.quota.clone()))
                    .with("LastLogin", Value::Str(u.last_login.clone()))
                    .with("Status", Value::Str(status_label(u.active)))
            })
            .collect::<Vec<_>>(),
    )
}

/// The directory's users (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "admin_users.kbcontrol")]
#[category("Kubuno")]
pub struct UsersSection {
    base: UserControlCore,
    /// What to show (bound by the console page).
    #[property(bindable, on_change = "state_changed")]
    #[category("Data")]
    pub state: Shared<SectionState<UsersData>>,
    /// Occurs when the section asks for another page, a search, or the web console.
    #[event]
    #[category("Action")]
    pub command: Event<ItemCommandEventArgs>,
    #[property(bindable)]
    #[browsable(false)]
    pub rows: Rows,
    #[property(bindable)]
    #[browsable(false)]
    pub total: f32,
    #[property(bindable)]
    #[browsable(false)]
    pub page: f32,
    #[property(bindable)]
    #[browsable(false)]
    pub loading: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub error_text: String,
    #[property(bindable)]
    #[browsable(false)]
    pub count_text: String,
    /// The accounts shown (what the cells draw).
    users: Vec<UserRow>,
}

impl UsersSection {
    fn state_changed(&mut self) {
        let state = self.state.clone();
        self.loading = state.loading;
        self.error_text = state.error.clone();
        if let Some(data) = state.data.as_ref() {
            self.show(data);
        }
    }

    /// Shows a page of accounts.
    pub fn show(&mut self, data: &UsersData) {
        self.users = data.users.clone();
        self.rows = rows(&data.users);
        self.total = data.total as f32;
        self.page = data.page as f32;
        self.count_text = Resources::users_count().replace("{0}", &data.total.to_string());
    }

    fn raise(&mut self, command: &str, id: &str, value: &str) {
        self.raise_command(ItemCommandEventArgs::new(command, id, value));
    }
}

/// A badge at its own size, left in the cell and centred on its height.
fn draw_badge(c: &dyn Canvas, cell: Rect, badge: Badge) {
    let room = (cell.right - cell.left).max(0.0);
    let badge = badge.max_width(room);
    let size = badge.measure(c);
    let cy = (cell.top + cell.bottom) / 2.0;
    badge.paint(c, Rect::new(cell.left, cy - size.height / 2.0, cell.left + size.width.min(room), cy + size.height / 2.0), WidgetState::REST);
}

#[kubuno::views::event_handlers]
impl UsersSection {
    fn users_section_load(&mut self) {
        if self.design_mode() && self.users.is_empty() {
            self.show(&design_data());
        }
    }

    fn search_text_changed(&mut self, e: &TextChangedEventArgs) {
        let query = e.new.clone();
        self.raise("load", "query", &query);
    }

    fn new_user_click(&mut self) {
        self.raise("open-web", "", "/admin/users");
    }

    fn table_page_changed(&mut self, e: &NumericValueChangedEventArgs) {
        let page = (e.new.max(0.0) as u32).to_string();
        self.raise("load", "page", &page);
    }

    fn table_cell_click(&mut self, e: &CellEventArgs) {
        if e.column_index == EDIT {
            if let Some(id) = self.users.get(e.row_index).map(|u| u.id.clone()) {
                let path = format!("/admin/users/{id}");
                self.raise("open-web", &id, &path);
            }
        }
    }

    fn table_draw_item(&mut self, e: &mut DrawItemEventArgs) {
        let (Some(index), Some(column)) = (e.index, e.sub_index) else {
            e.draw_default = true;
            return;
        };
        let Some(user) = self.users.get(index).cloned() else {
            e.draw_default = true;
            return;
        };
        let cell = e.bounds;
        let c: &dyn Canvas = e.graphics();
        let t = c.theme().clone();
        let f = c.formats();
        match column {
            USER => {
                let top = cell.top + ((cell.bottom - cell.top) - LINE - META_LINE) / 2.0;
                c.text_ellipsis(&user.name, &Rect::new(cell.left, top, cell.right, top + LINE), &f.body, &t.text_primary);
                c.text_ellipsis(&user.email, &Rect::new(cell.left, top + LINE, cell.right, top + LINE + META_LINE), &f.caption, &t.text_tertiary);
            }
            ROLE => draw_badge(c, cell, Badge::new(role_label(user.role.as_deref())).variant(role_variant(user.role.as_deref()))),
            STATUS => draw_badge(c, cell, Badge::new(status_label(user.active)).variant(if user.active { BadgeVariant::Success } else { BadgeVariant::Default })),
            EDIT => {
                let cx = (cell.left + cell.right) / 2.0;
                let cy = (cell.top + cell.bottom) / 2.0;
                let r = Rect::new(cx - PENCIL / 2.0, cy - PENCIL / 2.0, cx + PENCIL / 2.0, cy + PENCIL / 2.0);
                IconButton::plain("PenLine", PENCIL, PENCIL_GLYPH).paint(c, r, WidgetState::REST);
            }
            _ => e.draw_default = true,
        }
    }
}

/// What the designer shows: a few of the sample instance's accounts.
pub fn design_data() -> UsersData {
    let user = |name: &str, unit: &str, role: &str, active: bool| UserRow {
        id: name.to_lowercase().replace(' ', "."),
        name: name.into(),
        email: format!("{}@exemple.fr", name.to_lowercase().replace(' ', ".")),
        unit: unit.into(),
        role: Some(role.into()),
        active,
        quota: "3.61 Go / 10.00 Go".into(),
        last_login: "il y a 4 jours".into(),
    };
    UsersData {
        users: vec![
            user("Camille Martin", "Direction", "admin", true),
            user("Alex Martin", "Ressources humaines", "user", true),
            user("Sacha Martin", "Informatique", "user", true),
            user("Charlie Martin", "Direction", "guest", false),
        ],
        total: 248,
        page: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_login_reads_like_the_web() {
        let today = days_from_civil(2026, 10, 1);
        assert_eq!(last_login(None, today), Resources::users_never());
        assert_eq!(last_login(Some("2026-10-01T08:00:00Z"), today), Resources::users_today());
        assert_eq!(last_login(Some("2026-09-30"), today), Resources::users_one_day());
        assert_eq!(last_login(Some("2026-09-21"), today), Resources::users_days().replace("{0}", "10"));
    }

    #[test]
    fn roles_carry_the_webs_tones() {
        assert_eq!(role_variant(Some("admin")), BadgeVariant::Danger);
        assert_eq!(role_variant(Some("user")), BadgeVariant::Primary);
        assert_eq!(role_variant(Some("guest")), BadgeVariant::Default);
        assert_eq!(role_label(Some("auditor")), "auditor");
        assert_eq!(role_label(None), "—");
    }

    #[test]
    fn a_page_shows_its_rows_and_its_count() {
        let mut s = UsersSection::default();
        s.show(&design_data());
        assert_eq!(s.rows.len(), 4);
        assert_eq!(s.total, 248.0);
        assert_eq!(s.count_text, Resources::users_count().replace("{0}", "248"));
        assert_eq!(s.rows.first().map(|r| r.text("Status")), Some(Resources::users_active().to_string()));
    }
}
