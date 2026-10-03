//! The header's menus and their parts: the user controls [`waffle_menu::WaffleMenu`] (the app
//! launcher) and [`account_menu::AccountMenu`] (the account panel), the header buttons that open them
//! in a popup of their own ([`waffle_button::WaffleButton`], [`account_button::AccountButton`], sharing
//! [`header_popup`]), and the custom controls the menus place — [`app_tile_grid::AppTileGrid`],
//! [`panel_menu::PanelMenu`] and [`panel_menu::AccentPill`]. `design/` holds the designer's sample data.

pub mod account_button;
pub mod accounts_card;
pub mod avatar_decor;
pub mod account_menu;
pub mod app_tile_grid;
pub mod header_actions;
pub mod header_popup;
pub mod panel_menu;
pub mod waffle_button;
pub mod waffle_menu;

#[cfg(test)]
mod view_tests {
    /// Every user control's view compiles at run time, with the custom controls it places (the
    /// derives check their bindings, not every attribute of a custom control).
    #[test]
    fn the_user_control_views_compile() {
        for text in [
            include_str!("waffle_menu.kbcontrol"),
            include_str!("account_menu.kbcontrol"),
            include_str!("waffle_button.kbcontrol"),
            include_str!("account_button.kbcontrol"),
            include_str!("header_actions.kbcontrol"),
        ] {
            if let Err(d) = kubuno_desktop::views::compile::compile(text) {
                panic!("{d:?}");
            }
        }
    }
}
