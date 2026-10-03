//! `05-labels` — Label, LinkLabel, PictureBox, ProgressBar.

use kubuno_desktop_controls::labels::{
    Label, LinkBehavior, LinkLabel, PictureBox, PictureBoxSizeMode, ProgressBar, ProgressBarStyle,
};
use kubuno_desktop_controls::{BorderStyle, ContentAlignment, Size};

use crate::sheet::{group, kid, Group, Sheet};

pub fn build() -> Sheet {
    Sheet::new(vec![plain(), links(), picture(), progress()])
}

fn label(text: &str) -> Label {
    let mut l = Label::new();
    l.text = text.to_string();
    l
}

fn plain() -> Group {
    let mut framed = label("Fixed3D border");
    framed.border_style = BorderStyle::Fixed3D;

    let mut centered = label("MiddleCenter");
    centered.border_style = BorderStyle::FixedSingle;
    centered.text_align = ContentAlignment::MiddleCenter;

    let mut disabled = label("Disabled");
    disabled.enabled = false;

    group(
        "Label — BorderStyle / align",
        300.0,
        vec![
            kid(label("AutoSize label")),
            kid(framed).w(200.0),
            kid(centered).w(200.0).h(40.0),
            kid(disabled).w(200.0),
        ],
    )
}

fn links() -> Group {
    let link = |text: &str, behavior: LinkBehavior| {
        let mut l = LinkLabel::new();
        l.text = text.to_string();
        l.link_behavior = behavior;
        l
    };
    let mut visited = link("Visit the documentation page", LinkBehavior::default());
    visited.link_visited = true;

    group(
        "LinkLabel — LinkBehavior",
        300.0,
        vec![
            kid(link("AlwaysUnderline", LinkBehavior::AlwaysUnderline)).w(240.0),
            kid(link("HoverUnderline", LinkBehavior::HoverUnderline)).w(240.0),
            kid(visited).w(240.0),
        ],
    )
}

/// The library models `Image` as its SIZE only — the bitmap lives host-side per
/// the `shell_icon` contract — so the box, its border and the zoom rectangle are
/// reproduced, but nothing is blitted inside them.
fn picture() -> Group {
    let mut p = PictureBox::new();
    p.border_style = BorderStyle::FixedSingle;
    p.size_mode = PictureBoxSizeMode::Zoom;
    p.image = Some(Size::new(64.0, 48.0));
    group("PictureBox — SizeMode=Zoom", 300.0, vec![kid(p).size(120.0, 80.0)])
}

fn progress() -> Group {
    let bar = |value: i32, style: ProgressBarStyle| {
        let mut b = ProgressBar::new();
        b.style = style;
        b.set_value(value);
        kid(b).w(240.0)
    };
    group(
        "ProgressBar — Style",
        300.0,
        vec![
            bar(45, ProgressBarStyle::Blocks),
            bar(70, ProgressBarStyle::Continuous),
            bar(0, ProgressBarStyle::Marquee),
        ],
    )
}
