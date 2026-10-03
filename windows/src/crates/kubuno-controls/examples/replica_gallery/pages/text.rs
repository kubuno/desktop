//! `02-textboxbase` — TextBox, MaskedTextBox, RichTextBox.

use kubuno_controls::text::{MaskedTextBox, RichTextBox, StyledRun, TextBox};
use kubuno_controls::{BorderStyle, ScrollBars};

use crate::sheet::{group, kid, Group, Sheet};

pub fn build() -> Sheet {
    Sheet::new(vec![borders(), states(), multiline(), masked(), rich()])
}

fn text_box(text: &str) -> TextBox {
    let mut t = TextBox::new();
    t.set_text(text);
    t
}

fn borders() -> Group {
    let bordered = |text: &str, style: BorderStyle| {
        let mut t = text_box(text);
        t.border_style = style;
        kid(t).w(260.0)
    };
    group(
        "TextBox — BorderStyle",
        320.0,
        vec![
            bordered("Fixed3D (default)", BorderStyle::Fixed3D),
            bordered("FixedSingle", BorderStyle::FixedSingle),
            bordered("None", BorderStyle::None),
        ],
    )
}

fn states() -> Group {
    let mut read_only = text_box("ReadOnly");
    read_only.read_only = true;

    let mut disabled = text_box("Disabled");
    disabled.enabled = false;

    // `UseSystemPasswordChar` wins over `PasswordChar`, so the glyph the field
    // shows comes from the system, not from the string.
    let mut password = text_box("Password");
    password.use_system_password_char = true;

    let mut placeholder = TextBox::new();
    placeholder.placeholder_text = "PlaceholderText".to_string();

    group(
        "TextBox — states",
        320.0,
        vec![
            kid(read_only).w(260.0),
            kid(disabled).w(260.0),
            kid(password).w(260.0),
            kid(placeholder).w(260.0),
        ],
    )
}

fn multiline() -> Group {
    let mut t = text_box(
        "Multiline with a vertical scrollbar.\r\nSecond line.\r\nThird line.\r\nFourth line.",
    );
    t.multiline = true;
    t.scroll_bars = ScrollBars::Vertical;
    group("TextBox — Multiline / ScrollBars", 320.0, vec![kid(t).size(260.0, 80.0)])
}

fn masked() -> Group {
    let masked = |mask: &str| {
        let mut m = MaskedTextBox::new();
        m.set_mask(mask);
        kid(m).w(260.0)
    };
    group("MaskedTextBox", 320.0, vec![masked("00/00/0000"), masked("(999) 000-0000")])
}

/// The reference feeds RTF; the port has no RTF parser, so the same document is
/// expressed as the styled runs the control does model.
fn rich() -> Group {
    let mut r = RichTextBox::new();
    r.set_runs(vec![
        StyledRun { bold: true, ..StyledRun::plain("Bold") },
        StyledRun::plain(" then "),
        StyledRun { italic: true, ..StyledRun::plain("italic") },
        StyledRun::plain(" then "),
        StyledRun { underline: true, ..StyledRun::plain("underline") },
        StyledRun::plain(".\nSecond paragraph."),
    ]);
    group("RichTextBox", 320.0, vec![kid(r).size(260.0, 80.0)])
}
