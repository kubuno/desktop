//! A form built entirely in code, the Windows Forms way: controls created with builders, added to
//! the form's collection, events subscribed with closures; a modal dialog built in code, a message
//! box, a second window, and a control added while the application runs.
//!
//! `cargo run -p kubuno --example code_first`.
#![windows_subsystem = "windows"]

use kubuno::prelude::*;

/// A small dialog asking for a name: OK / Cancel, `DialogResult` on the buttons.
fn name_dialog(initial: &str) -> (Form, TextField) {
    let dialog = Form::new()
        .text("Your name")
        .client_size(360.0, 132.0)
        .form_border_style(FormBorderStyle::FixedDialog)
        .maximize_box(false)
        .minimize_box(false);
    let field = TextField::new().name("field").text(initial).bounds(16.0, 16.0, 328.0, 36.0).anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
    let ok = Button::new().name("ok").text("OK").variant("Primary").dialog_result(DialogResult::Ok).bounds(160.0, 80.0, 88.0, 36.0).anchor(Anchor::BOTTOM | Anchor::RIGHT);
    let cancel = Button::new().name("cancel").text("Cancel").dialog_result(DialogResult::Cancel).bounds(256.0, 80.0, 88.0, 36.0).anchor(Anchor::BOTTOM | Anchor::RIGHT);
    dialog.controls().add(&field);
    dialog.controls().add(&ok);
    dialog.controls().add(&cancel);
    dialog.set_accept_button(&ok);
    dialog.set_cancel_button(&cancel);
    (dialog, field)
}

fn main() -> kubuno::Result {
    let form = Form::new().text("Code first").client_size(440.0, 232.0).start_position(StartPosition::CenterScreen);

    let name = TextField::new().name("name").placeholder("Your name").bounds(16.0, 16.0, 408.0, 36.0).anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
    let loud = CheckBox::new().name("loud").text("Shout").bounds(16.0, 64.0, 200.0, 28.0);
    let greeting = Label::new().name("greeting").text("Type a name, then Greet.").bounds(16.0, 104.0, 408.0, 24.0).anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
    let greet = Button::new().name("greet").text("Greet").variant("Primary").bounds(16.0, 180.0, 96.0, 36.0).anchor(Anchor::BOTTOM | Anchor::LEFT);
    let ask = Button::new().name("ask").text("Name…").bounds(120.0, 180.0, 96.0, 36.0).anchor(Anchor::BOTTOM | Anchor::LEFT);
    let more = Button::new().name("more").text("Add a button").bounds(224.0, 180.0, 112.0, 36.0).anchor(Anchor::BOTTOM | Anchor::LEFT);
    let window = Button::new().name("window").text("Window").bounds(344.0, 180.0, 80.0, 36.0).anchor(Anchor::BOTTOM | Anchor::RIGHT);
    form.controls().add_range(&[&name, &loud, &greeting, &greet, &ask, &more, &window]);
    form.set_accept_button(&greet);

    // Greet: read the controls, write the label.
    let (n, l, g) = (name.clone(), loud.clone(), greeting.clone());
    greet.click().subscribe(move |_sender, _e| {
        let who = n.get_text();
        let mut text = if who.trim().is_empty() { "Hello, whoever you are!".to_string() } else { format!("Hello, {}!", who.trim()) };
        if l.is_checked() {
            text = text.to_uppercase();
        }
        kubuno::tracing::info!("greet: {text}");
        g.set_text(text);
    });

    // Name…: a modal dialog owned by this form; its result decides.
    let n = name.clone();
    ask.click().subscribe(move |sender: &Button, _e| {
        let (mut dialog, field) = name_dialog(&n.get_text());
        let Some(owner) = sender.form() else { return };
        let result = dialog.show_dialog(&owner);
        kubuno::tracing::info!("dialog: {result:?}, field = {:?}", field.get_text());
        if result == DialogResult::Ok {
            n.set_text(field.get_text());
        }
    });

    // Add a button: a message box asks, the form gets a control while it runs.
    let f = form.clone();
    let count = std::rc::Rc::new(std::cell::Cell::new(0));
    more.click().subscribe(move |_sender, _e| {
        let answer = MessageBox::show_with("Add a button to the form?", "Code first", MessageBoxButtons::YesNo, MessageBoxIcon::Question);
        kubuno::tracing::info!("message box: {answer:?}");
        if answer != DialogResult::Yes {
            return;
        }
        count.set(count.get() + 1);
        let i = count.get() - 1;
        let added = Button::new().text(format!("Added {}", count.get())).bounds(232.0 + 100.0 * (i % 2) as f32, 60.0 + 36.0 * (i / 2) as f32, 92.0, 32.0);
        added.click().subscribe(|sender: &Button, _e| kubuno::tracing::info!("{} clicked", sender.get_text()));
        f.controls().add(&added);
        kubuno::tracing::info!("added {}", added.get_name());
    });

    // Window: a second, modeless form.
    window.click().subscribe(|_sender, _e| {
        let other = Form::new().text("Another window").client_size(320.0, 120.0);
        let label = Label::new().text("A second form, on the same UI thread.").bounds(16.0, 16.0, 288.0, 24.0);
        let close = Button::new().text("Close").bounds(216.0, 68.0, 88.0, 36.0).anchor(Anchor::BOTTOM | Anchor::RIGHT);
        let me = other.clone();
        close.click().subscribe(move |_s, _e| me.close());
        other.controls().add(&label);
        other.controls().add(&close);
        other.show();
    });

    form.load().subscribe(|sender: &Form, _e| kubuno::tracing::info!("load: {} controls", sender.controls().len()));
    form.form_closing().subscribe(|_sender, e| kubuno::tracing::info!("closing: {:?}", e.reason));
    kubuno::Application::run(form)
}
