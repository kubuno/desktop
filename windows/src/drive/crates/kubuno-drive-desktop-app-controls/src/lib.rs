//! Custom Direct2D-rendered controls.
//! Rust port of `Files.App.Controls`.

// The module tree mirrors the folders of `Files.App.Controls` one to one, as
// the port requires — which puts `foo/foo.rs` under `foo`.
#![allow(clippy::module_inception)]
pub mod blade_view;
pub mod breadcrumb_bar;
pub mod button;
pub mod canvas;
pub mod edit_box;
pub mod geometry;
pub mod grid_splitter;
pub mod icon_source;
pub mod omnibar;
pub mod renderer;
pub mod scrollbar;
pub mod sidebar;
pub mod storage;
pub mod switch;
pub mod toolbar;
pub mod themed_icon;
pub mod themes;

pub use breadcrumb_bar::{layout_breadcrumbs, BreadcrumbLayout, BreadcrumbLayoutParams};
pub use canvas::Canvas;
pub use edit_box::EditView;
pub use geometry::Rect;
pub use scrollbar::Scrollbar;
pub use themes::{system_uses_light_theme, Theme, ThemeMode};
pub use renderer::{create_text_formats_styled, Images, Renderer, TextFormats, TextStyle};
pub use themed_icon::{icon_name, IconLayer, LayerRole, VectorIcons};
