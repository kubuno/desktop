//! `Toolbar` item layout (mirrors
//! `Files.App.Controls/Toolbar/Primitives/ToolbarLayout.cs`).
//!
//! The C# is only empty stubs (`MeasureOverride`/`ArrangeOverride` do
//! nothing); the intended arrangement algorithm (documented but never coded)
//! is materialized here by the port: "from last to first; if too narrow,
//! draw the ellipsis button".

use super::overflow_behaviors::OverflowBehavior;
use crate::geometry::Rect;

/// Overflow button ("More"): 40 x 32.
pub const OVERFLOW_BUTTON_WIDTH: f32 = 40.0;
pub const OVERFLOW_BUTTON_HEIGHT: f32 = 32.0;

/// An item as seen by the layout pass: desired width + rule.
#[derive(Clone, Copy)]
pub struct ItemMeasure {
    pub width: f32,
    pub overflow: OverflowBehavior,
}

/// Arrangement result.
#[derive(Default)]
pub struct ToolbarArrangement {
    /// (entry index, laid-out Rect) for items that stayed on the bar.
    pub visible: Vec<(usize, Rect)>,
    /// Indices routed to the overflow menu ("More ▾").
    pub overflow: Vec<usize>,
    /// Rect of the ellipsis button, `None` if nothing overflows.
    pub overflow_button: Option<Rect>,
}

/// Arranges items left to right within `available_width`, removing tail
/// items first ("last towards first"). Honors `OverflowBehavior`: `Always`
/// always goes to the menu, `Never` always stays, `Auto` overflows if space
/// is lacking. Reserves the ellipsis button's width as soon as an item
/// overflows. Pure primitive.
pub fn arrange(
    items: &[ItemMeasure],
    available_width: f32,
    top: f32,
    height: f32,
    spacing: f32,
) -> ToolbarArrangement {
    let n = items.len();
    let mut keep = vec![true; n];
    let mut any_overflow = false;
    for (i, it) in items.iter().enumerate() {
        if it.overflow == OverflowBehavior::Always {
            keep[i] = false;
            any_overflow = true;
        }
    }

    let width_of = |keep: &[bool]| -> f32 {
        let mut w = 0.0;
        let mut first = true;
        for (i, it) in items.iter().enumerate() {
            if !keep[i] {
                continue;
            }
            if !first {
                w += spacing;
            }
            w += it.width;
            first = false;
        }
        w
    };
    let budget = |any: bool| {
        if any {
            available_width - OVERFLOW_BUTTON_WIDTH - spacing
        } else {
            available_width
        }
    };

    while width_of(&keep) > budget(any_overflow) {
        let victim = (0..n)
            .rev()
            .find(|&i| keep[i] && items[i].overflow == OverflowBehavior::Auto);
        match victim {
            Some(i) => {
                keep[i] = false;
                any_overflow = true;
            }
            None => break,
        }
    }

    let mut out = ToolbarArrangement::default();
    let mut x = 0.0;
    for (i, it) in items.iter().enumerate() {
        if keep[i] {
            out.visible.push((i, Rect::new(x, top, x + it.width, top + height)));
            x += it.width + spacing;
        } else {
            out.overflow.push(i);
        }
    }
    if any_overflow {
        out.overflow_button = Some(Rect::new(x, top, x + OVERFLOW_BUTTON_WIDTH, top + height));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(w: f32, o: OverflowBehavior) -> ItemMeasure {
        ItemMeasure { width: w, overflow: o }
    }

    #[test]
    fn all_fit() {
        let items = [item(40.0, OverflowBehavior::Auto); 3];
        let a = arrange(&items, 500.0, 0.0, 32.0, 4.0);
        assert_eq!(a.visible.len(), 3);
        assert!(a.overflow_button.is_none());
    }

    #[test]
    fn overflow_from_tail() {
        let items = [item(40.0, OverflowBehavior::Auto); 5];
        let a = arrange(&items, 120.0, 0.0, 32.0, 4.0);
        // The tail overflows; the first ones stay.
        assert!(a.overflow_button.is_some());
        assert!(!a.overflow.is_empty());
        assert!(a.visible.iter().all(|(i, _)| !a.overflow.contains(i)));
    }

    #[test]
    fn never_stays_always_goes() {
        let items = [
            item(40.0, OverflowBehavior::Never),
            item(40.0, OverflowBehavior::Always),
        ];
        let a = arrange(&items, 500.0, 0.0, 32.0, 4.0);
        assert!(a.overflow.contains(&1));
        assert!(a.visible.iter().any(|(i, _)| *i == 0));
    }
}
