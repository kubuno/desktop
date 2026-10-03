//! Code-behind of the user control `DashboardSection` (`admin_dashboard.kbcontrol`, see its comment): the
//! console's dashboard. [`load`] reads the server's statistics (`GET /api/v1/admin/stats`, off the UI
//! thread); [`DashboardSection::show`] shows them.

use kubuno::views::component::Shared;
use kubuno::views::prelude::*;

use crate::admin::SectionState;
use crate::controls::bar_chart::series_text;
use crate::controls::stat_card::SHARE_SCALE;
use crate::controls::status_presenter::StatusMode;
use crate::Resources;

/// What the dashboard shows, read from the statistics.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DashboardData {
    pub users_total: i64,
    pub users_active: i64,
    pub users_online: i64,
    pub sessions: i64,
    pub modules: i64,
    pub modules_healthy: i64,
    pub new_users_7d: i64,
    /// Logins per day, oldest first.
    pub logins: Vec<i64>,
    /// New accounts per day, oldest first.
    pub signups: Vec<i64>,
}

impl DashboardData {
    /// The figures of the statistics payload (absent keys count as 0; the healthy modules default
    /// to all of them).
    pub fn from_stats(stats: &serde_json::Value) -> Self {
        let num = |k: &str| stats.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
        let series = |k: &str| -> Vec<i64> {
            stats.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|d| d.get("count").and_then(|v| v.as_i64())).collect()).unwrap_or_default()
        };
        let modules = num("modules_active");
        let healthy = stats
            .get("modules_by_status")
            .and_then(|v| v.as_array())
            .and_then(|a| a.iter().find(|d| d.get("key").and_then(|k| k.as_str()) == Some("healthy")))
            .and_then(|d| d.get("count").and_then(|v| v.as_i64()))
            .unwrap_or(modules);
        Self {
            users_total: num("users_total"),
            users_active: num("users_active"),
            users_online: num("users_online"),
            sessions: num("sessions_active"),
            modules,
            modules_healthy: healthy,
            new_users_7d: num("new_users_7d"),
            logins: series("logins_daily"),
            signups: series("signups_daily"),
        }
    }
}

/// Reads the statistics of instance `id` (blocking: run it off the UI thread).
pub fn load(id: &str) -> anyhow::Result<DashboardData> {
    Ok(DashboardData::from_stats(&crate::services::backend::admin_stats(id)?))
}

/// Thousands separated by a narrow no-break space, as French figures are set (« 1 248 »).
pub fn group_digits(n: i64) -> String {
    let s = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('\u{202F}');
        }
        out.push(ch);
    }
    if n < 0 {
        format!("−{out}")
    } else {
        out
    }
}

/// The dashboard (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "admin_dashboard.kbcontrol")]
#[category("Kubuno")]
pub struct DashboardSection {
    base: UserControlCore,
    /// Occurs when the section asks the window for something (the dashboard asks for nothing yet).
    #[event]
    #[category("Action")]
    pub command: Event<crate::model::events::ItemCommandEventArgs>,
    /// What to show (bound by the console page).
    #[property(bindable, on_change = "state_changed")]
    #[category("Data")]
    pub state: Shared<SectionState<DashboardData>>,
    #[property(bindable)]
    #[browsable(false)]
    pub status_mode: String,
    #[property(bindable)]
    #[browsable(false)]
    pub error_text: String,
    #[property(bindable)]
    #[browsable(false)]
    pub has_stats: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub total: String,
    #[property(bindable)]
    #[browsable(false)]
    pub total_sub: String,
    #[property(bindable)]
    #[browsable(false)]
    pub active: String,
    #[property(bindable)]
    #[browsable(false)]
    pub active_share: f32,
    #[property(bindable)]
    #[browsable(false)]
    pub online: String,
    #[property(bindable)]
    #[browsable(false)]
    pub online_sub: String,
    #[property(bindable)]
    #[browsable(false)]
    pub modules: String,
    #[property(bindable)]
    #[browsable(false)]
    pub modules_sub: String,
    #[property(bindable)]
    #[browsable(false)]
    pub logins: String,
    #[property(bindable)]
    #[browsable(false)]
    pub signups: String,
}

impl DashboardSection {
    fn state_changed(&mut self) {
        let state = self.state.clone();
        if let Some(data) = state.data.as_ref() {
            self.show(data);
        } else if !state.error.is_empty() {
            self.show_error(&state.error);
        } else if state.loading {
            self.show_loading();
        }
    }

    /// Shows the statistics.
    pub fn show(&mut self, d: &DashboardData) {
        self.total = group_digits(d.users_total);
        self.total_sub = Resources::dash_new_this_week().replace("{0}", &d.new_users_7d.to_string());
        self.active = group_digits(d.users_active);
        // The share of active accounts as a bar (a high share is good news: it never turns amber).
        self.active_share = if d.users_total > 0 { (d.users_active as f32 / d.users_total as f32 * SHARE_SCALE).round() } else { 0.0 };
        self.online = group_digits(d.users_online);
        self.online_sub = Resources::dash_sessions().replace("{0}", &d.sessions.to_string());
        self.modules = group_digits(d.modules);
        self.modules_sub = Resources::dash_healthy().replace("{0}", &d.modules_healthy.to_string()).replace("{1}", &d.modules.to_string());
        self.logins = series_text(&d.logins);
        self.signups = series_text(&d.signups);
        self.has_stats = true;
        self.error_text.clear();
        self.status_mode = StatusMode::None.name().into();
    }

    /// The statistics are being read: the spinner, over whatever was shown.
    pub fn show_loading(&mut self) {
        if !self.has_stats {
            self.status_mode = StatusMode::Loading.name().into();
        }
    }

    /// Reading them failed.
    pub fn show_error(&mut self, message: &str) {
        self.has_stats = false;
        self.error_text = message.to_string();
        self.status_mode = StatusMode::Error.name().into();
    }
}

#[kubuno::views::event_handlers]
impl DashboardSection {
    fn dashboard_section_load(&mut self) {
        if self.design_mode() && !self.has_stats {
            self.show(&design_data());
        }
    }
}

/// What the designer shows: the sample instance's figures.
pub fn design_data() -> DashboardData {
    DashboardData {
        users_total: 248,
        users_active: 231,
        users_online: 37,
        sessions: 52,
        modules: 14,
        modules_healthy: 13,
        new_users_7d: 6,
        logins: vec![12, 10, 14, 15, 14, 3, 2, 13, 14, 15, 16, 14, 4, 2],
        signups: vec![1, 0, 2, 0, 1, 0, 0, 3, 1, 0, 2, 1, 0, 0],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_figures_come_from_the_statistics() {
        let stats = serde_json::json!({
            "users_total": 248, "users_active": 231, "users_online": 37, "sessions_active": 52,
            "modules_active": 14, "new_users_7d": 6,
            "modules_by_status": [{"key": "healthy", "count": 13}],
            "logins_daily": [{"count": 3}, {"count": 5}],
        });
        let d = DashboardData::from_stats(&stats);
        assert_eq!((d.users_total, d.users_active, d.modules_healthy), (248, 231, 13));
        assert_eq!(d.logins, [3, 5]);
        assert!(d.signups.is_empty());
        assert_eq!(DashboardData::from_stats(&serde_json::json!({"modules_active": 4})).modules_healthy, 4);
    }

    #[test]
    fn showing_them_fills_the_cards() {
        let mut s = DashboardSection::default();
        s.show(&design_data());
        assert!(s.has_stats);
        assert_eq!(s.total, "248");
        assert_eq!(s.active_share, 9315.0);
        assert_eq!(s.logins.split(',').count(), 14);
        s.show_error("boom");
        assert_eq!((s.status_mode.as_str(), s.has_stats), ("Error", false));
        assert_eq!(group_digits(1_234_567), "1\u{202F}234\u{202F}567");
    }
}
