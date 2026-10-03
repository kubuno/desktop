//! `StorageBar` properties (mirrors
//! `Files.App.Controls/Storage/StorageBar/StorageBar.Properties.cs`).
//!
//! On the C# side these are `[GeneratedDependencyProperty]`; what's portable
//! is their default values, ported here as constants. The `OnXChanged` →
//! `UpdateControl` are WinUI machinery, not ported. The calculation logic
//! lives in [`super::storage_bar`].

/// `StorageBar.PercentCaution` — default amber threshold.
pub const PERCENT_CAUTION: f64 = 75.1;
/// `StorageBar.PercentCritical` — default red threshold.
pub const PERCENT_CRITICAL: f64 = 89.9;
/// Default height of the "value" bar (`ValueBarHeight`).
pub const VALUE_BAR_HEIGHT: f64 = 6.0;
/// Default height of the "track" bar (`TrackBarHeight`).
pub const TRACK_BAR_HEIGHT: f64 = 3.0;
