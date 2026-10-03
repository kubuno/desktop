//! [`MessageBox`]: a modal message in Kubuno's style (Windows Forms' `MessageBox.Show`).

use kubuno_controls::host::{self, FormBorderStyle, StartPosition};

use crate::forms::{AsForm, Button, Control, DialogResult, Form, Label, WindowKind};
use crate::Anchor;

/// The buttons of a [`MessageBox`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MessageBoxButtons {
    #[default]
    Ok,
    OkCancel,
    YesNo,
    YesNoCancel,
    RetryCancel,
    AbortRetryIgnore,
}

impl MessageBoxButtons {
    /// The buttons, left to right, with their result.
    fn buttons(self) -> &'static [(&'static str, DialogResult)] {
        match self {
            Self::Ok => &[("OK", DialogResult::Ok)],
            Self::OkCancel => &[("OK", DialogResult::Ok), ("Cancel", DialogResult::Cancel)],
            Self::YesNo => &[("Yes", DialogResult::Yes), ("No", DialogResult::No)],
            Self::YesNoCancel => &[("Yes", DialogResult::Yes), ("No", DialogResult::No), ("Cancel", DialogResult::Cancel)],
            Self::RetryCancel => &[("Retry", DialogResult::Retry), ("Cancel", DialogResult::Cancel)],
            Self::AbortRetryIgnore => &[("Abort", DialogResult::Abort), ("Retry", DialogResult::Retry), ("Ignore", DialogResult::Ignore)],
        }
    }
}

/// The icon of a [`MessageBox`] — Windows Forms' five, plus Kubuno's `Danger` (a destructive
/// confirmation: a red disc and a red confirming action, the web `ConfirmDialog`'s `danger`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MessageBoxIcon {
    #[default]
    None,
    Information,
    Warning,
    Error,
    Question,
    /// A destructive action (delete, discard): the confirming button turns red.
    Danger,
}

impl MessageBoxIcon {
    /// The glyph and the disc it sits on (the `Icon` element's `Disc`).
    fn glyph(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::None => None,
            Self::Information => Some(("Info", "Info")),
            Self::Warning => Some(("AlertTriangle", "Warning")),
            Self::Error => Some(("AlertCircle", "Danger")),
            Self::Question => Some(("HelpCircle", "Neutral")),
            Self::Danger => Some(("Trash2", "Danger")),
        }
    }
}
/// A modal message with buttons, in Kubuno's style — Windows Forms' `MessageBox`. It is owned by
/// the window whose event handler shows it (or by `owner`), which takes no input until it closes.
///
/// ```no_run
/// use kubuno::prelude::*;
/// if MessageBox::show_with("Discard the changes?", "Editor", MessageBoxButtons::YesNo, MessageBoxIcon::Question) == DialogResult::Yes {
///     // …
/// }
/// ```
pub struct MessageBox;

impl MessageBox {
    /// Shows `text` with an OK button.
    pub fn show(text: &str) -> DialogResult {
        Self::show_with(text, "", MessageBoxButtons::Ok, MessageBoxIcon::None)
    }

    /// Shows `text` with a title, buttons and an icon; returns the button chosen (`Cancel`, or `No`
    /// when there is no Cancel, if the box was closed otherwise).
    pub fn show_with(text: &str, caption: &str, buttons: MessageBoxButtons, icon: MessageBoxIcon) -> DialogResult {
        let owner = host::main_window().map(|h| h.0 as isize);
        Self::run(text, caption, buttons, icon, owner)
    }

    /// [`MessageBox::show_with`], owned by `owner`.
    pub fn show_owned(owner: &dyn AsForm, text: &str, caption: &str, buttons: MessageBoxButtons, icon: MessageBoxIcon) -> DialogResult {
        let handle = owner.as_form().handle().or_else(|| host::main_window().map(|h| h.0 as isize));
        Self::run(text, caption, buttons, icon, handle)
    }

    /// The box as a form (what [`MessageBox::show_with`] shows): exposed for tests and for callers
    /// that restyle it before `show_dialog`.
    ///
    /// The web `ConfirmDialog`'s look, in a real dialog window: the Kubuno title band with the
    /// caption, a body `p-6` holding the icon on its 48 DIP disc then the message, and the window's
    /// action bar with the buttons on the right — the confirming ones as text buttons, the
    /// cancelling one as a ghost, all at least 96 DIP wide (`FloatingWindow`'s footer rule).
    pub fn build(text: &str, caption: &str, buttons: MessageBoxButtons, icon: MessageBoxIcon) -> Form {
        /// `ConfirmDialog`'s `defaultWidth={380}`, `p-6`, the disc and the gap under it.
        const WIDTH: f32 = 380.0;
        const PAD: f32 = 24.0;
        const DISC: f32 = 48.0;
        const GAP: f32 = 16.0;
        const LINE: f32 = 20.0;
        let glyph = icon.glyph();
        // Roughly 50 characters per line at the body size in the text column, plus explicit breaks.
        let per_line = (((WIDTH - 2.0 * PAD) / 6.6) as usize).max(20);
        let lines: usize = text.split('\n').map(|l| l.chars().count().div_ceil(per_line).max(1)).sum();
        let text_height = lines as f32 * LINE;
        let text_top = PAD + if glyph.is_some() { DISC + GAP } else { 0.0 };
        let height = text_top + text_height + PAD + kubuno_controls::window_chrome::FOOTER_HEIGHT;
        let form = Form::new()
            .text(if caption.is_empty() { host::diagnostics::exe_name() } else { caption.to_string() })
            .client_size(WIDTH, height)
            .window_kind(WindowKind::Dialog)
            .start_position(StartPosition::CenterParent)
            .form_border_style(FormBorderStyle::FixedDialog)
            .maximize_box(false)
            .minimize_box(false)
            .property("ShowInTaskbar", false);
        if let Some((name, disc)) = glyph {
            let icon = Control::new("Icon");
            icon.set_property("Name", name);
            icon.set_property("Size", DISC);
            icon.set_property("Disc", disc);
            icon.set_bounds(PAD, PAD, DISC, DISC);
            form.controls().add(&icon);
        }
        let label = Label::new()
            .name("message")
            .text(text)
            .bounds(PAD, text_top, WIDTH - 2.0 * PAD, text_height)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .property("Overflow", "Wrap");
        form.controls().add(&label);
        let list = buttons.buttons();
        for (i, (caption, result)) in list.iter().enumerate() {
            let cancelling = matches!(result, DialogResult::Cancel | DialogResult::Ignore) && list.len() > 1;
            let variant = match (cancelling, icon) {
                (true, _) => "Ghost",
                (false, MessageBoxIcon::Danger) if i == 0 => "TextDanger",
                _ => "Text",
            };
            let button = Button::new()
                .name(&format!("button{}", i + 1))
                .text(*caption)
                .bounds(0.0, 0.0, 96.0, 36.0)
                .property("ActionBar.Region", "Right")
                .variant(variant)
                .dialog_result(*result);
            form.controls().add(&button);
            if i == 0 {
                form.set_accept_button(&button);
            }
            if matches!(result, DialogResult::Cancel) || (list.len() == 1) {
                form.set_cancel_button(&button);
            }
        }
        form
    }

    fn run(text: &str, caption: &str, buttons: MessageBoxButtons, icon: MessageBoxIcon, owner: Option<isize>) -> DialogResult {
        let mut form = Self::build(text, caption, buttons, icon);
        let result = crate::application::show_dialog(&mut form, owner);
        let has_cancel = buttons.buttons().iter().any(|(_, r)| *r == DialogResult::Cancel);
        match result {
            DialogResult::Cancel if !has_cancel && buttons == MessageBoxButtons::YesNo => DialogResult::No,
            DialogResult::Cancel if !has_cancel && buttons == MessageBoxButtons::Ok => DialogResult::Ok,
            r => r,
        }
    }
}
