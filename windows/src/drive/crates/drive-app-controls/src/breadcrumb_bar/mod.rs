//! `BreadcrumbBar` (mirror of `Files.App.Controls/BreadcrumbBar`): the DRAWING
//! of the breadcrumb trail (segments + chevrons + overflow ellipsis). The
//! PURE layout algorithm lives alongside, in [`breadcrumb_bar_layout`]
//! (← `BreadcrumbBarLayout.cs`).

pub mod breadcrumb_bar;
pub mod breadcrumb_bar_layout;
pub mod event_args;

pub use breadcrumb_bar::{draw, BreadcrumbSegment, BreadcrumbView};
pub use breadcrumb_bar_layout::{layout_breadcrumbs, BreadcrumbLayout, BreadcrumbLayoutParams};
