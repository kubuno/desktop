//! `StorageRing` properties (mirrors
//! `Files.App.Controls/Storage/StorageRing/StorageRing.Properties.cs`).
//!
//! On the C# side these are `[GeneratedDependencyProperty]` (+ `RangeBase`'s
//! properties); what's portable is their default values. They're ported here
//! as the input fields of the [`StorageRing`] struct + its `impl Default`.
//! The `OnXChanged` → `UpdateRings` / `UpdateValues` are WinUI machinery,
//! not ported. The CALCULATIONS live in [`super::storage_ring`].

/// `StorageRing` inputs: `RangeBase` (`Value`/`Minimum`/`Maximum`) + its
/// `DependencyProperty`s. Defaults mirror `StorageRing.Properties.cs`.
#[derive(Clone, Copy, Debug)]
pub struct StorageRing {
    pub value: f64,
    pub minimum: f64,
    pub maximum: f64,
    pub min_angle: f64,
    pub max_angle: f64,
    pub value_ring_thickness: f64,
    pub track_ring_thickness: f64,
    pub percent_caution: f64,
    pub percent_critical: f64,
    pub is_enabled: bool,
    // Box: dimensions and margins/borders (for `UpdateContainerCenterAndSizes`).
    pub width: f64,
    pub height: f64,
    pub padding_left: f64,
    pub padding_right: f64,
    pub padding_top: f64,
    pub padding_bottom: f64,
    pub border_left: f64,
    pub border_right: f64,
    pub border_top: f64,
    pub border_bottom: f64,
}

impl Default for StorageRing {
    fn default() -> Self {
        Self {
            value: 0.0,
            minimum: 0.0,
            maximum: 100.0,
            min_angle: 0.0,
            max_angle: 360.0,
            value_ring_thickness: 0.0,
            track_ring_thickness: 0.0,
            percent_caution: 75.01,
            percent_critical: 90.01,
            is_enabled: true,
            width: 16.0,
            height: 16.0,
            padding_left: 0.0,
            padding_right: 0.0,
            padding_top: 0.0,
            padding_bottom: 0.0,
            border_left: 0.0,
            border_right: 0.0,
            border_top: 0.0,
            border_bottom: 0.0,
        }
    }
}
