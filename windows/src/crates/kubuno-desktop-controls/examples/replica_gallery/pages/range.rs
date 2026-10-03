//! `06-range` — ScrollBar, TrackBar, UpDownBase.

use kubuno_desktop_controls::range::{
    DomainUpDown, HScrollBar, NumericUpDown, TickStyle, TrackBar, VScrollBar,
};

use crate::sheet::{group, kid, Group, Sheet};

pub fn build() -> Sheet {
    Sheet::new(vec![scroll_bars(), track_bars(), numerics(), domain()])
}

/// Reports the toolkit's own out-of-range error instead of hiding it: the value
/// setters throw in WinForms, and the port surfaces that as a `Result` precisely
/// so a demo cannot set an impossible value by accident.
fn assign(what: &str, r: Result<(), kubuno_desktop_controls::range::OutOfRange>) {
    if let Err(e) = r {
        eprintln!("[gallery] {what}: {e}");
    }
}

/// `LargeChange` decides the thumb's proportion, and the highest reachable
/// `Value` is `Maximum - LargeChange + 1` — the trap the reference calls out.
/// The bars are the one control the reference does NOT auto-size: their
/// thickness is a system metric, and asking them to fit their (absent) content
/// would collapse them.
fn scroll_bars() -> Group {
    let horizontal = |large: i32, value: i32| {
        let mut h = HScrollBar::new();
        h.set_minimum(0);
        h.set_maximum(100);
        assign("HScrollBar.LargeChange", h.set_large_change(large));
        assign("HScrollBar.Value", h.set_value(value));
        kid(h).w(240.0).fixed()
    };

    let mut v = VScrollBar::new();
    v.set_minimum(0);
    v.set_maximum(100);
    assign("VScrollBar.LargeChange", v.set_large_change(10));
    assign("VScrollBar.Value", v.set_value(40));

    group(
        "ScrollBar — thumb ∝ LargeChange",
        300.0,
        vec![horizontal(20, 30), horizontal(50, 0), kid(v).h(120.0).fixed()],
    )
}

fn track_bars() -> Group {
    let track = |value: i32, style: TickStyle| {
        let mut t = TrackBar::new();
        t.set_tick_style(style);
        assign("TrackBar.Value", t.set_value(value));
        kid(t).w(240.0)
    };
    group(
        "TrackBar — TickStyle",
        300.0,
        vec![
            track(4, TickStyle::BottomRight),
            track(6, TickStyle::Both),
            track(2, TickStyle::None),
        ],
    )
}

/// `Maximum` BEFORE `Value`: the setter validates against the CURRENT range and
/// refuses a value outside it, so raising the ceiling has to come first.
fn numerics() -> Group {
    let mut plain = NumericUpDown::new();
    assign("NumericUpDown.Value", plain.set_value(42.0));

    let mut decimals = NumericUpDown::new();
    assign("NumericUpDown.DecimalPlaces", decimals.set_decimal_places(2));
    assign("NumericUpDown.Increment", decimals.set_increment(0.25));
    assign("NumericUpDown.Value", decimals.set_value(3.5));

    let mut hex = NumericUpDown::new();
    hex.set_maximum(4095.0);
    hex.set_hexadecimal(true);
    assign("NumericUpDown.Value", hex.set_value(255.0));

    let mut thousands = NumericUpDown::new();
    thousands.set_maximum(100_000.0);
    thousands.set_thousands_separator(true);
    assign("NumericUpDown.Value", thousands.set_value(10_000.0));

    group(
        "NumericUpDown",
        300.0,
        vec![
            kid(plain).w(200.0),
            kid(decimals).w(200.0),
            kid(hex).w(200.0),
            kid(thousands).w(200.0),
        ],
    )
}

fn domain() -> Group {
    let mut d = DomainUpDown::new();
    for day in ["Lundi", "Mardi", "Mercredi"] {
        d.add(day);
    }
    assign("DomainUpDown.SelectedIndex", d.set_selected_index(1));
    group("DomainUpDown", 300.0, vec![kid(d).w(200.0)])
}
