//! `01-buttonbase` — Button, CheckBox, RadioButton.

use kubuno_controls::buttons::{Button, CheckBox, RadioButton};
use kubuno_controls::{Appearance, CheckState, ContentAlignment, FlatStyle};

use crate::sheet::{group, kid, Group, Sheet};

pub fn build() -> Sheet {
    Sheet::new(vec![flat_styles(), states(), check_boxes(), radios(), alignment()])
}

fn button(text: &str) -> Button {
    let mut b = Button::new();
    b.text = text.to_string();
    b
}

/// The one resting difference the four `FlatStyle` values make is the border;
/// `Popup` and `System` only lift under the mouse, which a paint pass has no
/// signal for.
fn flat_styles() -> Group {
    let styled = |text: &str, style: FlatStyle| {
        let mut b = button(text);
        b.flat_style = style;
        kid(b).w(120.0)
    };
    group(
        "Button — FlatStyle",
        300.0,
        vec![
            styled("Standard", FlatStyle::Standard),
            styled("Flat", FlatStyle::Flat),
            styled("Popup", FlatStyle::Popup),
            styled("System", FlatStyle::System),
        ],
    )
}

fn states() -> Group {
    let mut disabled = button("Disabled");
    disabled.enabled = false;
    group(
        "Button — states",
        300.0,
        vec![
            kid(disabled).w(120.0),
            kid(button("Default (accept)")).w(140.0),
            // No explicit width: this is the one that shows what `AutoSize`
            // measures on its own.
            kid(button("AutoSize")),
        ],
    )
}

fn check_boxes() -> Group {
    let mut unchecked = CheckBox::new();
    unchecked.text = "Unchecked".to_string();

    let mut checked = CheckBox::new();
    checked.text = "Checked".to_string();
    checked.set_checked(true);

    // `ThreeState` first: without it the toolkit refuses to sit on
    // `Indeterminate`, so the flag has to be raised before the state is set.
    let mut indeterminate = CheckBox::new();
    indeterminate.text = "Indeterminate".to_string();
    indeterminate.three_state = true;
    indeterminate.check_state = CheckState::Indeterminate;

    let mut as_button = CheckBox::new();
    as_button.text = "Appearance=Button".to_string();
    as_button.appearance = Appearance::Button;

    group(
        "CheckBox — Appearance / CheckState",
        300.0,
        vec![
            kid(unchecked).w(160.0),
            kid(checked).w(160.0),
            kid(indeterminate).w(160.0),
            kid(as_button).w(160.0),
        ],
    )
}

fn radios() -> Group {
    let radio = |text: &str| {
        let mut r = RadioButton::new();
        r.text = text.to_string();
        r
    };
    let mut a = radio("Option A");
    a.checked = true;
    let b = radio("Option B");
    let mut disabled = radio("Disabled");
    disabled.enabled = false;

    group(
        "RadioButton",
        300.0,
        vec![kid(a).w(160.0), kid(b).w(160.0), kid(disabled).w(160.0)],
    )
}

/// A box taller than its caption is the only way `TextAlign` shows: the label is
/// pushed into the corner the alignment names.
fn alignment() -> Group {
    let aligned = |text: &str, align: ContentAlignment| {
        let mut b = button(text);
        b.text_align = align;
        kid(b).w(140.0).h(40.0)
    };
    group(
        "ButtonBase — TextAlign / Image",
        300.0,
        vec![
            aligned("TopLeft", ContentAlignment::TopLeft),
            aligned("BottomRight", ContentAlignment::BottomRight),
        ],
    )
}
