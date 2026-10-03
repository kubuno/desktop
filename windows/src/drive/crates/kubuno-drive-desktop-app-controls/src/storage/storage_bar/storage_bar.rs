//! Storage bar — calculations (mirrors
//! `Files.App.Controls/Storage/StorageBar/StorageBar.cs`).
//!
//! The `UpdateVisualState` logic: Safe / Caution / Critical based on two
//! percentages; and `UpdateContainerHeightsAndCorners`: the corner radius
//! based on [`BarShapes`]. Drawing (rectangles) stays in `kubuno-drive-desktop`; the
//! severity CHOICE and the radius calculation live here. The
//! `DependencyProperty` default values are in
//! [`super::storage_bar_properties`].

use super::storage_bar_properties::{PERCENT_CAUTION, PERCENT_CRITICAL};
use crate::storage::data::{BarShapes, StorageSeverity};

/// `UpdateContainerHeightsAndCorners`: corner radius of a bar of height
/// `bar_height` based on its shape — Round = h/2, Soft = h/4, Flat = 0.
pub fn corner_radius(shape: BarShapes, bar_height: f64) -> f64 {
    match shape {
        BarShapes::Round => bar_height / 2.0,
        BarShapes::Soft => bar_height / 4.0,
        BarShapes::Flat => 0.0,
    }
}

/// Width of the "value" bar for a container of width `container_width`
/// filled to `value_percent`% — the C#'s `(MaxWidth / 100) * valuePercent`.
pub fn value_bar_width(container_width: f64, value_percent: f64) -> f64 {
    (container_width / 100.0) * value_percent
}

/// Severity for a fill percentage `[0, 100]`, with the two thresholds.
pub fn severity(percent: f64, caution: f64, critical: f64) -> StorageSeverity {
    if percent >= critical {
        StorageSeverity::Critical
    } else if percent >= caution {
        StorageSeverity::Caution
    } else {
        StorageSeverity::Safe
    }
}

/// Severity with the default thresholds (`PERCENT_CAUTION` / `PERCENT_CRITICAL`).
pub fn severity_default(percent: f64) -> StorageSeverity {
    severity(percent, PERCENT_CAUTION, PERCENT_CRITICAL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severities() {
        assert_eq!(severity_default(50.0), StorageSeverity::Safe);
        assert_eq!(severity_default(80.0), StorageSeverity::Caution);
        assert_eq!(severity_default(95.0), StorageSeverity::Critical);
        assert_eq!(severity_default(75.1), StorageSeverity::Caution);
        assert_eq!(severity_default(89.9), StorageSeverity::Critical);
    }

    #[test]
    fn corners_by_shape() {
        assert_eq!(corner_radius(BarShapes::Round, 6.0), 3.0);
        assert_eq!(corner_radius(BarShapes::Soft, 6.0), 1.5);
        assert_eq!(corner_radius(BarShapes::Flat, 6.0), 0.0);
    }

    #[test]
    fn value_width() {
        assert_eq!(value_bar_width(200.0, 50.0), 100.0);
        assert_eq!(value_bar_width(200.0, 0.0), 0.0);
    }
}
