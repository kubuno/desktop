//! # kubuno-desktop-controls — the Kubuno desktop control library
//!
//! A native reproduction of the **WinForms control surface** on Direct2D, built
//! to be the single set of primitives every Kubuno desktop application uses.
//!
//! ## What this is, and why it is shaped this way
//!
//! The reference is not prose: it is the shipping `System.Windows.Forms`
//! assembly, read by reflection (65 control types, their inheritance chain and
//! every designer-visible property with its declared default), and rendered by
//! the real toolkit into per-family reference sheets. `tools/winforms-ref/`
//! holds both generators and explains how to regenerate them;
//! `docs/AGENT_BRIEF.md` is the contract every control family is written to.
//!
//! ## The inheritance chain is the design
//!
//! `Control` declares 52 settable properties that all 64 other control types
//! inherit. `ButtonBase` adds 18 shared by `Button`, `CheckBox` and
//! `RadioButton` — and `Button` itself declares only **two**. A port that
//! re-implements each control from scratch would duplicate that surface 65
//! times and let it drift.
//!
//! So the chain is mirrored with **composition + `Deref`**: each type owns
//! exactly the properties its .NET counterpart *declares*, and derefs to its
//! base for the rest. Adding a property to `ControlBase` gives it to every
//! control, once — as it does in the toolkit.
//!
//! Two subtleties the counts hide, both of which the port respects:
//!
//! * A subclass often **re-declares** a property its base already owns, only to
//!   change an attribute — `ButtonBase` re-declares `Text`, `BackColor` and
//!   `AutoSize`. That is not new storage: the value stays on `ControlBase`, and
//!   the subclass may only give it a different **default** (`RadioButton`
//!   starts with `TabStop = false` where `Control` starts `true`).
//! * The counts below are the **designer-visible** ones from the JSON
//!   catalogue. `winforms-hierarchy.txt` prints a raw count that includes
//!   non-browsable members, so the two differ by a few; the JSON is the source
//!   of truth.
//!
//! ```ignore
//! Control                (control::ControlBase)   52 props
//!   ButtonBase           (buttons::ButtonBase)    +18
//!     Button                                       +2
//!     CheckBox                                     +7
//!     RadioButton                                  +6
//!   TextBoxBase                                   +23
//!     TextBox / MaskedTextBox / RichTextBox
//!   ListControl                                    +8
//!     ComboBox / ListBox → CheckedListBox
//!   ScrollableControl                              +8
//!     ContainerControl                             +7  → Form, UserControl, …
//!     Panel                                        +5  → FlowLayoutPanel, TabPage, …
//! ```
//!
//! ## What a control may and may not do
//!
//! * It paints through [`kubuno_drive_desktop_app_controls::Canvas`] only — never its own
//!   device, window or font object. Colours come from the theme, text from the
//!   shared formats; a control that hard-codes either is wrong.
//! * It owns no timer, thread or I/O.
//! * Its geometry is pure: [`layout`] resolves `Dock` then `Anchor` as free
//!   functions, so container behaviour is tested without a message loop.
//! * A property the toolkit offers is either honoured or explicitly documented
//!   as not-yet-honoured. It is never silently treated as another value.

// ── The foundation ───────────────────────────────────────────────────────────
pub mod host;
pub mod control;
pub mod enums;
pub mod layout;
/// The system's own colours, metrics and UI font — what the controls paint
/// with, so a replica matches the toolkit rather than the Kubuno design system.
pub mod system;
/// The **themed** parts, rendered by `uxtheme.dll` itself — what the controls
/// paint with when visual styles are on, which is how the reference sheets were
/// made. [`system`] stays the fallback for a themed-off machine.
pub mod theme;
/// Painting part of a window in other colours, another font or with an image (a control's
/// `BackColor`/`ForeColor`/`Font`/`BackgroundImage`), system colours and high contrast.
pub mod styled;
/// Icons drawn from image files (SVG, PNG, JPEG, BMP, GIF, ICO, TIFF, WebP) and icons rasterized off
/// screen (the window icon, the designer's previews).
pub mod icon_image;
/// The Kubuno window chrome (title band, caption buttons, footer, grip): one painter for top-level
/// windows, dialogs, the designer's picture of a form and in-window floating windows.
pub mod window_chrome;

// ── The control families, one module per shared base ─────────────────────────
// Each module owns the whole subtree under one .NET base class, because that
// base is exactly what must be implemented once and reused.
pub mod buttons;       // ButtonBase → Button, CheckBox, RadioButton
pub mod containers;    // ScrollableControl → ContainerControl, Panel, GroupBox, Form
pub mod datetime;      // DateTimePicker, MonthCalendar
pub mod labels;        // Label → LinkLabel; PictureBox; ProgressBar
pub mod layout_panels; // FlowLayoutPanel, TableLayoutPanel, SplitContainer, TabControl
pub mod lists;         // ListControl → ComboBox, ListBox → CheckedListBox
pub mod range;         // ScrollBar → H/V; TrackBar; UpDownBase → Numeric/Domain
pub mod text;          // TextBoxBase → TextBox, MaskedTextBox, RichTextBox
pub mod toolstrip;     // ToolStrip → MenuStrip, StatusStrip, ContextMenuStrip
pub mod views;         // TreeView, ListView

pub use control::{Control, ControlBase, ControlCanvas, ControlClone, ControlState, FontRole};
pub use enums::{
    AccessibleRole, Appearance, AnchorStyles, AutoSizeMode, BorderStyle, CheckState,
    ContentAlignment, DockStyle, FlatStyle, HorizontalAlignment, ImageLayout, ImeMode,
    LeftRightAlignment, Padding, RightToLeft, ScrollBars, Size,
};
pub use layout::{layout, Item};
pub use system::{Border3DSide, Border3DStyle, SystemColors, SystemFonts, SystemMetrics, Visuals};
pub use theme::{CachedPart, ThemeCacheStats, ThemeClass, ThemeRenderer};

// Re-exported so a control module needs one `use` for the drawing surface.
pub use kubuno_drive_desktop_app_controls::{Canvas, Rect};
