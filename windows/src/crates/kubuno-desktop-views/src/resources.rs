//! `{Res key}` — resource references in views (`vskubuno/docs/RESOURCES.md`).
//!
//! ```xml
//! <Button Text="{Res save_text}" Image="{Res save_icon}"/>
//! <Label Text="{Res title, Source=strings}" ForeColor="{Res accent}" Font="{Res heading}"/>
//! <Panel BackgroundImage="{Res banner}"/>
//! ```
//!
//! A reference is parsed like a binding (so every bindable property takes one, with the same
//! machinery) into a one-way [`BindingSpec`] whose path starts with [`RES_PREFIX`] — a path no view
//! model can hold. It is resolved by [`kubuno_desktop_resources::lookup`] each frame, in the current UI
//! culture: a string property gets the text, a colour or font property the `Value` text, an image
//! property the URI `kbres:<set>/<name>`, which `kubuno_desktop_controls::styled::load_image` resolves when
//! painting. A culture switch ([`kubuno_desktop_resources::set_culture`]) repaints every window of the
//! process ([`install`]), so the whole UI changes language live.

use crate::binding::{BindingMode, BindingSpec, Value, ViewModel};
use crate::format::ValueKind;
use kubuno_desktop_resources::{Kind, ResolvedValue};

// The `{Res …}` grammar lives in the platform-neutral `kubuno-desktop-views-syntax` (`res`, WV-1),
// re-exported here under its historical paths; this module resolves references at run time.
pub use kubuno_desktop_views_syntax::res::{is_res_expr, RES_PREFIX};

/// Parses the inside of `{Res key[, Source=set]}` (without the braces); `None` when it is not a
/// `Res` expression. `Key=` may name the key explicitly; `Source=` is the set (the `.kbres` file stem).
pub fn parse_res(inner: &str) -> Option<BindingSpec> {
    let path = kubuno_desktop_views_syntax::res::parse_res_path(inner)?;
    Some(BindingSpec { path, mode: BindingMode::OneWay, ..BindingSpec::default() })
}

/// `(set, key)` of a resource reference's spec; `None` for an ordinary binding.
pub fn reference(spec: &BindingSpec) -> Option<(Option<&str>, &str)> {
    kubuno_desktop_views_syntax::res::res_reference(&spec.path)
}

/// The value `spec` gives in `want` form: a resource when it is one, else what `vm` holds.
pub fn get_bound(vm: &dyn ViewModel, spec: &BindingSpec, want: ValueKind) -> Option<Value> {
    match reference(spec) {
        Some((set, key)) => crate::format::to_target(value_of(set, key)?, want, &spec.format),
        None => vm.get_bound(spec, want),
    }
}

/// The raw value of `spec`: a resource when it is one, else `vm.get(path)`.
pub fn get(vm: &dyn ViewModel, spec: &BindingSpec) -> Option<Value> {
    match reference(spec) {
        Some((set, key)) => value_of(set, key),
        None => vm.get(&spec.path),
    }
}

/// The value of resource `key` (of `set`) for a property: the text of a text entry, the URI of an
/// image or icon, a text file's text.
pub fn value_of(set: Option<&str>, key: &str) -> Option<Value> {
    match kubuno_desktop_resources::lookup(None, set, key)? {
        ResolvedValue::Text { text, .. } => Some(Value::Str(text)),
        ResolvedValue::Bytes { kind: Kind::Image | Kind::Icon, .. } => Some(Value::Str(kubuno_desktop_resources::uri(set.unwrap_or_default(), key))),
        v => v.text().map(Value::Str),
    }
}

/// The `kbres:` URI of an image attribute written `{Res key}` (or already a `kbres:` URI); `None`
/// for a path or a glyph name.
pub fn image_uri(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.starts_with(kubuno_desktop_resources::URI_SCHEME) {
        return Some(raw.to_string());
    }
    let spec = parse_res(raw.strip_prefix('{')?.strip_suffix('}')?)?;
    let (set, key) = reference(&spec)?;
    Some(kubuno_desktop_resources::uri(set.unwrap_or_default(), key))
}

/// Connects the view layer to the resources once per process: `kbres:` images are resolved by
/// `kubuno_desktop_controls`' image loader, and a culture change repaints every window of the process.
/// Called when a view runtime is created.
pub fn install() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        kubuno_desktop_controls::styled::set_resource_image_resolver(resolve_image);
        // Icons painted by name (`Icon="{Res app}"` on a button, the icon picker's values).
        kubuno_desktop_controls::icon_image::set_resource_loader(|uri| kubuno_desktop_resources::resolve_uri(uri).map(|(_, bytes, _)| bytes.to_vec()));
        kubuno_desktop_resources::on_culture_changed(|_| {
            // Decoded icons of the previous culture.
            kubuno_desktop_controls::icon_image::release();
            // The window being painted (a switch made by a handler, or by the designer's surface, whose
            // window is a child of Visual Studio's), then every top-level window of the process.
            kubuno_desktop_controls::host::request_repaint_after(0);
            repaint_process_windows();
        })
        .forget();
    });
}

/// The image loader's hook: the bytes of a `kbres:` URI in the current culture.
fn resolve_image(uri: &str, with: &mut dyn FnMut(u64, &[u8], &str)) -> bool {
    match kubuno_desktop_resources::resolve_uri(uri) {
        Some((id, bytes, format)) => {
            with(id, &bytes, &format);
            true
        }
        None => false,
    }
}

/// Invalidates every window (and child window) of this process, so it paints again with the
/// current culture's resources.
pub fn repaint_process_windows() {
    // Declared by hand (user32): the matching `windows` features are not this crate's.
    type EnumProc = unsafe extern "system" fn(hwnd: isize, lparam: isize) -> i32;
    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(proc_: EnumProc, lparam: isize) -> i32;
        fn EnumChildWindows(parent: isize, proc_: EnumProc, lparam: isize) -> i32;
        fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
        fn InvalidateRect(hwnd: isize, rect: *const core::ffi::c_void, erase: i32) -> i32;
    }
    unsafe extern "system" fn child(hwnd: isize, _: isize) -> i32 {
        // SAFETY: a window handle Windows handed to the enumeration callback.
        unsafe { InvalidateRect(hwnd, std::ptr::null(), 0) };
        1
    }
    unsafe extern "system" fn top(hwnd: isize, me: isize) -> i32 {
        let mut pid = 0u32;
        // SAFETY: as above; `pid` is a valid out pointer.
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid as isize == me {
                InvalidateRect(hwnd, std::ptr::null(), 0);
                EnumChildWindows(hwnd, child, 0);
            }
        }
        1
    }
    // SAFETY: plain enumeration with callbacks that only invalidate.
    unsafe {
        EnumWindows(top, std::process::id() as isize);
    }
}

/// The resource tests share the process-wide culture: one at a time.
#[cfg(test)]
pub(crate) static TEST_CULTURE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binding::{parse_binding, MapViewModel, PropSource};

    static NEUTRAL: &str = r##"<Resources><String Name="title">Hello</String><Color Name="accent" Value="#3366FF"/><String Name="size">12.5</String><Image Name="logo" Format="png">AAEC</Image></Resources>"##;
    static FR: &str = r#"<Resources><String Name="title">Bonjour</String></Resources>"#;
    static EMBEDDED: kubuno_desktop_resources::EmbeddedSet = kubuno_desktop_resources::EmbeddedSet { name: "viewres", neutral: NEUTRAL, satellites: &[("fr", FR)], files: &[] };
    static SET: kubuno_desktop_resources::StaticSet = kubuno_desktop_resources::StaticSet::new(&EMBEDDED);

    #[test]
    fn parses_res_expressions() {
        let spec = parse_binding("{Res title}").expect("res");
        assert_eq!(reference(&spec), Some((None, "title")));
        let spec = parse_binding("{Res title, Source=strings.kbres}").expect("res with source");
        assert_eq!(reference(&spec), Some((Some("strings"), "title")));
        assert_eq!(reference(&parse_binding("{Res Key=okButton.Text}").expect("key=")), Some((None, "okButton.Text")));
        assert!(parse_binding("{Resx title}").is_none());
        assert!(parse_binding("{Res}").is_none());
        assert!(reference(&parse_binding("{Binding title}").expect("binding")).is_none());
        assert!(is_res_expr(" {Res a} ") && !is_res_expr("{Binding a}"));
        assert_eq!(image_uri("{Res logo, Source=app}").as_deref(), Some("kbres:app/logo"));
        assert_eq!(image_uri("{Res logo}").as_deref(), Some("kbres:logo"));
        assert_eq!(image_uri("kbres:x/y").as_deref(), Some("kbres:x/y"));
        assert_eq!(image_uri("img/logo.png"), None);
    }

    /// A custom control's own property written `{Res key}` (`<AccentPill Text="{Res manage}"/>`)
    /// gets the resource, like a built-in one: its value is read through [`get`].
    #[test]
    fn a_custom_property_reads_its_resource() {
        let _g = TEST_CULTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        kubuno_desktop_resources::register_static(&SET);
        let parse = crate::syntax::parse(r#"<Panel Text="{Res size, Source=viewres}"/>"#);
        let element = { use crate::ast::AstNode; crate::ast::Document::cast(parse.syntax()).and_then(|d| d.root_element()).expect("element") };
        let meta = crate::registry::PropertyMeta::new("Text", crate::registry::PropKind::String, "", "");
        let prop = crate::design::CustomProp::read(&element, &meta).expect("set");
        assert_eq!(prop.resolve(&MapViewModel::default()), Some(Value::Str("12.5".into())));
    }

    /// A `{Res}` property follows a culture switch on the next frame, without rebuilding the view.
    #[test]
    fn res_bindings_refresh_on_culture_change() {
        let _g = TEST_CULTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        kubuno_desktop_resources::register_static(&SET);
        let vm = MapViewModel::default();
        let text: PropSource<String> = PropSource::Bound { spec: parse_binding("{Res title, Source=viewres}").expect("res"), fallback: String::new() };
        let size: PropSource<f32> = PropSource::Bound { spec: parse_binding("{Res size, Source=viewres}").expect("res"), fallback: 0.0 };
        let logo: PropSource<String> = PropSource::Bound { spec: parse_binding("{Res logo, Source=viewres}").expect("res"), fallback: String::new() };
        let missing: PropSource<String> = PropSource::Bound { spec: parse_binding("{Res nope, Source=viewres}").expect("res"), fallback: "fallback".into() };
        kubuno_desktop_resources::set_culture("fr-FR");
        assert_eq!(text.resolve(&vm), "Bonjour");
        kubuno_desktop_resources::set_culture("en-US");
        assert_eq!(text.resolve(&vm), "Hello");
        assert_eq!(size.resolve(&vm), 12.5);
        assert_eq!(logo.resolve(&vm), "kbres:viewres/logo");
        assert_eq!(missing.resolve(&vm), "fallback");
        assert_eq!(get(&vm, &parse_binding("{Res accent, Source=viewres}").expect("res")), Some(Value::Str("#3366FF".into())));
    }
}

#[cfg(test)]
mod protocol_tests {
    use crate::binding::{parse_binding, MapViewModel, PropSource};
    use crate::protocol::{parse_host_message, HostMessage};

    /// The designer's `setResources`: the project's files become loaded sets, shown in the design-time culture.
    #[test]
    fn set_resources_loads_the_project_sets_in_the_design_culture() {
        let _g = super::TEST_CULTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let line = r#"{"type":"setResources","culture":"fr","sets":[{"name":"designres","baseDir":"C:/p/src","neutral":"<Resources><String Name=\"t\">Title</String></Resources>","satellites":[{"culture":"fr","text":"<Resources><String Name=\"t\">Titre</String></Resources>"}]}]}"#;
        let parsed: Result<HostMessage, _> = serde_json::from_str(line);
        assert!(parsed.is_ok(), "{parsed:?}");
        let Some(HostMessage::SetResources { culture, sets }) = parse_host_message(line) else { panic!("not parsed") };
        crate::protocol::apply_resources(&culture, &sets);
        let vm = MapViewModel::default();
        let text: PropSource<String> = PropSource::Bound { spec: parse_binding("{Res t, Source=designres}").expect("res"), fallback: String::new() };
        assert_eq!(text.resolve(&vm), "Titre");
        crate::protocol::apply_resources("", &sets);
        assert_eq!(text.resolve(&vm), "Title", "(Default) shows the neutral values");
        kubuno_desktop_resources::replace_loaded(Vec::new());
        kubuno_desktop_resources::set_culture("en-US");
    }
}

#[cfg(test)]
mod button_image_tests {
    use crate::ast::{AstNode, Document};
    use crate::props::Props;

    /// A button's `Image="{Res key}"` reaches the icon pipeline as a `kbres:` URI (not joined to the view's folder).
    #[test]
    fn a_resource_image_of_a_button_is_a_kbres_uri() {
        let parse = crate::syntax::parse(r#"<Button Text="OK" Image="{Res app_icon}"/>"#);
        let element = Document::cast(parse.syntax()).and_then(|d| d.root_element()).expect("element");
        let meta = crate::registry::lookup("Button").expect("Button");
        let base = crate::common::ButtonBaseProps::read(&Props::new(&element, meta), Some(std::path::Path::new("C:/views")));
        assert_eq!(base.image.as_deref(), Some("kbres:app_icon"));
    }
}
