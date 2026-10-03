//! Port of `Files.App/UserControls/` — the hand-painted controls. Each
//! sub-module mirrors an original UserControl and extends [`crate::ui::Painter`]
//! with that control's drawing methods.
//!
//! C# correspondence:
//! - `toolbar` ↔ `UserControls/Toolbar.xaml` (command bar)
//! - `navigation_toolbar` ↔ `UserControls/NavigationToolbar.xaml` (Omnibar)
//! - `tab_bar` ↔ `UserControls/TabBar/TabBar.xaml`
//! - `pane::info_pane` ↔ `UserControls/Pane/InfoPane.xaml`
//! - `status_bar` ↔ `UserControls/StatusBar.xaml`
//! - `status_center` ↔ `UserControls/StatusCenter/`
//! - `widgets` ↔ `UserControls/Widgets/`
//! - `layout_flyout` ↔ the `<Flyout>` of the `LayoutOptionsButton` (Toolbar.xaml)
//! - `flyout` / `flyout_window` ↔ the WinUI popups (MenuFlyout /
//!   CommandBarFlyout / PopupWindowSiteBridge) — infrastructure, not a
//!   UserControl
//! - `edit_box` ↔ the inline rename TextBox of the layout pages
//! - `scrollbar` ↔ the standard WinUI `ScrollBar` (not overridden by Files)

pub mod edit_box;
pub mod flyout;
pub mod flyout_window;
pub mod color_picker;
pub mod layout_flyout;
pub mod navigation_toolbar;
pub mod pane;
pub mod scrollbar;
pub mod status_bar;
pub mod status_center;
pub mod tab_bar;
pub mod tab_ghost;
pub mod toolbar;
pub mod widgets;
