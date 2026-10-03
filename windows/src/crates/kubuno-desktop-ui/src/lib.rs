//! # `kubuno-desktop-ui` — the Kubuno desktop design system
//!
//! Two layers already exist under this one, and this crate is what joins them:
//!
//! * [`kubuno_desktop_controls`] reproduces the **WinForms control surface** — every
//!   property .NET declares, the defaults it ships, the state machines it
//!   implements (three-state check cycling, scrollbar arithmetic, tab
//!   selection, date grids) and the layout engines (Dock/Anchor, Flow, Table,
//!   Split), all checked against the real toolkit.
//! * [`kubuno_drive_desktop_app_controls`] is the **painting surface** every Kubuno desktop app
//!   already draws through: a [`Canvas`] carrying the resolved theme, the
//!   shared DirectWrite formats and the vector-icon geometries.
//!
//! A Kubuno primitive is therefore **not a new control**. It is a replica —
//! kept whole, for its model and its geometry — with the *pixels* replaced by
//! the Kubuno look.
//!
//! ```text
//!   kubuno_desktop_ui::Button
//!     ├── inner: kubuno_desktop_controls::buttons::Button   ← 52 + 19 + 2 properties,
//!     │                                               defaults, AutoSize rules
//!     ├── variant / size                            ← what Kubuno adds
//!     └── paint(&dyn Canvas, …)                     ← Kubuno pixels
//! ```
//!
//! ## The five rules
//!
//! 1. **Own a replica, never restate it.** A primitive holds its .NET
//!    counterpart and [`Deref`](std::ops::Deref)s to it, so
//!    `button.text`, `button.enabled`, `button.padding` are the replica's
//!    fields — one storage location, no drift. A field is added here *only*
//!    when Kubuno adds a concept .NET does not have (`variant`, `size`).
//! 2. **Take geometry and state from the replica.** Checkbox cycling,
//!    scrollbar thumb arithmetic, flow wrapping, anchor resolution: all of it
//!    is already implemented and tested against the toolkit. Re-deriving it
//!    here would be re-deriving the bugs too.
//! 3. **Paint through [`Canvas`] only.** Colours come from
//!    [`Canvas::theme`], type from [`Canvas::formats`], glyphs from
//!    `vector_icon`. No `Visuals`, no `uxtheme`, and no colour literal in a
//!    paint body — the replica layer owns the system look, this one owns the
//!    Kubuno look, and the two must not leak into each other.
//! 4. **Never multiply by [`Canvas::scale`].** The renderer sets the D2D dpi,
//!    so every coordinate here is already a DIP. Scaling again is the single
//!    most expensive mistake this codebase has made.
//! 5. **One metric table.** Heights, radii, paddings and gaps live in
//!    [`metrics`], never as literals in a paint body — a design change must be
//!    one edit, not a search.
//!
//! ## Not regressing what ships
//!
//! Several of these primitives already exist, hand-written, in
//! [`kubuno_drive_desktop_app_controls`] (`button`, `switch`, `edit_box`, `scrollbar`,
//! `toolbar`, `sidebar`, `breadcrumb_bar`…) and are what the shipping shell
//! and Drive paint with today. Rebuilding them on the replica base is only
//! worth doing if the result is **indistinguishable**: for every primitive
//! that has a predecessor, `examples/parity_skin.rs` paints old and new from
//! the same inputs and the pair is compared pixel-for-pixel before any app is
//! migrated.

pub mod focus; // FocusRing: Tab order, click-to-focus, :focus-visible
/// The WinForms-`Graphics`-like drawing API (paths, gradients, pens, text layout, clip,
/// transforms) and owner-draw (`DrawItem`/`MeasureItem`) — see the module doc.
pub mod graphics;
pub mod metrics;
pub mod mnemonic; // the underlined shortcut letter of a `&Save` text
pub mod widget;

// One module per family. Each owns its own file: the families are built in
// parallel and nothing forces two of them to touch the same source.
pub mod buttons; // Button, IconButton, CheckBox, RadioButton, Switch
pub mod containers; // Panel, Card, GroupBox, Splitter, ScrollView, ScrollArea
pub mod display; // Label, LinkLabel, Badge, Icon, Tooltip, Separator
pub mod lists; // ListBox, ComboBox, Menu
pub mod navigation; // Toolbar, Sidebar, Breadcrumb, Tabs, StatusBar
pub mod range; // ScrollBar, Slider, ProgressBar, NumericField
pub mod text; // TextField, TextArea, SearchField
pub mod views; // TreeView, ListView

// Second wave: the families the first pass left out. The inventory that found
// them is worth keeping in mind — the first eight were chosen from the WinForms
// replica list, which is why `datetime` was missed despite its replica already
// existing, and why everything the *web* has and WinForms does not (colour
// pickers, the data table, dialogs, toasts) was missed entirely.
pub mod color; // ColorField, ColorPicker, SwatchPicker, GradientPicker
pub mod datetime; // DatePicker, MonthCalendar, TimePicker
pub mod dialogs; // ConfirmDialog, PromptDialog, FloatingWindow, Popover, Toast
pub mod editors; // Dropdown, Editable, FontPicker, FontSizeField, RichText
pub mod feedback; // Spinner, EmptyState, Callout, Accordion, Stepper
pub mod fields; // OutlinedField and the Contacts-style composites built on it
pub mod help; // HelpBubble and the « ? » HelpButton that opens it
pub mod richtext; // RichText editor (placeholder, see the module)
pub mod ribbon; // Ribbon (Office command surface), Backstage
pub mod tables; // DataTable

// The workspace family: a port of `core/frontend/src/core/shell/workspace/`.
pub mod dock; // DockArea: dockable / floating panels, guide diamond, persistence
pub mod workspace; // WorkspaceShell, MenuBar, WorkspaceTheme

// The startup window of every Kubuno desktop app, with its procedural artwork.
pub mod splash; // SplashScreen, Splash, Artwork

pub use focus::{FocusCause, FocusChange, FocusId, FocusOpts, FocusRing, FocusState};
pub use widget::{Widget, WidgetState};

// Re-exported so a consumer needs one crate, not three: everything a Kubuno
// app paints with is reachable from here.
pub use kubuno_drive_desktop_app_controls::{Canvas, Rect, Theme, ThemeMode};
/// The diagnostics sink of a GUI application (no console): see `kubuno_desktop_controls::host::diagnostics`.
pub use kubuno_desktop_controls::host::diagnostics;
pub use kubuno_desktop_controls::{
    enums::{AnchorStyles, ContentAlignment, DockStyle, Padding, Size},
    layout::{layout, Item},
};
