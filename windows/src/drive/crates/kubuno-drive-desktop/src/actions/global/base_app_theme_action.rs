//! set_theme (mirror of BaseAppThemeAction.cs)

use crate::main_window::MainWindow;

pub(super) fn set_theme(w: &mut MainWindow, theme: crate::services::settings::ThemeSetting) {
    crate::services::settings::update(|s| s.theme = theme);
    w.apply_appearance();
}
