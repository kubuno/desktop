//! `ToolbarSizes` (mirrors `Files.App.Controls/Toolbar/Primitives/ToolbarSizes.cs`).

/// The `Toolbar`'s three size presets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolbarSize {
    Small,
    Medium,
    Large,
}

impl ToolbarSize {
    /// (MinWidth, MinHeight), from `ToolbarButton.ThemeResources.xaml` (the
    /// source that actually sizes the button): Small 32x32, Medium 40x32,
    /// Large 40x40.
    pub const fn button_size(self) -> (f32, f32) {
        match self {
            ToolbarSize::Small => (32.0, 32.0),
            ToolbarSize::Medium => (40.0, 32.0),
            ToolbarSize::Large => (40.0, 40.0),
        }
    }
    pub const fn button_width(self) -> f32 {
        self.button_size().0
    }
    pub const fn button_height(self) -> f32 {
        self.button_size().1
    }
}
