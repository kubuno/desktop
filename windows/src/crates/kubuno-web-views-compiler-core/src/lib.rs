//! # `kubuno-web-views-compiler-core` — the web target's `.kbview` compiler
//!
//! Lot WV-2 of `vskubuno/docs/WEB-VIEWS.md`. Given a view's text and the web element registry (the host's
//! `kbview-registry.web.json` from `@kubuno/ui`, the project's own control registries and its `.kbcontrol`
//! user controls), it:
//!
//! - parses with the shared grammar (`kubuno-desktop-views-syntax`: the desktop's own lossless parser) and validates
//!   against the registry (elements, properties, events, values, children models, `x:Name`, handlers,
//!   bindings, module isolation) — [`compile`];
//! - produces the **render plan** (`plan`) that the `@kubuno/views` runtime renders, as data;
//! - produces the generated declarations of the view (`X.kbview.d.ts`: `plan`, the abstract `ViewBase` with a
//!   typed handle per `x:Name` and an abstract method per handler) and its **check file** (`X.kbview.check.ts`:
//!   the bindings written as TypeScript against the code-behind, with a span map so `kbview-tsc` reports
//!   their errors at the `.kbview` attribute);
//! - produces the handle interfaces of the host elements for `@kubuno/views` ([`ts::handle_types`]).
//!
//! It is compiled to WebAssembly by `@kubuno/views-compiler` (core repository,
//! `frontend/packages/views-compiler/wasm`), which ships the `.wasm`: module builds need no Rust toolchain.
//! Platform-neutral: no UI, no OS API.

pub mod compile;
pub mod lines;
pub mod plan;
pub mod registry;
pub mod ts;

use serde::{Deserialize, Serialize};

pub use compile::{CompileOptions, Diagnostic, HandlerUse, NameInfo};
pub use plan::{Plan, VIEWS_ABI};
pub use registry::{UserControlRef, WebRegistry};
pub use ts::CheckSpan;

/// The compiler's own version (the git tag the core consumes is `views-web-v<VERSION>`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Everything one compile produces.
#[derive(Debug, Clone, Serialize)]
pub struct CompileOutput {
    /// No error-severity diagnostic.
    pub ok: bool,
    pub diagnostics: Vec<Diagnostic>,
    /// The render plan (present whenever the view has a root element, even with errors).
    pub plan: Option<Plan>,
    /// `X.kbview.d.ts`.
    pub dts: String,
    /// `X.kbview.check.ts`.
    pub check: String,
    /// Span map of `check` (generated → `.kbview`).
    pub check_map: Vec<CheckSpan>,
    pub names: Vec<NameInfo>,
    pub handlers: Vec<HandlerUse>,
    /// The class the code-behind must export.
    pub class_name: String,
    /// Plan ABI and compiler version.
    pub abi: u32,
    pub compiler: &'static str,
}

/// Compiles one view.
pub fn compile(source: &str, registry: &WebRegistry, options: &CompileOptions) -> CompileOutput {
    let mut c = compile::Compiler::new(source, registry, options);
    let plan = c.run();
    let dts = ts::dts(options, &c.facts);
    let (check, check_map) = ts::check(options, &c.facts);
    let mut diagnostics = std::mem::take(&mut c.diagnostics);
    diagnostics.sort_by_key(|d| (d.line, d.column));
    CompileOutput {
        ok: !diagnostics.iter().any(|d| d.severity == "error"),
        diagnostics,
        plan,
        dts,
        check,
        check_map,
        names: c.facts.names.clone(),
        handlers: c.facts.handlers.clone(),
        class_name: ts::class_name(options),
        abi: VIEWS_ABI,
        compiler: VERSION,
    }
}

/// One registry document handed to a [`Session`].
#[derive(Debug, Clone, Deserialize)]
pub struct RegistryInput {
    /// The JSON text (VIEWS-SPEC §10).
    pub json: String,
    /// Shown in messages (`@kubuno/ui/kbview-registry.web.json`).
    pub label: String,
    /// A host registry (`@kubuno/ui`): may only name host modules. A project registry may also name
    /// project-local modules (`./controls`, `/src/controls`).
    pub host: bool,
}

/// A compile request: registries are loaded once per session and reused across compiles.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CompileRequest {
    pub source: String,
    pub options: CompileOptions,
}

/// A registry loaded once, then many compiles (the Vite plugin keeps one per dev server).
#[derive(Debug, Clone, Default)]
pub struct Session {
    documents: Vec<(kubuno_desktop_views_model::RegistryDocument, String, bool)>,
    user_controls: Vec<UserControlRef>,
    pub registry: WebRegistry,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a registry document; a JSON or schema error is returned (and the document is skipped).
    /// Returns how many elements the registry has now.
    pub fn add_registry(&mut self, input: &RegistryInput) -> Result<usize, String> {
        let doc = kubuno_desktop_views_model::load_registry(&input.json).map_err(|e| format!("{}: {e}", input.label))?;
        self.documents.push((doc, input.label.clone(), input.host));
        self.rebuild();
        Ok(self.registry.elements().count())
    }

    /// Replaces the project's user controls (the caller re-scans `*.kbcontrol` files).
    pub fn set_user_controls(&mut self, controls: &[UserControlRef]) {
        self.user_controls = controls.to_vec();
        self.rebuild();
    }

    fn rebuild(&mut self) {
        let mut registry = WebRegistry::new();
        for (doc, label, host) in &self.documents {
            registry.add_document(doc, label, *host);
        }
        registry.add_user_controls(&self.user_controls);
        self.registry = registry;
    }

    pub fn compile(&self, request: &CompileRequest) -> CompileOutput {
        compile(&request.source, &self.registry, &request.options)
    }

    /// The handle interfaces of every element of the registry (`@kubuno/views`).
    pub fn handle_types(&self) -> String {
        ts::handle_types(&self.registry)
    }
}
