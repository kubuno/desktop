//! The catalogue of overridable members of the control hierarchy (EVT-7b of
//! `vskubuno/docs/EVENTS.md`, "Override assistance"): every `on_…` method and hook a class can
//! override, per level trait, with its exact signature and the body that calls the base behaviour
//! (WinForms' `base.OnPaint(e)`). Visual Studio's "Substituer des membres…" dialog and the
//! `override`-style completion in an `impl Control for X` block read it (exported as JSON by an
//! ignored test, see `tests::write_overrides_fixture`), and [`stub`] writes the method.
//!
//! The table is checked against the real traits: a test renders every member with [`stub`] into
//! `overrides_fixture.rs` form and compares it with that file, which is compiled as a class
//! overriding everything — a signature that drifts from its trait breaks the build.

/// One overridable member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverridableMember {
    /// The level trait that declares it (`"Control"`, `"ButtonBase"`…).
    pub level: &'static str,
    /// The method name.
    pub name: &'static str,
    /// The signature, as written in an `impl` (`fn on_click(&mut self, e: &mut EventCx<'_,
    /// MouseEventArgs>)`), without the body.
    pub signature: &'static str,
    /// The body that runs the base behaviour (`self.base_mut().on_click(e);`).
    pub base_call: &'static str,
    /// The event its base behaviour raises (`"OnClick"`), if any.
    pub event: Option<&'static str>,
    /// What it does, for users.
    pub doc: &'static str,
    /// The same in French.
    pub doc_fr: &'static str,
}

macro_rules! event_members {
    ($level:literal: $($name:ident($args:ident) => $event:literal, $doc:literal, $doc_fr:literal;)*) => {
        [$(OverridableMember {
            level: $level,
            name: stringify!($name),
            signature: concat!("fn ", stringify!($name), "(&mut self, e: &mut EventCx<'_, ", stringify!($args), ">)"),
            base_call: concat!("self.base_mut().", stringify!($name), "(e);"),
            event: Some($event),
            doc: $doc,
            doc_fr: $doc_fr,
        }),*]
    };
}

const fn member(level: &'static str, name: &'static str, signature: &'static str, base_call: &'static str, event: Option<&'static str>, doc: &'static str, doc_fr: &'static str) -> OverridableMember {
    OverridableMember { level, name, signature, base_call, event, doc, doc_fr }
}

const CONTROL_EVENTS: [OverridableMember; 33] = event_members! { "Control":
    on_click(MouseEventArgs) => "OnClick", "Raises Click when the control is clicked.", "Déclenche Click quand le contrôle est cliqué.";
    on_double_click(MouseEventArgs) => "OnDoubleClick", "Raises DoubleClick.", "Déclenche DoubleClick.";
    on_mouse_click(MouseEventArgs) => "OnMouseClick", "Raises MouseClick.", "Déclenche MouseClick.";
    on_mouse_double_click(MouseEventArgs) => "OnMouseDoubleClick", "Raises MouseDoubleClick.", "Déclenche MouseDoubleClick.";
    on_mouse_down(MouseEventArgs) => "OnMouseDown", "Raises MouseDown when a mouse button is pressed over the control.", "Déclenche MouseDown quand un bouton de la souris est enfoncé sur le contrôle.";
    on_mouse_up(MouseEventArgs) => "OnMouseUp", "Raises MouseUp.", "Déclenche MouseUp.";
    on_mouse_move(MouseEventArgs) => "OnMouseMove", "Raises MouseMove.", "Déclenche MouseMove.";
    on_mouse_enter(EmptyEventArgs) => "OnMouseEnter", "Raises MouseEnter when the pointer enters the control.", "Déclenche MouseEnter quand le pointeur entre dans le contrôle.";
    on_mouse_leave(EmptyEventArgs) => "OnMouseLeave", "Raises MouseLeave.", "Déclenche MouseLeave.";
    on_mouse_hover(EmptyEventArgs) => "OnMouseHover", "Raises MouseHover.", "Déclenche MouseHover.";
    on_mouse_wheel(MouseEventArgs) => "OnMouseWheel", "Raises MouseWheel.", "Déclenche MouseWheel.";
    on_key_down(KeyEventArgs) => "OnKeyDown", "Raises KeyDown.", "Déclenche KeyDown.";
    on_key_up(KeyEventArgs) => "OnKeyUp", "Raises KeyUp.", "Déclenche KeyUp.";
    on_key_press(KeyPressEventArgs) => "OnKeyPress", "Raises KeyPress for a character key.", "Déclenche KeyPress pour une touche de caractère.";
    on_enter(EmptyEventArgs) => "OnEnter", "Raises Enter when the focus enters the control.", "Déclenche Enter quand le focus entre dans le contrôle.";
    on_leave(EmptyEventArgs) => "OnLeave", "Raises Leave.", "Déclenche Leave.";
    on_got_focus(EmptyEventArgs) => "OnGotFocus", "Raises GotFocus.", "Déclenche GotFocus.";
    on_lost_focus(EmptyEventArgs) => "OnLostFocus", "Raises LostFocus.", "Déclenche LostFocus.";
    on_validating(CancelEventArgs) => "OnValidating", "Raises Validating; set cancel to keep the focus.", "Déclenche Validating ; définissez cancel pour garder le focus.";
    on_validated(EmptyEventArgs) => "OnValidated", "Raises Validated.", "Déclenche Validated.";
    on_move(EmptyEventArgs) => "OnMove", "Raises Move.", "Déclenche Move.";
    on_size_changed(EmptyEventArgs) => "OnSizeChanged", "Raises SizeChanged.", "Déclenche SizeChanged.";
    on_location_changed(EmptyEventArgs) => "OnLocationChanged", "Raises LocationChanged.", "Déclenche LocationChanged.";
    on_layout(LayoutEventArgs) => "OnLayout", "Raises Layout.", "Déclenche Layout.";
    on_visible_changed(EmptyEventArgs) => "OnVisibleChanged", "Raises VisibleChanged.", "Déclenche VisibleChanged.";
    on_enabled_changed(EmptyEventArgs) => "OnEnabledChanged", "Raises EnabledChanged.", "Déclenche EnabledChanged.";
    on_text_changed(TextChangedEventArgs) => "OnTextChanged", "Raises TextChanged.", "Déclenche TextChanged.";
    on_handle_created(EmptyEventArgs) => "OnHandleCreated", "Raises HandleCreated.", "Déclenche HandleCreated.";
    on_handle_destroyed(EmptyEventArgs) => "OnHandleDestroyed", "Raises HandleDestroyed.", "Déclenche HandleDestroyed.";
    on_drag_enter(DragEventArgs) => "OnDragEnter", "Raises DragEnter when a drag comes over the control; set e.effect to accept the drop.", "Déclenche DragEnter quand un glissement arrive sur le contrôle ; définissez e.effect pour accepter le dépôt.";
    on_drag_over(DragEventArgs) => "OnDragOver", "Raises DragOver while a drag moves over the control.", "Déclenche DragOver pendant qu'un glissement se déplace sur le contrôle.";
    on_drag_drop(DragEventArgs) => "OnDragDrop", "Raises DragDrop when the data is dropped on the control.", "Déclenche DragDrop quand les données sont déposées sur le contrôle.";
    on_drag_leave(EmptyEventArgs) => "OnDragLeave", "Raises DragLeave when a drag leaves the control or is cancelled.", "Déclenche DragLeave quand un glissement quitte le contrôle ou est annulé.";
};

const CONTROL_HOOKS: [OverridableMember; 14] = [
    member("Control", "on_paint", "fn on_paint(&mut self, e: &mut PaintEventCx<'_>)", "self.base_mut().on_paint(e);", Some("OnPaint"),
        "Paints the control (e.graphics, e.clip_rectangle), then raises Paint.", "Dessine le contrôle (e.graphics, e.clip_rectangle), puis déclenche Paint."),
    member("Control", "on_paint_background", "fn on_paint_background(&mut self, e: &mut PaintEventCx<'_>)", "self.base_mut().on_paint_background(e);", None,
        "Paints the background (BackColor, BackgroundImage) before on_paint; not called with the OPAQUE style.", "Dessine l'arrière-plan (BackColor, BackgroundImage) avant on_paint ; pas appelé avec le style OPAQUE."),
    member("Control", "on_print", "fn on_print(&mut self, e: &mut PaintEventCx<'_>)", "self.paint_layers(e);", None,
        "Renders the control for printing or an off-screen capture (draw_to_bitmap): background then on_paint by default.", "Rend le contrôle pour l'impression ou une capture hors écran (draw_to_bitmap) : arrière-plan puis on_paint par défaut."),
    member("Control", "on_resize", "fn on_resize(&mut self, e: &mut EventCx<'_, EmptyEventArgs>)", "self.base_mut().on_resize(e);", Some("OnResize"),
        "Raises Resize, then invalidates under RESIZE_REDRAW.", "Déclenche Resize, puis invalide avec RESIZE_REDRAW."),
    member("Control", "on_create_control", "fn on_create_control(&mut self)", "self.base_mut().on_create_control();", None,
        "Called once, before the control is first painted.", "Appelé une fois, avant le premier dessin du contrôle."),
    member("Control", "get_preferred_size", "fn get_preferred_size(&self, canvas: &dyn Canvas, proposed: Size) -> Size", "self.base().get_preferred_size(canvas, proposed)", None,
        "The size the control would like (what a layout gives an auto-sized control).", "La taille souhaitée par le contrôle (celle qu'une disposition donne à un contrôle dimensionné automatiquement)."),
    member("Control", "set_bounds_core", "fn set_bounds_core(&mut self, bounds: Rect, specified: BoundsSpecified)", "self.base_mut().set_bounds_core(bounds, specified);", None,
        "Stores new bounds; a custom container constrains its size here.", "Enregistre de nouvelles limites ; un conteneur personnalisé contraint sa taille ici."),
    member("Control", "is_input_key", "fn is_input_key(&self, key: Keys) -> bool", "self.base().is_input_key(key)", None,
        "Whether the control takes a dialog key (Tab, arrows, Enter, Escape) as input.", "Indique si le contrôle prend une touche de dialogue (Tab, flèches, Entrée, Échap) comme saisie."),
    member("Control", "is_input_char", "fn is_input_char(&self, c: char) -> bool", "self.base().is_input_char(c)", None,
        "Whether the control takes the character as input.", "Indique si le contrôle prend le caractère comme saisie."),
    member("Control", "process_cmd_key", "fn process_cmd_key(&mut self, msg: &mut Message, key: Keys) -> bool", "self.base_mut().process_cmd_key(msg, key)", None,
        "A shortcut seen before anything else; return true to consume it.", "Un raccourci vu avant tout le reste ; renvoyez true pour le consommer."),
    member("Control", "process_dialog_key", "fn process_dialog_key(&mut self, key: Keys) -> bool", "self.base_mut().process_dialog_key(key)", None,
        "A dialog key the control did not take; return true to consume it.", "Une touche de dialogue non prise par le contrôle ; renvoyez true pour la consommer."),
    member("Control", "wnd_proc", "fn wnd_proc(&mut self, msg: &mut Message) -> bool", "self.base_mut().wnd_proc(msg)", None,
        "The message pre-filter: return true to consume a message.", "Le préfiltre des messages : renvoyez true pour consommer un message."),
    member("Control", "create_params", "fn create_params(&self) -> CreateParams", "self.base().create_params()", None,
        "The window options of a natively hosted control.", "Les options de fenêtre d'un contrôle hébergé nativement."),
    member("Control", "on_event", "fn on_event(&mut self, event: &'static str, e: &mut EventCx<'_, dyn EventArgs>)", "self.base_mut().on_event(event, e);", None,
        "An event the level has no method for (ItemActivate, StepSelected…).", "Un événement sans méthode propre au niveau (ItemActivate, StepSelected…)."),
];

const LEVEL_MEMBERS: [OverridableMember; 15] = [
    member("Component", "dispose_core", "fn dispose_core(&mut self, disposing: bool)", "self.base_mut().dispose_core(disposing);", None,
        "Releases what the component holds (WinForms Dispose(bool)).", "Libère ce que le composant détient (Dispose(bool) de WinForms)."),
    member("ButtonBase", "on_checked_changed", "fn on_checked_changed(&mut self, e: &mut EventCx<'_, CheckedChangedEventArgs>)", "self.base_mut().on_checked_changed(e);", Some("OnCheckedChanged"),
        "Raises CheckedChanged.", "Déclenche CheckedChanged."),
    member("TextBoxBase", "on_read_only_changed", "fn on_read_only_changed(&mut self, e: &mut EventCx<'_, EmptyEventArgs>)", "self.base_mut().on_read_only_changed(e);", Some("OnReadOnlyChanged"),
        "Raises ReadOnlyChanged.", "Déclenche ReadOnlyChanged."),
    member("ListControl", "on_selection_changed", "fn on_selection_changed(&mut self, e: &mut EventCx<'_, SelectionChangedEventArgs>)", "self.base_mut().on_selection_changed(e);", Some("OnSelectionChanged"),
        "Raises SelectionChanged.", "Déclenche SelectionChanged."),
    member("ListControl", "on_selected_value_changed", "fn on_selected_value_changed(&mut self, e: &mut EventCx<'_, TextChangedEventArgs>)", "self.base_mut().on_selected_value_changed(e);", Some("OnSelectedValueChanged"),
        "Raises SelectedValueChanged.", "Déclenche SelectedValueChanged."),
    member("RangeBase", "on_value_changed", "fn on_value_changed(&mut self, e: &mut EventCx<'_, NumericValueChangedEventArgs>)", "self.base_mut().on_value_changed(e);", Some("OnValueChanged"),
        "Raises ValueChanged.", "Déclenche ValueChanged."),
    member("RangeBase", "on_scroll", "fn on_scroll(&mut self, e: &mut EventCx<'_, ScrollEventArgs>)", "self.base_mut().on_scroll(e);", Some("OnScroll"),
        "Raises Scroll.", "Déclenche Scroll."),
    member("ScrollableControl", "on_scroll", "fn on_scroll(&mut self, e: &mut EventCx<'_, ScrollEventArgs>)", "self.base_mut().on_scroll(e);", Some("OnScroll"),
        "Raises Scroll.", "Déclenche Scroll."),
    member("UserControl", "on_load", "fn on_load(&mut self, e: &mut EventCx<'_, EmptyEventArgs>)", "self.base_mut().on_load(e);", Some("OnLoad"),
        "Raises Load, before the user control is first shown.", "Déclenche Load, avant le premier affichage du contrôle utilisateur."),
    member("View", "on_load", "fn on_load(&mut self, e: &mut EventCx<'_, EmptyEventArgs>)", "self.base_mut().on_load(e);", Some("OnLoad"),
        "Raises Load, before the view is first shown.", "Déclenche Load, avant le premier affichage de la vue."),
    member("View", "on_shown", "fn on_shown(&mut self, e: &mut EventCx<'_, EmptyEventArgs>)", "self.base_mut().on_shown(e);", Some("OnShown"),
        "Raises Shown.", "Déclenche Shown."),
    member("View", "on_activated", "fn on_activated(&mut self, e: &mut EventCx<'_, EmptyEventArgs>)", "self.base_mut().on_activated(e);", Some("OnActivated"),
        "Raises Activated.", "Déclenche Activated."),
    member("View", "on_deactivate", "fn on_deactivate(&mut self, e: &mut EventCx<'_, EmptyEventArgs>)", "self.base_mut().on_deactivate(e);", Some("OnDeactivate"),
        "Raises Deactivate.", "Déclenche Deactivate."),
    member("View", "on_form_closing", "fn on_form_closing(&mut self, e: &mut EventCx<'_, FormClosingEventArgs>)", "self.base_mut().on_form_closing(e);", Some("OnFormClosing"),
        "Raises FormClosing; set cancel to keep the window.", "Déclenche FormClosing ; définissez cancel pour garder la fenêtre."),
    member("View", "on_form_closed", "fn on_form_closed(&mut self, e: &mut EventCx<'_, FormClosedEventArgs>)", "self.base_mut().on_form_closed(e);", Some("OnFormClosed"),
        "Raises FormClosed.", "Déclenche FormClosed."),
];

/// Every overridable member: the `Control` hooks, the `Control` event methods, then the other
/// levels' members.
pub fn all() -> Vec<OverridableMember> {
    CONTROL_HOOKS.iter().chain(CONTROL_EVENTS.iter()).chain(LEVEL_MEMBERS.iter()).copied().collect()
}

/// The members a class whose chain is `chain` (`["RoundButton", "Button", "ButtonBase",
/// "Control", "Component"]`) can override, grouped in `chain` order (nearest level first).
pub fn for_chain(chain: &[&str]) -> Vec<OverridableMember> {
    let all = all();
    let mut out = Vec::new();
    for level in chain {
        out.extend(all.iter().filter(|m| m.level == *level).copied());
    }
    out
}

/// The method `member` overriding its base (`indent` before each line, 4 spaces per level
/// inside): `fn on_click(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {\n    self.base_mut()
/// .on_click(e);\n}\n`.
pub fn stub(member: &OverridableMember, indent: &str) -> String {
    format!("{indent}{} {{\n{indent}    {}\n{indent}}}\n", member.signature, member.base_call)
}

#[cfg(test)]
#[path = "overrides_fixture.rs"]
mod overrides_fixture;

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders every member of `level` the way the fixture file writes them.
    fn render(level: &str) -> String {
        all().iter().filter(|m| m.level == level).map(|m| stub(m, "    ")).collect::<Vec<_>>().join("\n")
    }

    /// The fixture (`overrides_fixture.rs`, compiled as a test module) holds, for each level, an
    /// `impl` overriding every member exactly as [`stub`] writes it: the catalogue's signatures and
    /// base calls compile against the real traits.
    #[test]
    fn every_member_compiles_as_the_stub_writes_it() {
        let fixture = include_str!("overrides_fixture.rs").replace("\r\n", "\n");
        for level in ["Component", "Control", "ButtonBase", "TextBoxBase", "ListControl", "RangeBase", "ScrollableControl", "UserControl", "View"] {
            let rendered = render(level);
            assert!(fixture.contains(&rendered), "the fixture's `impl {level}` differs from the catalogue:\n{rendered}");
        }
        assert_eq!(all().len(), 33 + 14 + 15);
    }

    #[test]
    fn members_follow_the_chain() {
        let chain = ["RoundButton", "Button", "ButtonBase", "Control", "Component"];
        let members = for_chain(&chain);
        assert_eq!(members.first().map(|m| m.name), Some("on_checked_changed"), "the nearest level first");
        assert!(members.iter().any(|m| m.name == "on_paint"));
        assert!(!members.iter().any(|m| m.level == "ListControl"));
        assert_eq!(members.last().map(|m| m.name), Some("dispose_core"));
        let s = stub(&members[0], "    ");
        assert_eq!(s, "    fn on_checked_changed(&mut self, e: &mut EventCx<'_, CheckedChangedEventArgs>) {\n        self.base_mut().on_checked_changed(e);\n    }\n");
    }

    /// Regenerates `overrides_fixture.rs` from the catalogue (then build and review the diff): run
    /// with `cargo test -p kubuno-desktop-views write_overrides_rust_fixture -- --ignored`.
    #[test]
    #[ignore]
    fn write_overrides_rust_fixture() {
        let classes: &[(&str, &str, &str, &[&str])] = &[
            ("AllButton", "Button", "base: Button", &["Component", "Control", "ButtonBase"]),
            ("AllText", "TextField", "base: TextField", &["TextBoxBase"]),
            ("AllList", "ListBox", "base: ListBox", &["ListControl"]),
            ("AllRange", "Slider", "base: Slider", &["RangeBase"]),
            ("AllScroll", "ScrollArea", "base: ScrollArea", &["ScrollableControl"]),
            ("AllUser", "UserControl", "base: UserControlCore", &["UserControl"]),
            ("AllView", "View", "base: ViewCore", &["View"]),
        ];
        let mut out = String::from(
            "//! Generated by `overrides::tests::write_overrides_rust_fixture`: one class per level overriding\n\
             //! every member of the catalogue exactly as `overrides::stub` writes it (compiled by the tests).\n\n\
             #![allow(dead_code)]\n\n\
             use crate::prelude::*;\n\n",
        );
        for (class, base, field, levels) in classes {
            out.push_str(&format!("#[derive(Component, Default)]\n#[kubuno(extends = {base}, overrides({}))]\npub struct {class} {{\n    {field},\n}}\n\n", levels.join(", ")));
            for level in *levels {
                out.push_str(&format!("impl {level} for {class} {{\n{}}}\n\n", render(level)));
            }
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/component/overrides_fixture.rs");
        std::fs::write(path, out.trim_end().to_string() + "\n").unwrap();
    }

    /// Writes the catalogue as JSON for Visual Studio (`Kubuno.Desktop.Logic`'s embedded
    /// `OverridableMembers.json`). Run with `cargo test -p kubuno-desktop-views write_overrides_fixture --
    /// --ignored` after changing the catalogue.
    #[test]
    #[ignore]
    fn write_overrides_fixture() {
        use crate::component::{
            ButtonBaseCore, ComponentCore, ContainerBaseCore, ContainerControlCore, ControlCore, LabelBaseCore, Lineage, ListControlCore, RangeBaseCore, ScrollableControlCore, TextBoxBaseCore,
            UserControlCore, ViewCore,
        };
        // The chains of the built-in classes and of the levels (what `extends` can name).
        let mut chains: Vec<&[&str]> = crate::controls::CLASSES.iter().map(|c| c.chain).collect();
        chains.extend([
            ComponentCore::CHAIN,
            ControlCore::CHAIN,
            ScrollableControlCore::CHAIN,
            ContainerControlCore::CHAIN,
            UserControlCore::CHAIN,
            ViewCore::CHAIN,
            ButtonBaseCore::CHAIN,
            TextBoxBaseCore::CHAIN,
            ListControlCore::CHAIN,
            LabelBaseCore::CHAIN,
            ContainerBaseCore::CHAIN,
            RangeBaseCore::CHAIN,
        ]);
        // `UserControl` is both an element (its class is the level's core) and a level: listed once.
        chains.dedup_by(|a, b| a[0] == b[0]);
        let mut seen = std::collections::HashSet::new();
        chains.retain(|c| seen.insert(c[0]));
        let mut json = String::from("{\n  \"chains\": {\n");
        for (i, chain) in chains.iter().enumerate() {
            let items: Vec<String> = chain.iter().map(|n| format!("\"{n}\"")).collect();
            json.push_str(&format!("    \"{}\": [{}]{}\n", chain[0], items.join(", "), if i + 1 < chains.len() { "," } else { "" }));
        }
        json.push_str("  },\n  \"members\": [\n");
        let members = all();
        for (i, m) in members.iter().enumerate() {
            let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
            json.push_str(&format!(
                "    {{ \"level\": \"{}\", \"name\": \"{}\", \"signature\": \"{}\", \"base_call\": \"{}\", \"event\": {}, \"doc\": \"{}\", \"doc_fr\": \"{}\" }}{}\n",
                m.level,
                m.name,
                esc(m.signature),
                esc(m.base_call),
                m.event.map_or("null".to_string(), |e| format!("\"{e}\"")),
                esc(m.doc),
                esc(m.doc_fr),
                if i + 1 < members.len() { "," } else { "" }
            ));
        }
        json.push_str("  ]\n}\n");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../../vskubuno/src/Desktop/Kubuno.Desktop.Logic/Overrides/OverridableMembers.json");
        std::fs::write(&path, json).unwrap();
    }
}
