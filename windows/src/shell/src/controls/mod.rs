//! The building blocks the views place: the custom-drawn controls (`StatusDot`, `StorageGauge`,
//! `BarChart`, `StackedBar`, `OrgUnitTree`) and the user controls used as plain elements by several
//! views (`StatusPresenter`, `StatCard`). The header's menus (`WaffleMenu`, `AccountMenu`, with their
//! `AppTileGrid`, `PanelMenu` and `AccentPill`) are the shared crate `kubuno-desktop-shell-controls`'s.

pub mod bar_chart;
pub mod org_unit_tree;
pub mod stacked_bar;
pub mod stat_card;
pub mod status_dot;
pub mod status_presenter;
pub mod storage_gauge;
