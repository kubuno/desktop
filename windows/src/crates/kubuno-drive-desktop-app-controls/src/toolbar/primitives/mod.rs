//! `Toolbar` primitives (mirrors `Files.App.Controls/Toolbar/Primitives`).
//!
//! One file per type, like the C#: [`toolbar_sizes`] (← `ToolbarSizes.cs`),
//! [`overflow_behaviors`] (← `OverflowBehaviors.cs`), [`toolbar_layout`]
//! (← `ToolbarLayout.cs`, the `arrange` algorithm).

pub mod overflow_behaviors;
pub mod toolbar_layout;
pub mod toolbar_sizes;

pub use overflow_behaviors::OverflowBehavior;
pub use toolbar_layout::{arrange, ItemMeasure, ToolbarArrangement, OVERFLOW_BUTTON_HEIGHT, OVERFLOW_BUTTON_WIDTH};
pub use toolbar_sizes::ToolbarSize;
