//! Every kind of Kubuno window, in the web `FloatingWindow`'s look: a main form with controls in
//! its title bar, a dialog with an action bar, tool windows, a splash screen, a flyout, a
//! borderless window, an MDI parent with documents, an in-window dialog over a veil, and the
//! message boxes — in the light or the dark theme, switched live.
//!
//! `cargo run -p kubuno --example window_kinds [-- --open <kind>] [--dark]`, `<kind>` one of
//! `dialog`, `tool`, `splash`, `flyout`, `borderless`, `mdi`, `inwindow`, `info`, `warning`,
//! `error`, `question`, `danger` (the window opens at start, for screenshots).
#![windows_subsystem = "windows"]

use kubuno::prelude::*;

/// A dialog in the web's shape: the band, a body, the action bar (confirm on the left of cancel,
/// both on the right, text buttons).
fn properties_dialog() -> Form {
    let dialog = Form::new()
        .text("Propriétés — Rapport annuel.pdf")
        .client_size(560.0, 232.0)
        .window_kind(WindowKind::Dialog);
    dialog.set_icon("FileText");
    for (i, line) in ["Type : Document PDF", "Taille : 2,4 Mo", "Modifié : aujourd'hui, 14:32", "Emplacement : Drive / Documents / 2026"].iter().enumerate() {
        dialog.controls().add(&Label::new().text(*line).bounds(24.0, 24.0 + 30.0 * i as f32, 500.0, 30.0));
    }
    let apply = Button::new().name("apply").text("Appliquer").variant("Text").size(96.0, 36.0).property("ActionBar.Region", "Right").dialog_result(DialogResult::Ok);
    let cancel = Button::new().name("cancel").text("Annuler").variant("Ghost").size(96.0, 36.0).property("ActionBar.Region", "Right").dialog_result(DialogResult::Cancel);
    dialog.controls().add_range(&[&apply, &cancel]);
    dialog.set_accept_button(&apply);
    dialog.set_cancel_button(&cancel);
    dialog
}

fn tool_window() -> Form {
    let tool = Form::new().text("Calques").client_size(260.0, 300.0).window_kind(WindowKind::ToolWindow);
    for (i, layer) in ["Texte", "Formes", "Image de fond"].iter().enumerate() {
        tool.controls().add(&CheckBox::new().text(*layer).bounds(16.0, 16.0 + 32.0 * i as f32, 220.0, 28.0).property("Checked", true));
    }
    tool
}

fn splash() -> Form {
    let splash = Form::new().text("Démarrage").client_size(420.0, 240.0).window_kind(WindowKind::Splash);
    splash.root().set_property("SplashDuration", 4000.0);
    splash.root().set_property("BackColor", "Primary");
    let icon = Control::new("Icon");
    icon.set_property("Name", "Cloud");
    icon.set_property("Size", 56.0);
    icon.set_property("ForeColor", "OnPrimary");
    icon.set_bounds(182.0, 56.0, 56.0, 56.0);
    splash.controls().add(&icon);
    splash.controls().add(&Label::new().text("Kubuno Desktop").bounds(0.0, 128.0, 420.0, 32.0).property("TextAlign", "MiddleCenter").property("ForeColor", "OnPrimary").property("Font", "Segoe UI, 16pt"));
    splash.controls().add(&Label::new().text("Chargement…").bounds(0.0, 168.0, 420.0, 24.0).property("TextAlign", "MiddleCenter").property("ForeColor", "OnPrimary"));
    splash
}

fn flyout() -> Form {
    let flyout = Form::new().text("Notifications").client_size(300.0, 180.0);
    flyout.controls().add(&Label::new().text("Notifications").bounds(16.0, 12.0, 260.0, 28.0).property("Font", "Segoe UI, 12pt, style=Bold"));
    flyout.controls().add(&Label::new().text("Rapport annuel.pdf a été partagé avec vous.").bounds(16.0, 48.0, 268.0, 44.0).property("Overflow", "Wrap"));
    flyout.controls().add(&Button::new().text("Tout marquer comme lu").variant("Text").bounds(16.0, 124.0, 180.0, 36.0));
    flyout
}

fn borderless() -> Form {
    let window = Form::new().text("Lecteur").client_size(420.0, 260.0).form_border_style(FormBorderStyle::None);
    window.root().set_property("ResizeBorder", true);
    window.root().set_property("CornerPreference", "Round");
    let bar = Label::new().text("  Glisser ici pour déplacer — fenêtre sans bordure").bounds(0.0, 0.0, 420.0, 40.0).anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
        .property("BackColor", "Surface2").property("TitleBar.Drag", true);
    let close = Button::new().text("Fermer").variant("Secondary").bounds(312.0, 208.0, 96.0, 36.0).anchor(Anchor::BOTTOM | Anchor::RIGHT);
    let me = window.clone();
    close.click().subscribe(move |_s, _e| me.close());
    window.controls().add_range(&[&bar, &close]);
    window
}

fn mdi_parent() -> Form {
    let parent = Form::new().text("Éditeur — MDI").client_size(900.0, 560.0);
    parent.set_is_mdi_container(true);
    parent.set_icon("LayoutGrid");
    let bottom = Anchor::BOTTOM | Anchor::LEFT;
    let new_doc = Button::new().text("Nouveau document").variant("Secondary").bounds(16.0, 516.0, 160.0, 32.0).anchor(bottom);
    let cascade = Button::new().text("Cascade").variant("Ghost").bounds(184.0, 516.0, 96.0, 32.0).anchor(bottom);
    let tile_h = Button::new().text("Mosaïque H").variant("Ghost").bounds(288.0, 516.0, 110.0, 32.0).anchor(bottom);
    let tile_v = Button::new().text("Mosaïque V").variant("Ghost").bounds(406.0, 516.0, 110.0, 32.0).anchor(bottom);
    parent.controls().add_range(&[&new_doc, &cascade, &tile_h, &tile_v]);
    let count = std::rc::Rc::new(std::cell::Cell::new(0));
    let p = parent.clone();
    new_doc.click().subscribe(move |_s, _e| open_document(&p, &count));
    for (button, layout) in [(&cascade, MdiLayout::Cascade), (&tile_h, MdiLayout::TileHorizontal), (&tile_v, MdiLayout::TileVertical)] {
        let p = parent.clone();
        button.click().subscribe(move |_s, _e| p.layout_mdi(layout));
    }
    parent.mdi_child_activate().subscribe(|sender: &Form, _e| {
        kubuno::tracing::info!("active document: {:?}", sender.active_mdi_child().map(|f| f.get_text()));
    });
    parent
}

fn open_document(parent: &Form, count: &std::rc::Rc<std::cell::Cell<u32>>) {
    count.set(count.get() + 1);
    let doc = Form::new().text(format!("Document {}", count.get())).client_size(360.0, 200.0);
    doc.set_icon("FileText");
    doc.controls().add(&TextArea::new().text("Un document MDI, dessiné dans la fenêtre parente.").bounds(12.0, 12.0, 336.0, 176.0).anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT | Anchor::BOTTOM));
    doc.set_mdi_parent(parent);
    doc.show();
}

fn in_window_dialog() -> Form {
    let dialog = Form::new().text("Renommer").client_size(400.0, 132.0).window_kind(WindowKind::Dialog);
    dialog.controls().add(&TextField::new().text("Photos de vacances").bounds(24.0, 24.0, 352.0, 36.0));
    let ok = Button::new().text("Renommer").variant("Text").size(96.0, 36.0).property("ActionBar.Region", "Right").dialog_result(DialogResult::Ok);
    let cancel = Button::new().text("Annuler").variant("Ghost").size(96.0, 36.0).property("ActionBar.Region", "Right").dialog_result(DialogResult::Cancel);
    let me = dialog.clone();
    ok.click().subscribe(move |_s, _e| me.close());
    let me = dialog.clone();
    cancel.click().subscribe(move |_s, _e| me.close());
    dialog.controls().add_range(&[&ok, &cancel]);
    dialog
}

fn message_box(kind: &str) {
    let (text, icon, buttons) = match kind {
        "info" => ("La synchronisation est terminée : 128 fichiers à jour.", MessageBoxIcon::Information, MessageBoxButtons::Ok),
        "warning" => ("Trois fichiers ont été ignorés : leur format n'est pas pris en charge.", MessageBoxIcon::Warning, MessageBoxButtons::OkCancel),
        "error" => ("Le serveur n'a pas répondu. Vérifiez votre connexion, puis réessayez.", MessageBoxIcon::Error, MessageBoxButtons::RetryCancel),
        "question" => ("Enregistrer les modifications de « Rapport annuel » ?", MessageBoxIcon::Question, MessageBoxButtons::YesNoCancel),
        _ => ("« Rapport annuel.pdf » sera supprimé définitivement.", MessageBoxIcon::Danger, MessageBoxButtons::OkCancel),
    };
    let answer = MessageBox::show_with(text, "Message", buttons, icon);
    kubuno::tracing::info!("message box {kind}: {answer:?}");
}

/// Opens the window `kind` (from the main form's frame).
fn open(kind: &str, owner: &Form) {
    match kind {
        "dialog" => {
            let mut d = properties_dialog();
            let result = d.show_dialog(owner);
            kubuno::tracing::info!("dialog: {result:?}");
        }
        "tool" => {
            let t = tool_window();
            t.set_owner(owner);
            t.show();
        }
        "splash" => splash().show(),
        "flyout" => flyout().show_flyout(900.0, 180.0),
        "borderless" => borderless().show(),
        "mdi" => {
            let parent = mdi_parent();
            let count = std::rc::Rc::new(std::cell::Cell::new(0));
            // Two documents from the start.
            open_document(&parent, &count);
            open_document(&parent, &count);
            parent.show();
        }
        "inwindow" => in_window_dialog().show_in_window(owner, |r| kubuno::tracing::info!("in-window dialog: {r:?}")),
        "info" | "warning" | "error" | "question" | "danger" => message_box(kind),
        _ => {}
    }
}

fn main() -> kubuno::Result {
    let args: Vec<String> = std::env::args().collect();
    let dark = args.iter().any(|a| a == "--dark");
    let open_at_start = args.iter().position(|a| a == "--open").and_then(|i| args.get(i + 1)).cloned();
    if dark {
        Application::set_theme(kubuno::ui::Theme::dark());
    }

    let form = Form::new().text("Kubuno — Types de fenêtres").client_size(760.0, 420.0).start_position(StartPosition::CenterScreen);
    form.set_icon("AppWindow");
    form.set_subtitle("Rapport annuel.kbdoc");
    form.set_help_button(true);
    form.set_caption_buttons(&[CaptionCommand { id: "pin".into(), glyph: "Bookmark".into(), tooltip: "Garder au premier plan".into(), enabled: true, checked: false }, CaptionCommand::new("bell", "Bell")]);
    // Controls in the title bar: a search field in the middle, an avatar on the right.
    let search = SearchField::new().name("search").placeholder("Rechercher").size(260.0, 30.0).property("TitleBar.Region", "Center");
    let avatar = IconButton::new().name("avatar").property("Icon", "UserRound").property("ForeColor", "OnPrimary").size(30.0, 30.0).property("TitleBar.Region", "Right");
    form.controls().add_range(&[&search, &avatar]);

    let intro = Label::new()
        .text("Chaque bouton ouvre un type de fenêtre, dans l'apparence des fenêtres Kubuno du web.")
        .bounds(24.0, 20.0, 712.0, 24.0)
        .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
    form.controls().add(&intro);
    let kinds = [
        ("Dialogue", "dialog"),
        ("Fenêtre d'outils", "tool"),
        ("Écran de démarrage", "splash"),
        ("Menu volant", "flyout"),
        ("Sans bordure", "borderless"),
        ("Parent MDI", "mdi"),
        ("Dialogue dans la fenêtre", "inwindow"),
        ("Info", "info"),
        ("Avertissement", "warning"),
        ("Erreur", "error"),
        ("Question", "question"),
        ("Danger", "danger"),
    ];
    for (i, (label, kind)) in kinds.iter().enumerate() {
        let (col, row) = ((i % 3) as f32, (i / 3) as f32);
        let button = Button::new().text(*label).variant("Secondary").bounds(24.0 + 240.0 * col, 60.0 + 48.0 * row, 228.0, 36.0);
        let kind = kind.to_string();
        button.click().subscribe(move |sender: &Button, _e| {
            if let Some(owner) = sender.form() {
                open(&kind, &owner);
            }
        });
        form.controls().add(&button);
    }
    let theme = Button::new().text("Thème clair / sombre").variant("Primary").bounds(24.0, 360.0, 200.0, 36.0).anchor(Anchor::BOTTOM | Anchor::LEFT);
    let is_dark = std::rc::Rc::new(std::cell::Cell::new(dark));
    theme.click().subscribe(move |_s, _e| {
        is_dark.set(!is_dark.get());
        Application::set_theme(if is_dark.get() { kubuno::ui::Theme::dark() } else { kubuno::ui::Theme::light() });
    });
    form.controls().add(&theme);

    form.caption_button_click().subscribe(|_sender, e| kubuno::tracing::info!("caption button: {}", e.id));
    form.help_button_clicked().subscribe(|_sender, _e| {
        MessageBox::show_with("Aide des types de fenêtres.", "Aide", MessageBoxButtons::Ok, MessageBoxIcon::Information);
    });
    form.title_bar_double_click().subscribe(|_s, _e| kubuno::tracing::info!("title bar double-click"));
    form.resize_end().subscribe(|_s, _e| kubuno::tracing::info!("resize end"));
    form.dpi_changed().subscribe(|_s, e| kubuno::tracing::info!("dpi {} -> {}", e.old_dpi, e.new_dpi));

    if let Some(kind) = open_at_start {
        let opened = std::rc::Rc::new(std::cell::Cell::new(false));
        form.shown().subscribe(move |sender: &Form, _e| {
            if !opened.replace(true) {
                open(&kind, sender);
            }
        });
    }
    Application::run(form)
}
