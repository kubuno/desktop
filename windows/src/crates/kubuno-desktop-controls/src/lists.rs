//! `ListControl` and its subtree: `ComboBox`, and `ListBox → CheckedListBox`.
//!
//! ## Why this shape
//!
//! `ListControl` is the abstract base that WinForms puts *between* `Control` and
//! the two list widgets. It is where the toolkit parks the data-binding surface
//! (`DataSource`/`DisplayMember`/`ValueMember`/`SelectedValue`) and the one piece
//! of shared selection state every list has — `SelectedIndex`. It is abstract, so
//! it is modelled here as [`ListControlBase`] and never constructed on its own.
//!
//! The chain is mirrored with composition + `Deref`, exactly as `control.rs`
//! describes:
//!
//! ```ignore
//! ListControl  (ListControlBase { control: ControlBase, +7 })
//!   ComboBox   ({ base: ListControlBase, +22 })
//!   ListBox    ({ base: ListControlBase, +21 })
//!     CheckedListBox ({ list: ListBox, +7 })   // derives from ListBox, so it
//!                                               // COMPOSES a ListBox, not a
//!                                               // ListControlBase.
//! ```
//!
//! The declared-property counts come from the reflection catalogue, not prose:
//! `ListControl` declares **7**, `ComboBox` **22**, `ListBox` **21**,
//! `CheckedListBox` **7**. Several of those are `[Browsable]` re-declarations of a
//! property a base already owns (a `ComboBox` re-surfaces `Control.BackColor`,
//! `Control.Text`, `ListControl.DataSource`…). An override is not a new field: it
//! is honoured through the inherited `ControlBase`/`ListControlBase` slot, and
//! each such property is called out in a comment below rather than duplicated.
//!
//! ## Selection is state, not paint
//!
//! Everything a test wants to check about a list — the items, `SelectedIndex` /
//! `SelectedIndices`, the four `SelectionMode` rules, `TopIndex`, the integral
//! height, and a `CheckedListBox`'s per-item `CheckState` — is modelled as plain
//! data with pure methods. `paint` only reads that state. So the state machines
//! are unit-tested with no canvas at all.
//!
//! ## What it paints with
//!
//! The **system** — [`crate::system`] — for every colour, length and glyph, and
//! [`crate::theme`] for the four pieces of chrome the system colours cannot
//! reach. Each of those four was chosen by rendering the candidate parts and
//! sampling them against `C:\kubuno-build\winforms-ref\shots\03-listcontrol.png`,
//! never by reading a part's name (the rule [`crate::theme`] states, and the one
//! `EP_EDITBORDER_NOSCROLL` broke for the text family):
//!
//! | what | class / part / state | sampled | on the sheet |
//! |---|---|---|---|
//! | list frame | `EDIT` / `EP_EDITTEXT` / `ETS_*` | `#ABADB3` over `#FFFFFF` | `#ABADB3` |
//! | editable combo frame | `COMBOBOX` / `CP_BORDER` / `CBB_FOCUSED` | `#0078D4` over `#FFFFFF` | `#0078D4` |
//! | `DropDownList` face | `COMBOBOX` / `CP_READONLY` / `CBRO_NORMAL` | `#D2D2D2` top, `#FDFDFD` face, `#BCBCBC` bottom | the same three |
//! | drop-down button | `COMBOBOX` / `CP_DROPDOWNBUTTONRIGHT` / `CBXSR_*` | a bare chevron, **no face** | a bare chevron |
//! | check well | `BUTTON` / `BP_CHECKBOX` / `CBS_*` | `#626262`+`#F3F3F3`, `#005FB8`, `#C3C3C3` | the same three |
//!
//! Each is drawn behind `if !c.draw_theme_part(…)`, so a machine with visual
//! styles **off** — where the classic `DrawEdge` bevel is not a fallback but the
//! only correct rendering — keeps exactly the painting it had. `KUBUNO_CONTROLS_CLASSIC`
//! ([`crate::theme::CLASSIC_ENV`]) exercises that branch on a themed machine.
//!
//! ### The two parts that were measured and REFUSED
//!
//! **`LISTVIEW` / `LVP_LISTITEM` for the selected row.** It is the obvious part,
//! and it is wrong twice over. Opened the way [`crate::theme`] opens a class —
//! `OpenThemeDataForDpi(NULL, "LISTVIEW", dpi)`, with no window to hang an
//! application name on — all **six** `LISS_*` states render the *identical*
//! picture: a hollow one-pixel `#828790` rectangle over the ground, with no fill
//! and no selection colour anywhere. `LISS_SELECTED` and `LISS_SELECTEDNOTFOCUS`
//! are not merely close, they are byte-for-byte the same, which is the signature
//! of a part the class does not define; the modern item states live under the
//! *application* class `Explorer::ListView`, which this renderer cannot open and
//! which paints Win11's translucent light-blue band — also not the sheet.
//!
//! The sheet settles it: both list boxes on it show a **flat `#0078D7`** band,
//! and `#0078D7` is `COLOR_HIGHLIGHT` on the same machine — which the classic
//! fill already produces exactly. So the row keeps `SystemColors.Highlight`, and
//! [`tests::the_listview_row_part_carries_no_selection_colour`] pins the refusal
//! so a Windows update that starts defining those states fails a test instead of
//! leaving a better part unused.
//!
//! That also disposes of the grey unfocused band: a `ListView` greys its
//! selection when it loses focus, a **`ListBox` does not**, and the sheet proves
//! it — its second list box is not the focused control and its three bands are
//! the same `#0078D7` as the first one's. Focus is modelled all the same, on the
//! one control where it does change a pixel: see [`ComboBox::paint`].
//!
//! **`CBS_MIXED*` for an indeterminate row.** `CheckedListBox` shows an
//! `Indeterminate` item as a **greyed tick**, not as the theme's mixed glyph:
//! the sheet's `Delta` well is `#C3C3C3` carrying two light strokes at the exact
//! offsets the checked well carries its tick, whereas `CBS_MIXEDDISABLED` draws
//! one wide `#F0F0F0` bar. The part is `CBS_CHECKEDDISABLED` — which is also what
//! the toolkit asks for, since it converts `Indeterminate` to
//! `ButtonState.Checked | ButtonState.Inactive`.

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::control::{Control, ControlBase, ControlCanvas, ControlState};
use crate::enums::{BorderStyle, CheckState, FlatStyle, Size};
use crate::system::{edge_interior, Border3DSide, Border3DStyle, Visuals};
use crate::theme;
use crate::theme::part::EP_EDITTEXT;
use crate::theme::state::{ETS_DISABLED, ETS_NORMAL};
use crate::{Canvas, Rect};

// ── The themed parts this family adopts ───────────────────────────────────────
// Re-exported from the Windows SDK's own constants rather than written as
// numbers, for the reason `theme::part` gives: a part id is exactly the kind of
// magic number that is copied wrong once and then looks merely « a bit off »
// forever. They sit here rather than in `theme::part` because that table carries
// only the ids already adopted crate-wide and several families are measuring
// their own at once; they belong there once the wave has landed.
//
// Nothing is listed speculatively — every constant below is drawn by this file,
// and the ones that were measured and refused (`LVP_LISTITEM`, `CBS_MIXED*`) are
// deliberately absent, because a constant nobody draws is an invitation to draw
// it without measuring it.

/// `COMBOBOX` — the frame of an **editable** combo (`DropDown`, `Simple`).
const CP_BORDER: i32 = windows::Win32::UI::Controls::CP_BORDER.0;
/// `COMBOBOX` — the whole face of a **`DropDownList`**, which is a button, not a
/// field: the theme draws it with a push button's edges rather than a frame.
const CP_READONLY: i32 = windows::Win32::UI::Controls::CP_READONLY.0;
/// `COMBOBOX` — the drop-down button at the trailing edge. **A chevron and
/// nothing else**: sampled, the part paints no face and no bevel, which is why
/// the themed branch does not fill the button rectangle first.
const CP_DROPDOWNBUTTONRIGHT: i32 = windows::Win32::UI::Controls::CP_DROPDOWNBUTTONRIGHT.0;
/// `BUTTON` — the check well of a `CheckedListBox` row. The same part
/// `CheckBox` draws with, at the same 13 DIP size.
const CP_CHECKBOX: i32 = windows::Win32::UI::Controls::BP_CHECKBOX.0;

/// The state ids that go with the four parts above.
mod state {
    use windows::Win32::UI::Controls as sdk;

    /// `CP_BORDER` — at rest. A grey `#8D8D8D` frame; **not** what the sheet
    /// shows, which is why [`super::ComboBox::paint`] paints focused.
    pub const CBB_NORMAL: i32 = sdk::CBB_NORMAL.0;
    /// `CP_BORDER` — the pointer is over the combo.
    pub const CBB_HOT: i32 = sdk::CBB_HOT.0;
    /// `CP_BORDER` — the combo holds the focus: the accent frame `#0078D4`.
    pub const CBB_FOCUSED: i32 = sdk::CBB_FOCUSED.0;
    /// `CP_BORDER` — the combo is disabled.
    pub const CBB_DISABLED: i32 = sdk::CBB_DISABLED.0;

    /// `CP_READONLY` — at rest.
    pub const CBRO_NORMAL: i32 = sdk::CBRO_NORMAL.0;
    /// `CP_READONLY` — the pointer is over it.
    pub const CBRO_HOT: i32 = sdk::CBRO_HOT.0;
    /// `CP_READONLY` — the pointer is down on it (the list is dropping).
    pub const CBRO_PRESSED: i32 = sdk::CBRO_PRESSED.0;
    /// `CP_READONLY` — disabled.
    pub const CBRO_DISABLED: i32 = sdk::CBRO_DISABLED.0;

    /// `CP_DROPDOWNBUTTONRIGHT` — at rest.
    pub const CBXSR_NORMAL: i32 = sdk::CBXSR_NORMAL.0;
    /// `CP_DROPDOWNBUTTONRIGHT` — the pointer is over the combo.
    pub const CBXSR_HOT: i32 = sdk::CBXSR_HOT.0;
    /// `CP_DROPDOWNBUTTONRIGHT` — the pointer is down on it.
    pub const CBXSR_PRESSED: i32 = sdk::CBXSR_PRESSED.0;
    /// `CP_DROPDOWNBUTTONRIGHT` — disabled.
    pub const CBXSR_DISABLED: i32 = sdk::CBXSR_DISABLED.0;

    /// `BP_CHECKBOX` — an empty well: `#626262` frame over `#F3F3F3`.
    pub const CBS_UNCHECKEDNORMAL: i32 = sdk::CBS_UNCHECKEDNORMAL.0;
    /// `BP_CHECKBOX` — an empty well on a dead control.
    pub const CBS_UNCHECKEDDISABLED: i32 = sdk::CBS_UNCHECKEDDISABLED.0;
    /// `BP_CHECKBOX` — a ticked well: the accent fill `#005FB8`.
    pub const CBS_CHECKEDNORMAL: i32 = sdk::CBS_CHECKEDNORMAL.0;
    /// `BP_CHECKBOX` — a greyed tick, `#C3C3C3`. Drawn for a dead control **and**
    /// for an `Indeterminate` row — see the module docs on `CBS_MIXED*`.
    pub const CBS_CHECKEDDISABLED: i32 = sdk::CBS_CHECKEDDISABLED.0;
}

// ── Family enumerations ──────────────────────────────────────────────────────
// These live here, not in `enums.rs`, because only the list family uses them.
// Members and discriminants are the toolkit's own, so a round-trip is exact and
// nothing the designer offers is silently dropped.

/// How a `ComboBox` presents itself (`ComboBoxStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ComboBoxStyle {
    /// The list is always shown; a separate edit field sits above it.
    Simple = 0,
    /// An editable field with a drop-down list — the WinForms default.
    #[default]
    DropDown = 1,
    /// A non-editable field: the user may only pick an existing item.
    DropDownList = 2,
}

/// How a `ListBox` lets the user select (`SelectionMode`).
///
/// The distinction between `MultiSimple` and `MultiExtended` is purely an
/// INTERACTION rule (which modifier keys extend a selection), not a difference in
/// what can be stored — both hold an arbitrary set. It is honoured in
/// [`ListBox::click`], the one method that takes modifier state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionMode {
    /// No item can ever be selected; `SelectedIndex` stays `-1`.
    None = 0,
    /// At most one item — the default.
    #[default]
    One = 1,
    /// Any number; a plain click TOGGLES one item, leaving the rest.
    MultiSimple = 2,
    /// Any number; plain click replaces, `Ctrl` toggles, `Shift` extends a range.
    MultiExtended = 3,
}

/// Whether item drawing is owner-drawn (`DrawMode`). The port only paints
/// `Normal`; the two owner-draw modes are recorded but not honoured, because
/// there is no user draw callback in this surface — see the field docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DrawMode {
    #[default]
    Normal = 0,
    OwnerDrawFixed = 1,
    OwnerDrawVariable = 2,
}

/// The auto-complete behaviour of a `ComboBox` edit field (`AutoCompleteMode`).
/// Recorded but not honoured — auto-completion is an input-loop concern this
/// value-type control does not own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AutoCompleteMode {
    #[default]
    None = 0,
    Suggest = 1,
    Append = 2,
    SuggestAppend = 3,
}

/// Where auto-complete candidates come from (`AutoCompleteSource`). The
/// discriminants are the toolkit's flag values. Recorded but not honoured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AutoCompleteSource {
    FileSystem = 1,
    HistoryList = 2,
    RecentlyUsedList = 4,
    AllUrl = 6,
    AllSystemSources = 7,
    FileSystemDirectories = 0x20,
    CustomSource = 0x40,
    #[default]
    None = 0x80,
    ListItems = 0x100,
}

// ── Metrics ──────────────────────────────────────────────────────────────────
// DIP constants — and they are the TOOLKIT's numbers, not a design system's.
//
// This family used to reskin the WinForms surface onto Kubuno's own (rounded
// corners, the 20 DIP body line-height). That is reversed: a list here is a
// faithful `System.Windows.Forms` replica, and the Kubuno skin becomes a later
// layer on top of it. Every value below was read from the live toolkit on this
// machine with the default UI font (Segoe UI 9 pt = 12 DIP):
// `ListBox.ItemHeight` = 15, `CheckedListBox.ItemHeight` = 18,
// `ComboBox.PreferredHeight` = 23, `ComboBox.DefaultSize.Width` = 121.
//
// Everything is drawn in DIP; the `Canvas` applies `scale()` internally, so no
// metric here multiplies by the DPI itself. The two lengths that are genuinely
// the *system's* — the drop-down button width (`SM_CXVSCROLL`) and the one-pixel
// thickness of a bevel ring — never appear as constants at all: they are read
// per paint from `ControlCanvas::visuals()`.

/// One-pixel chrome border around a list/box, for MEASURING only. The paint
/// takes the real thickness from the rectangle `ControlCanvas::draw_edge`
/// returns, which is one *device* pixel per ring at any DPI.
const BORDER: f32 = 1.0;
/// The row height used when `ItemHeight` is left at its auto value — the
/// `ListBox.ItemHeight` the toolkit reports for the default UI font. See
/// [`auto_item_height`], which derives the same number from the font actually
/// installed; this constant is the fallback for the measuring path, which cannot
/// reach the system font (see [`widest_item`]).
const ITEM_HEIGHT_DEFAULT: f32 = 15.0;
/// A `CheckedListBox` row is taller than a `ListBox` one by this much — 18 vs 15
/// with the default font, the toolkit's own room for the check well.
const CHECK_ROW_EXTRA: f32 = 3.0;
/// Horizontal inset of item text from the list's client edge — the couple of
/// pixels the native list box leaves, not a design-system gutter.
const LIST_PAD_X: f32 = 2.0;
/// The check-box glyph square in a `CheckedListBox` row: the same 13 DIP well
/// `CheckBox` and `DateTimePicker.ShowCheckBox` draw (`DrawFrameControl`).
const CHECK_BOX_SIZE: f32 = 13.0;
/// The tick/bar inside that well, passed to `Canvas::vector_icon`.
const CHECK_MARK: f32 = 9.0;
/// Gap between a check box and its label.
const CHECK_GAP: f32 = 4.0;
/// The chevron in a drop-down button, passed to `Canvas::vector_icon`.
const DROPDOWN_GLYPH: f32 = 12.0;
/// Extra vertical chrome a single-line `ComboBox` adds around its row: 15 + 8 is
/// the 23 DIP `ComboBox.PreferredHeight` the toolkit reports.
const COMBO_CHROME_V: f32 = 8.0;
/// The minimum width WinForms gives a fresh `ComboBox` (`DefaultSize.Width`).
const COMBO_MIN_WIDTH: f32 = 121.0;

/// The row height in DIP for a stored `ItemHeight` (`0` means « auto »).
fn item_height_dip(item_height: i32) -> f32 {
    if item_height > 0 {
        item_height as f32
    } else {
        ITEM_HEIGHT_DEFAULT
    }
}

/// The auto row height for a UI font whose em size is `font_dip`.
///
/// `ListBox.ItemHeight` is the font's **GDI cell height** (`TEXTMETRIC.tmHeight`),
/// which the toolkit reports as **15** for the default Segoe UI 9 pt (12 DIP) —
/// the 1.25 ratio used here.
///
/// The ratio is not perfectly DPI-independent, because GDI rounds the face's
/// ascent and descent at the *device* resolution: the same toolkit reports 30 px
/// at 168 DPI, which is 17.1 DIP rather than 15. Reproducing that would need
/// `tmHeight` itself, and [`Visuals`] publishes the font's em size but neither its
/// cell height nor its line spacing — so the DIP-constant ratio is used and the
/// gap is stated rather than hidden.
fn auto_item_height(font_dip: f32) -> f32 {
    (font_dip * 1.25).round().max(1.0)
}

/// The row height at paint time, where the *real* UI font is reachable.
fn item_height_for(v: &Visuals, item_height: i32) -> f32 {
    if item_height > 0 {
        item_height as f32
    } else {
        auto_item_height(v.fonts.size_dip)
    }
}

/// The tallest client height not greater than `available` that shows only WHOLE
/// rows — WinForms' `IntegralHeight`. A pure function so the arithmetic is tested
/// without a window. `available` and the result are DIP.
pub fn integral_height(available: f32, item_h: f32, border: f32) -> f32 {
    if item_h <= 0.0 {
        return available;
    }
    let inner = (available - 2.0 * border).max(0.0);
    let rows = (inner / item_h).floor().max(0.0);
    rows * item_h + 2.0 * border
}

// ── ListControlBase — the abstract `ListControl` (+7) ─────────────────────────

/// The properties `System.Windows.Forms.ListControl` declares. Abstract in the
/// toolkit, so it is only ever a field of a concrete list, never built alone.
#[derive(Clone)]
pub struct ListControlBase {
    pub control: ControlBase,

    // ── Data binding (declared here, not yet honoured) ───────────────────
    /// `DataSource` — the bound list. The port has no boxed-object binding, so
    /// this is kept as an opaque marker: present so the property is not silently
    /// dropped, but items are supplied through each list's own collection.
    pub data_source: Option<String>,
    /// `DisplayMember` — which bound field to show. Stored, not yet honoured
    /// (there is nothing to bind against without a data source).
    pub display_member: String,
    /// `ValueMember` — which bound field is the value. Stored, not yet honoured.
    pub value_member: String,
    /// `SelectedValue` — the value of the selected row under binding. Stored,
    /// not yet honoured; use `SelectedIndex` instead.
    pub selected_value: Option<String>,
    /// `FormatString` — a .NET format string applied to each item. Stored, not
    /// yet honoured (items here are already strings).
    pub format_string: String,
    /// `FormattingEnabled` — whether `FormatString` is applied. Default `false`.
    pub formatting_enabled: bool,

    // ── Selection ────────────────────────────────────────────────────────
    /// `SelectedIndex`: `-1` when nothing is selected. The *authoritative* single
    /// index; the multi-selection set on `ListBox` keeps this in sync with its
    /// lowest member, the way the toolkit's `SelectedIndex` reports the anchor.
    pub selected_index: i32,
}

impl Default for ListControlBase {
    fn default() -> Self {
        Self {
            control: ControlBase::default(),
            data_source: None,
            display_member: String::new(),
            value_member: String::new(),
            selected_value: None,
            format_string: String::new(),
            formatting_enabled: false,
            selected_index: -1,
        }
    }
}

impl std::ops::Deref for ListControlBase {
    type Target = ControlBase;
    fn deref(&self) -> &ControlBase {
        &self.control
    }
}
impl std::ops::DerefMut for ListControlBase {
    fn deref_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }
}

// ── ComboBox (+22) ────────────────────────────────────────────────────────────

/// A drop-down or simple combo. Owns its own item collection
/// (`ComboBox.ObjectCollection`) and the drop-down metrics; the rest is inherited.
#[derive(Clone)]
pub struct ComboBox {
    pub base: ListControlBase,

    /// `Items` — the `ComboBox.ObjectCollection`, kept as strings (the port has
    /// no boxed items). Mutated through [`ComboBox::add_item`] so `Sorted` and
    /// the selection stay consistent.
    pub items: Vec<String>,

    /// `AutoCompleteCustomSource` — candidate strings for custom auto-complete.
    /// Stored; auto-complete itself is not honoured (see [`AutoCompleteMode`]).
    pub auto_complete_custom_source: Vec<String>,
    /// `AutoCompleteMode`. Stored, not honoured.
    pub auto_complete_mode: AutoCompleteMode,
    /// `AutoCompleteSource`. Stored, not honoured.
    pub auto_complete_source: AutoCompleteSource,
    /// `DrawMode`. Only `Normal` is painted; owner-draw is not honoured.
    pub draw_mode: DrawMode,
    /// `DropDownHeight` — the popup's pixel height. Default `106`.
    pub drop_down_height: i32,
    /// `DropDownStyle`. Default `DropDown`.
    pub drop_down_style: ComboBoxStyle,
    /// `DropDownWidth` — the popup's width; `0` means « match the control », the
    /// toolkit's default (its real default is the control width, computed lazily).
    pub drop_down_width: i32,
    /// `FlatStyle`. Default `Standard`. Honoured by the paint: `Standard` and
    /// `System` get the classic two-ring bevel, `Flat` and `Popup` a single flat
    /// ring (`Popup` lifts only under the pointer, which a resting paint never
    /// sees).
    pub flat_style: FlatStyle,
    /// `IntegralHeight` — for `Simple`, trim the list to whole rows. Default
    /// `true`. Honoured by [`integral_height`] at paint time.
    pub integral_height: bool,
    /// `ItemHeight` — row height in DIP; `0` means « derive from the font ».
    pub item_height: i32,
    /// `MaxDropDownItems` — how many rows the popup shows before scrolling.
    /// Default `8`.
    pub max_drop_down_items: i32,
    /// `MaxLength` — max characters in the edit field (`0` = unlimited). Stored;
    /// this control does not run an editor, so it only bounds a set `Text`.
    pub max_length: i32,
    /// `Sorted` — keep `Items` sorted. Default `false`.
    pub sorted: bool,
    /// `BackgroundImage` — an `Image` the port has no type for. Not honoured;
    /// present so the declared property is not dropped.
    pub background_image: Option<String>,
    // Re-declared overrides honoured through the inherited slots, NOT duplicated:
    //   BackColor, ForeColor            → ControlBase::{back_color, fore_color}
    //   BackgroundImageLayout           → ControlBase::background_image_layout
    //   MaximumSize, MinimumSize        → ControlBase::{maximum_size, minimum_size}
    //   Text                            → ControlBase::text
    //   DataSource                      → ListControlBase::data_source
}

impl Default for ComboBox {
    /// The catalogue defaults: `DropDownStyle = DropDown`, `DropDownHeight = 106`,
    /// `MaxDropDownItems = 8`, `IntegralHeight = true`, `Sorted = false`,
    /// `MaxLength = 0`, `DrawMode = Normal`, `FlatStyle = Standard`.
    fn default() -> Self {
        Self {
            base: ListControlBase::default(),
            items: Vec::new(),
            auto_complete_custom_source: Vec::new(),
            auto_complete_mode: AutoCompleteMode::default(),
            auto_complete_source: AutoCompleteSource::default(),
            draw_mode: DrawMode::default(),
            drop_down_height: 106,
            drop_down_style: ComboBoxStyle::default(),
            drop_down_width: 0,
            flat_style: FlatStyle::default(),
            integral_height: true,
            item_height: 0,
            max_drop_down_items: 8,
            max_length: 0,
            sorted: false,
            background_image: None,
        }
    }
}

impl std::ops::Deref for ComboBox {
    type Target = ListControlBase;
    fn deref(&self) -> &ListControlBase {
        &self.base
    }
}
impl std::ops::DerefMut for ComboBox {
    fn deref_mut(&mut self) -> &mut ListControlBase {
        &mut self.base
    }
}

impl ComboBox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// The selected item's text, or `None` when nothing is selected.
    pub fn selected_item(&self) -> Option<&str> {
        let i = self.base.selected_index;
        if i < 0 {
            None
        } else {
            self.items.get(i as usize).map(String::as_str)
        }
    }

    /// Adds an item, honouring `Sorted`, and returns the index it landed at. When
    /// sorting inserts ahead of the current selection, the selection follows its
    /// item so `SelectedIndex` never silently points at a different row.
    pub fn add_item(&mut self, text: impl Into<String>) -> usize {
        let text = text.into();
        let at = if self.sorted {
            self.items.partition_point(|existing| existing.as_str() < text.as_str())
        } else {
            self.items.len()
        };
        self.items.insert(at, text);
        if self.base.selected_index >= at as i32 {
            self.base.selected_index += 1;
        }
        at
    }

    /// Removes the item at `index`, shifting `SelectedIndex`: a lower item pulls
    /// the selection down, the selected item itself clears it to `-1`.
    pub fn remove_item(&mut self, index: usize) {
        if index >= self.items.len() {
            return;
        }
        self.items.remove(index);
        let sel = self.base.selected_index;
        if sel == index as i32 {
            self.base.selected_index = -1;
        } else if sel > index as i32 {
            self.base.selected_index = sel - 1;
        }
    }

    /// Sets `SelectedIndex`, clamped to `[-1, Items.Count-1]`. On a valid index
    /// the control's `Text` follows the item, as the toolkit updates it. Passing
    /// anything below `0` clears the selection.
    pub fn set_selected_index(&mut self, index: i32) {
        let max = self.items.len() as i32 - 1;
        let clamped = if index < 0 { -1 } else { index.min(max) };
        self.base.selected_index = clamped;
        if clamped >= 0 {
            self.base.control.text = self.items[clamped as usize].clone();
        }
    }

    /// The single-line box height for `DropDown`/`DropDownList`.
    fn line_height(&self) -> f32 {
        item_height_dip(self.item_height) + COMBO_CHROME_V
    }
}

impl Control for ComboBox {
    fn control(&self) -> &ControlBase {
        &self.base.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base.control
    }

    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        let ih = item_height_dip(self.item_height);
        let widest = widest_item(c, &self.items);
        // The edit area, the chevron (a square the size of the row) and padding.
        let width = (widest + ih + LIST_PAD_X * 2.0 + BORDER * 2.0).max(COMBO_MIN_WIDTH);
        let height = match self.drop_down_style {
            // Simple shows the edit row AND a list; give room for a few rows.
            ComboBoxStyle::Simple => {
                let rows = self.items.len().clamp(3, self.max_drop_down_items.max(1) as usize);
                self.line_height() + rows as f32 * ih + BORDER * 2.0
            }
            _ => self.line_height(),
        };
        Size::new(width, height)
    }

    /// Paints the combo as a resting, **focused** control.
    ///
    /// A combo is the one control in this family whose chrome changes with the
    /// focus: `CP_BORDER` frames it in the accent `#0078D4` when it has it and in
    /// `#8D8D8D` when it does not, and an editable combo shows its text selected
    /// only while it holds it. A `paint` with no state has no way to know, so it
    /// paints the focused look — which is both the reference sheet's (every
    /// editable combo on `03-listcontrol.png` carries the accent frame) and the
    /// convention `views.rs` already set for `ListView`/`TreeView`.
    ///
    /// A host that *does* know calls [`Control::paint_with_state`] and gets the
    /// grey frame, the pressed face and the hot chevron for free.
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        self.paint_with_state(c, bounds, ControlState { focused: true, ..ControlState::default() });
    }

    /// The real painter — see [`paint_combo`].
    fn paint_with_state(&self, c: &dyn ControlCanvas, bounds: Rect, state: ControlState) {
        paint_combo(self, c, bounds, state);
    }

    fn type_name(&self) -> &'static str {
        "ComboBox"
    }
}

// ── ListBox (+21) ─────────────────────────────────────────────────────────────

/// A scrolling list. Owns its items, its selection set and its scroll position;
/// `CheckedListBox` composes it.
#[derive(Clone)]
pub struct ListBox {
    pub base: ListControlBase,

    /// `Items` — the `ListBox.ObjectCollection`. Mutate via [`ListBox::add_item`]
    /// / [`ListBox::remove_item`] so selection and scroll stay consistent.
    pub items: Vec<String>,

    /// `SelectionMode`. Default `One`. Assign through [`ListBox::set_selection_mode`]
    /// so an existing multi-selection is trimmed when narrowing to `One`/`None`.
    pub selection_mode: SelectionMode,
    /// `BorderStyle`. Default `Fixed3D`.
    pub border_style: BorderStyle,
    /// `ColumnWidth` — width of a column in multi-column mode (`0` = auto).
    pub column_width: i32,
    /// `CustomTabOffsets` — tab-stop positions. Stored; honoured only with
    /// `UseCustomTabOffsets`, and tab expansion is not painted here.
    pub custom_tab_offsets: Vec<i32>,
    /// `DrawMode`. Only `Normal` is painted.
    pub draw_mode: DrawMode,
    /// `HorizontalExtent` — the scrollable width when `HorizontalScrollbar` is on.
    pub horizontal_extent: i32,
    /// `HorizontalScrollbar` — show a horizontal scrollbar. Default `false`.
    pub horizontal_scrollbar: bool,
    /// `IntegralHeight` — resize height to whole rows. Default `true`.
    pub integral_height: bool,
    /// `ItemHeight` — row height in DIP; `0` means « derive from the font ».
    pub item_height: i32,
    /// `MultiColumn` — lay items out in columns. Default `false`. Stored; the
    /// single-column layout is what is painted.
    pub multi_column: bool,
    /// `ScrollAlwaysVisible` — keep the scrollbar shown even when it fits.
    pub scroll_always_visible: bool,
    /// `Sorted` — keep `Items` sorted. Default `false`.
    pub sorted: bool,
    /// `UseCustomTabOffsets`. Default `false`.
    pub use_custom_tab_offsets: bool,
    /// `UseTabStops` — expand tab characters. Default `true`.
    pub use_tab_stops: bool,
    /// `BackgroundImage` — no `Image` type in the port; not honoured.
    pub background_image: Option<String>,
    // Re-declared overrides honoured through the inherited slots, NOT duplicated:
    //   BackColor, ForeColor  → ControlBase::{back_color, fore_color}
    //   BackgroundImageLayout → ControlBase::background_image_layout
    //   Font                  → ControlBase::font
    //   Text                  → ControlBase::text

    // ── Selection & scroll state (runtime, not designer properties) ──────
    /// `SelectedIndices`, kept sorted-ascending and unique — the toolkit's own
    /// invariant. `base.selected_index` mirrors its first member.
    selected: Vec<usize>,
    /// The `MultiExtended` range anchor. Pure interaction state, not a property.
    anchor: Option<usize>,
    /// `TopIndex` — the first row scrolled into view.
    pub top_index: usize,
}

impl Default for ListBox {
    /// The catalogue defaults: `SelectionMode = One`, `BorderStyle = Fixed3D`,
    /// `IntegralHeight = true`, `UseTabStops = true`; everything else `false`/`0`.
    fn default() -> Self {
        Self {
            base: ListControlBase::default(),
            items: Vec::new(),
            selection_mode: SelectionMode::default(),
            border_style: BorderStyle::default(),
            column_width: 0,
            custom_tab_offsets: Vec::new(),
            draw_mode: DrawMode::default(),
            horizontal_extent: 0,
            horizontal_scrollbar: false,
            integral_height: true,
            item_height: 0,
            multi_column: false,
            scroll_always_visible: false,
            sorted: false,
            use_custom_tab_offsets: false,
            use_tab_stops: true,
            background_image: None,
            selected: Vec::new(),
            anchor: None,
            top_index: 0,
        }
    }
}

impl std::ops::Deref for ListBox {
    type Target = ListControlBase;
    fn deref(&self) -> &ListControlBase {
        &self.base
    }
}
impl std::ops::DerefMut for ListBox {
    fn deref_mut(&mut self) -> &mut ListControlBase {
        &mut self.base
    }
}

impl ListBox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// `SelectedIndices` — sorted, unique, all in range.
    pub fn selected_indices(&self) -> &[usize] {
        &self.selected
    }

    pub fn is_selected(&self, index: usize) -> bool {
        self.selected.binary_search(&index).is_ok()
    }

    /// `SelectedIndex`: the anchor of the selection, `-1` when empty.
    pub fn selected_index(&self) -> i32 {
        self.base.selected_index
    }

    /// Restores the invariants after any mutation: sorted-unique, in range, the
    /// mirror index refreshed, the scroll position clamped.
    fn resync(&mut self) {
        let len = self.items.len();
        self.selected.retain(|&i| i < len);
        self.selected.sort_unstable();
        self.selected.dedup();
        self.base.selected_index = self.selected.first().map(|&i| i as i32).unwrap_or(-1);
        if len == 0 {
            self.top_index = 0;
        } else if self.top_index >= len {
            self.top_index = len - 1;
        }
    }

    /// Narrows or widens `SelectionMode`, trimming an existing multi-selection to
    /// its anchor when moving to `One`, and clearing it for `None` — the toolkit
    /// drops the extra selection rather than keeping it hidden.
    pub fn set_selection_mode(&mut self, mode: SelectionMode) {
        self.selection_mode = mode;
        match mode {
            SelectionMode::None => self.selected.clear(),
            SelectionMode::One => {
                if let Some(&first) = self.selected.first() {
                    self.selected = vec![first];
                }
            }
            _ => {}
        }
        self.resync();
    }

    /// `SetSelected(index, value)` — programmatic selection, obeying the mode
    /// (`None` refuses; `One` replaces on select).
    pub fn set_selected(&mut self, index: usize, value: bool) {
        if index >= self.items.len() {
            return;
        }
        match self.selection_mode {
            SelectionMode::None => {}
            SelectionMode::One => {
                if value {
                    self.selected = vec![index];
                } else {
                    self.selected.retain(|&i| i != index);
                }
            }
            SelectionMode::MultiSimple | SelectionMode::MultiExtended => {
                if value {
                    if !self.selected.contains(&index) {
                        self.selected.push(index);
                    }
                } else {
                    self.selected.retain(|&i| i != index);
                }
            }
        }
        self.anchor = if value { Some(index) } else { self.anchor };
        self.resync();
    }

    /// Sets `SelectedIndex` directly (`-1` clears). Follows the mode: under a
    /// multi-mode it replaces the whole selection, matching the property setter.
    pub fn set_selected_index(&mut self, index: i32) {
        if index < 0 || self.selection_mode == SelectionMode::None {
            self.selected.clear();
            self.anchor = None;
        } else if (index as usize) < self.items.len() {
            self.selected = vec![index as usize];
            self.anchor = Some(index as usize);
        }
        self.resync();
    }

    pub fn clear_selected(&mut self) {
        self.selected.clear();
        self.anchor = None;
        self.resync();
    }

    /// A user click on `index` with the `Ctrl`/`Shift` modifiers, resolved per
    /// `SelectionMode`. This is where `MultiSimple` and `MultiExtended` diverge:
    ///
    /// * `None` — ignored.
    /// * `One` — always the single clicked item.
    /// * `MultiSimple` — a plain click TOGGLES that item; modifiers are ignored,
    ///   because in simple mode every click already toggles.
    /// * `MultiExtended` — `Shift` extends a range from the anchor (adding to the
    ///   set when `Ctrl` is also held, else replacing it); `Ctrl` alone toggles
    ///   the one item; a bare click replaces the selection.
    pub fn click(&mut self, index: usize, ctrl: bool, shift: bool) {
        if index >= self.items.len() {
            return;
        }
        match self.selection_mode {
            SelectionMode::None => {}
            SelectionMode::One => {
                self.selected = vec![index];
                self.anchor = Some(index);
            }
            SelectionMode::MultiSimple => {
                self.toggle(index);
                self.anchor = Some(index);
            }
            SelectionMode::MultiExtended => {
                if shift {
                    let a = self.anchor.unwrap_or(index);
                    let (lo, hi) = (a.min(index), a.max(index));
                    if !ctrl {
                        self.selected.clear();
                    }
                    for k in lo..=hi {
                        if !self.selected.contains(&k) {
                            self.selected.push(k);
                        }
                    }
                    // The anchor stays put so a further Shift extends from it.
                } else if ctrl {
                    self.toggle(index);
                    self.anchor = Some(index);
                } else {
                    self.selected = vec![index];
                    self.anchor = Some(index);
                }
            }
        }
        self.resync();
    }

    fn toggle(&mut self, index: usize) {
        if let Some(pos) = self.selected.iter().position(|&i| i == index) {
            self.selected.remove(pos);
        } else {
            self.selected.push(index);
        }
    }

    /// `TopIndex` setter, clamped so at least the last row can reach the top.
    pub fn set_top_index(&mut self, index: usize) {
        self.top_index = if self.items.is_empty() {
            0
        } else {
            index.min(self.items.len() - 1)
        };
    }

    /// Adds an item (honouring `Sorted`) and returns its landing index. Selection
    /// indices at or after the insertion point shift up so they keep pointing at
    /// their own items.
    pub fn add_item(&mut self, text: impl Into<String>) -> usize {
        let text = text.into();
        let at = if self.sorted {
            self.items.partition_point(|existing| existing.as_str() < text.as_str())
        } else {
            self.items.len()
        };
        self.items.insert(at, text);
        for i in self.selected.iter_mut() {
            if *i >= at {
                *i += 1;
            }
        }
        if let Some(a) = self.anchor {
            if a >= at {
                self.anchor = Some(a + 1);
            }
        }
        self.resync();
        at
    }

    /// Removes the item at `index`, dropping it from the selection and shifting
    /// every higher index down by one.
    pub fn remove_item(&mut self, index: usize) {
        if index >= self.items.len() {
            return;
        }
        self.items.remove(index);
        self.selected.retain(|&i| i != index);
        for i in self.selected.iter_mut() {
            if *i > index {
                *i -= 1;
            }
        }
        self.anchor = match self.anchor {
            Some(a) if a == index => None,
            Some(a) if a > index => Some(a - 1),
            other => other,
        };
        self.resync();
    }
}

impl Control for ListBox {
    fn control(&self) -> &ControlBase {
        &self.base.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base.control
    }

    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        list_preferred_size(
            c,
            &self.items,
            item_height_dip(self.item_height),
            self.integral_height,
            self.base.control.height(),
            0.0,
        )
    }

    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        paint_list(
            c,
            bounds,
            &ListPaint {
                base:      &self.base.control,
                items:     &self.items,
                item_h:    item_height_for(c.visuals(), self.item_height),
                top_index: self.top_index,
                border:    self.border_style,
                checkbox:  None,
                selected:  &|i| self.is_selected(i),
            },
        );
    }

    fn type_name(&self) -> &'static str {
        "ListBox"
    }
}

// ── CheckedListBox (+7) ────────────────────────────────────────────────────────

/// A `ListBox` whose rows carry a check box. It COMPOSES a `ListBox` (its .NET
/// base), adding only the per-item `CheckState` and the check-related properties.
///
/// Every field's default is the type's own (`ListBox::default()`, `false`, an
/// empty `Vec`), so `Default` is derived — the check-related properties are all
/// `false` in the catalogue, matching the derived zero.
#[derive(Clone, Default)]
pub struct CheckedListBox {
    pub list: ListBox,

    /// `CheckOnClick` — toggle the check on a single click rather than on the
    /// second click of a selected row. Default `false`.
    pub check_on_click: bool,
    /// `ThreeDCheckBoxes` — draw sunken (3-D) rather than flat boxes. Default
    /// `false`. Recorded; the themed flat box is what is painted.
    pub three_d_check_boxes: bool,
    /// `UseCompatibleTextRendering` — GDI vs GDI+ text metrics. Default `false`.
    /// Irrelevant to the DirectWrite paint; recorded for completeness.
    pub use_compatible_text_rendering: bool,
    // Re-declared overrides honoured through the composed `ListBox`, NOT
    // duplicated, each with a narrower contract the toolkit enforces:
    //   DrawMode      → list.draw_mode, but only `Normal` is valid here
    //   ItemHeight    → list.item_height (the toolkit forces a uniform height)
    //   Items         → list.items, paired 1:1 with `check_states`
    //   SelectionMode → list.selection_mode, restricted to `One`/`None`

    /// Per-item `CheckState`, kept parallel to `list.items`. Not a designer
    /// property (the toolkit exposes it through `GetItemCheckState`), so it is
    /// private and maintained only through this type's methods.
    check_states: Vec<CheckState>,
}

impl std::ops::Deref for CheckedListBox {
    type Target = ListBox;
    fn deref(&self) -> &ListBox {
        &self.list
    }
}
impl std::ops::DerefMut for CheckedListBox {
    fn deref_mut(&mut self) -> &mut ListBox {
        &mut self.list
    }
}

impl CheckedListBox {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an item with an initial check state, keeping `check_states` aligned
    /// with the (possibly sorted) insertion the `ListBox` performs.
    pub fn add_item(&mut self, text: impl Into<String>, state: CheckState) -> usize {
        let at = self.list.add_item(text);
        self.check_states.insert(at, state);
        at
    }

    pub fn remove_item(&mut self, index: usize) {
        if index >= self.check_states.len() {
            return;
        }
        self.list.remove_item(index);
        self.check_states.remove(index);
    }

    pub fn get_item_check_state(&self, index: usize) -> CheckState {
        self.check_states.get(index).copied().unwrap_or_default()
    }

    /// `SetItemCheckState` — the only path to `Indeterminate`, which a click
    /// never produces.
    pub fn set_item_check_state(&mut self, index: usize, state: CheckState) {
        if let Some(slot) = self.check_states.get_mut(index) {
            *slot = state;
        }
    }

    /// `GetItemChecked` — true when the row is Checked OR Indeterminate, matching
    /// `CheckedIndices`/`CheckedItems`, which both include the indeterminate rows.
    pub fn get_item_checked(&self, index: usize) -> bool {
        self.get_item_check_state(index) != CheckState::Unchecked
    }

    /// `SetItemChecked(index, true|false)` — sets Checked or Unchecked (never
    /// Indeterminate; use [`CheckedListBox::set_item_check_state`] for that).
    pub fn set_item_checked(&mut self, index: usize, checked: bool) {
        self.set_item_check_state(
            index,
            if checked { CheckState::Checked } else { CheckState::Unchecked },
        );
    }

    /// `CheckedIndices` — the rows that are Checked or Indeterminate, ascending.
    pub fn checked_indices(&self) -> Vec<usize> {
        (0..self.check_states.len()).filter(|&i| self.get_item_checked(i)).collect()
    }

    /// The click-time state cycle WinForms actually uses: any non-`Unchecked`
    /// state collapses to `Unchecked`, and `Unchecked` becomes `Checked`. So a
    /// click on an `Indeterminate` row goes to `Unchecked`, NOT to `Checked` —
    /// the trap that catches ports which assume a three-way cycle.
    pub fn toggle_check(&mut self, index: usize) {
        if let Some(slot) = self.check_states.get_mut(index) {
            *slot = if *slot == CheckState::Unchecked {
                CheckState::Checked
            } else {
                CheckState::Unchecked
            };
        }
    }

    /// A user click: it always moves the selection (via the composed `ListBox`),
    /// and additionally toggles the check when `CheckOnClick` is set. Selection in
    /// a `CheckedListBox` is single, so modifiers do not extend it.
    pub fn click(&mut self, index: usize) {
        if index >= self.list.items.len() {
            return;
        }
        self.list.click(index, false, false);
        if self.check_on_click {
            self.toggle_check(index);
        }
    }

    /// `SelectionMode` on a `CheckedListBox` accepts only `One` or `None`; the
    /// toolkit throws on the multi-modes. Anything else is coerced to `One`.
    pub fn set_selection_mode(&mut self, mode: SelectionMode) {
        let allowed = match mode {
            SelectionMode::None => SelectionMode::None,
            _ => SelectionMode::One,
        };
        self.list.set_selection_mode(allowed);
    }
}

impl Control for CheckedListBox {
    fn control(&self) -> &ControlBase {
        &self.list.base.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.list.base.control
    }

    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        // Same as a ListBox, plus room for the check box in each row.
        list_preferred_size(
            c,
            &self.list.items,
            checked_row_height(self.list.item_height, ITEM_HEIGHT_DEFAULT),
            self.list.integral_height,
            self.list.base.control.height(),
            CHECK_BOX_SIZE + CHECK_GAP,
        )
    }

    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        let auto = auto_item_height(c.visuals().fonts.size_dip);
        paint_list(
            c,
            bounds,
            &ListPaint {
                base:      &self.list.base.control,
                items:     &self.list.items,
                item_h:    checked_row_height(self.list.item_height, auto),
                top_index: self.list.top_index,
                border:    self.list.border_style,
                checkbox:  Some(&|i| self.get_item_check_state(i)),
                selected:  &|i| self.list.is_selected(i),
            },
        );
    }

    fn type_name(&self) -> &'static str {
        "CheckedListBox"
    }
}

// ── Painting (the system's own surface) ───────────────────────────────────────
// Every colour is a `SystemColors` entry read from `GetSysColor`, every length
// either a DIP constant above or a `SystemMetrics` entry, and every string is
// drawn in `Visuals::fonts::message` — the real UI font. Nothing here reaches
// for `Canvas::theme()` or the shared Kubuno text formats, and nothing rounds a
// corner: a WinForms list has none.

/// A `CheckedListBox` row height: the list row plus the toolkit's own extra room
/// for the check well (18 vs 15 with the default font). An explicit `ItemHeight`
/// is taken as given — the toolkit lets it override.
fn checked_row_height(item_height: i32, auto_row: f32) -> f32 {
    if item_height > 0 {
        item_height as f32
    } else {
        auto_row + CHECK_ROW_EXTRA
    }
}

/// The widest item's measured text width (0 for an empty list).
///
/// **Measurement only, and in the wrong face on purpose.** A list *paints* in the
/// system UI font, which is what the toolkit measures against — but
/// `Control::preferred_size` receives a bare [`Canvas`], not a [`ControlCanvas`],
/// so `Visuals::fonts` is out of reach there and the shared Kubuno formats are
/// the only ones available. Widening that signature is out of scope for a
/// repaint, so the gap is stated rather than hidden: this is the one place in the
/// family that still touches `Canvas::formats()`. Every list on the reference
/// sheet is given an explicit `Width`, so the difference does not reach the
/// comparison.
fn widest_item(c: &dyn Canvas, items: &[String]) -> f32 {
    let body = &c.formats().body;
    items.iter().fold(0.0_f32, |w, it| w.max(c.measure(it, body)))
}

/// The content-driven size shared by `ListBox` and `CheckedListBox`. `ih` is the
/// resolved row height and `extra_w` the per-row width a check box adds.
fn list_preferred_size(
    c: &dyn Canvas,
    items: &[String],
    ih: f32,
    integral: bool,
    current_height: f32,
    extra_w: f32,
) -> Size {
    let width = widest_item(c, items) + extra_w + LIST_PAD_X * 2.0 + BORDER * 2.0;
    let rows = items.len().max(1) as f32;
    let content_h = rows * ih + BORDER * 2.0;
    let height = if integral {
        integral_height(content_h.max(current_height), ih, BORDER)
    } else {
        content_h
    };
    Size::new(width.max(1.0), height)
}

/// Everything one list body needs to paint itself.
///
/// A struct rather than eight parameters: `ListBox`, `CheckedListBox` and the
/// always-open list of a `Simple` combo all render through the same function, and
/// widening its signature once per caller is how three lists start to drift apart.
struct ListPaint<'a> {
    /// The inherited block, for the ambient `BackColor`/`ForeColor` resolution.
    base:      &'a ControlBase,
    items:     &'a [String],
    /// Already resolved — the caller knows whether it is a `ListBox` row or the
    /// taller `CheckedListBox` one.
    item_h:    f32,
    top_index: usize,
    border:    BorderStyle,
    /// `Some` only for a `CheckedListBox`: the `CheckState` of each row.
    checkbox:  Option<&'a dyn Fn(usize) -> CheckState>,
    selected:  &'a dyn Fn(usize) -> bool,
}

/// An explicit colour if set, else the system one. In WinForms an unset
/// `BackColor`/`ForeColor` means « take the ambient default », never a literal
/// transparent — and for a list that default is the WINDOW pair, not the control
/// face: a list is a field, like a text box.
fn or_system(opt: Option<D2D1_COLOR_F>, fallback: D2D1_COLOR_F) -> D2D1_COLOR_F {
    opt.unwrap_or(fallback)
}

/// The 3-D edge a list or an edit field is framed with, for this `FlatStyle`.
/// `Flat` and `Popup` drop the bevel to a single `ControlDark` ring; `Popup` only
/// lifts under the pointer, which a resting paint never sees.
fn field_edge(flat_style: FlatStyle, sunken: Border3DStyle) -> Border3DStyle {
    match flat_style {
        FlatStyle::Flat | FlatStyle::Popup => Border3DStyle::Flat,
        FlatStyle::Standard | FlatStyle::System => sunken,
    }
}

/// Frames `bounds` per `BorderStyle` and returns the client rectangle inside it.
///
/// `Fixed3D` is the one style with two correct renderings — see
/// [`fixed3d_frame`]. `FixedSingle` is the flat `WindowFrame` ring, and `None`
/// frames nothing at all; neither is themed, in the toolkit or here.
fn frame(
    c: &dyn ControlCanvas,
    bounds: &Rect,
    border: BorderStyle,
    enabled: bool,
    ground: &D2D1_COLOR_F,
) -> Rect {
    match border {
        BorderStyle::None => *bounds,
        BorderStyle::FixedSingle => {
            c.stroke_rect(bounds, &c.visuals().colors.window_frame);
            edge_interior(bounds, 1, c.scale())
        }
        BorderStyle::Fixed3D => fixed3d_frame(c, bounds, enabled, ground),
    }
}

/// `BorderStyle::Fixed3D` — the themed `EDIT` frame, falling back to the classic
/// `DrawEdge` well. Returns the **interior**, as `draw_edge` does.
///
/// ## Two renderings, both correct
///
/// With visual styles ON — how the reference sheet was captured — a `Fixed3D`
/// list is framed by one flat theme line, `#ABADB3` on a default Windows 11,
/// with the frame's inner pixel in the list's own ground; there is no bevel at
/// all. With them OFF it is the classic two-ring sunken well built from
/// `COLOR_3DDKSHADOW` and friends. Neither approximates the other, and no
/// `GetSysColor` index can produce the first.
///
/// The part is `EP_EDITTEXT`, the same one [`crate::text`] measured: on Windows
/// 11 `EP_EDITBORDER_NOSCROLL` renders the modern rounded frame, and only
/// `EP_EDITTEXT` renders the toolkit's.
///
/// ## Why two rings come back, and the pad is repainted
///
/// `EP_EDITTEXT` paints a frame AND a fill, and its fill under `ETS_DISABLED` is
/// the theme's own tint — whereas the toolkit shows the list's `BackColor` there,
/// because its client area covers everything but the frame. So the inner ring is
/// put back explicitly in the list's ground.
///
/// The interior is then reported **two** device pixels in, not one, and that is
/// measured rather than tidy: on the sheet the list frame sits at x=362, the
/// first `#FFFFFF` pad pixel at 363, and the selection band runs 364…482 — one
/// pixel of ground on each side that the rows never cover. Reporting one ring
/// would push every band a pixel wide at both ends. It is also exactly what the
/// classic `draw_edge(Sunken)` returns, so the two branches lay their rows out
/// identically.
fn fixed3d_frame(
    c: &dyn ControlCanvas,
    bounds: &Rect,
    enabled: bool,
    ground: &D2D1_COLOR_F,
) -> Rect {
    let state = if enabled { ETS_NORMAL } else { ETS_DISABLED };
    if c.draw_theme_part(theme::class::EDIT, EP_EDITTEXT, state, *bounds, *ground) {
        let pad = edge_interior(bounds, 1, c.scale());
        c.fill_rect(&pad, ground);
        return edge_interior(bounds, 2, c.scale());
    }
    c.draw_edge(bounds, Border3DStyle::Sunken, Border3DSide::ALL)
}

/// Draws a list body: the window ground, the border, then each visible row from
/// `top_index`, with an optional check well and the system selection band.
fn paint_list(c: &dyn ControlCanvas, bounds: Rect, p: &ListPaint) {
    let visuals = c.visuals();
    let colors = &visuals.colors;

    // A list is a FIELD: its ground is `Window` and its ink `WindowText`, not the
    // control face. The bevel is painted over the fill, so the fill goes first.
    let back = or_system(p.base.back_color, colors.window);
    let fore = if p.base.enabled {
        or_system(p.base.fore_color, colors.window_text)
    } else {
        colors.gray_text
    };
    c.fill_rect(&bounds, &back);
    let inner = frame(c, &bounds, p.border, p.base.enabled, &back);
    // The themed `EDIT` frame brings its own fill — the theme's white, or its
    // disabled tint — so the client area is put back in the list's own ground
    // over it. On a default list the two are the same `Window` white and this
    // changes no pixel; on a list with a `BackColor`, or a disabled one, it is
    // the difference between the toolkit's ground and the theme's.
    c.fill_rect(&inner, &back);
    c.push_clip(&inner);

    let mut y = inner.top;
    for (i, item) in p.items.iter().enumerate().skip(p.top_index) {
        if y >= inner.bottom {
            break;
        }
        let row = Rect::new(inner.left, y, inner.right, (y + p.item_h).min(inner.bottom));
        // A selected row is the system `Highlight` band edge to edge — the real
        // system blue, and the text on it turns `HighlightText`.
        //
        // NOT `LISTVIEW`/`LVP_LISTITEM`, which is the obvious part and was
        // measured to carry no selection colour at all; and NOT greyed when the
        // list loses focus, which is a `ListView` behaviour a `ListBox` does not
        // share. Both findings are the module docs', and both are pinned by a
        // test.
        let selected = (p.selected)(i);
        if selected {
            c.fill_rect(&row, &colors.highlight);
        }

        let mut text_left = row.left + LIST_PAD_X;
        if let Some(state_of) = p.checkbox {
            let box_rect = Rect::new(
                row.left + LIST_PAD_X,
                row.top + (p.item_h - CHECK_BOX_SIZE) / 2.0,
                row.left + LIST_PAD_X + CHECK_BOX_SIZE,
                row.top + (p.item_h + CHECK_BOX_SIZE) / 2.0,
            );
            // The well is composited onto whatever the row already put down —
            // the band when the row is selected, the list's ground otherwise —
            // because the themed part does not cover its own rectangle.
            let under = if selected { colors.highlight } else { back };
            paint_check_box(c, &box_rect, state_of(i), p.base.enabled, &under);
            text_left = box_rect.right + CHECK_GAP;
        }

        let label = Rect::new(text_left, row.top, row.right - LIST_PAD_X, row.bottom);
        let colour = if selected { colors.highlight_text } else { fore };
        c.text_ellipsis(item, &label, &visuals.fonts.message, &colour);
        y += p.item_h;
    }

    c.pop_clip();
}

/// The `BP_CHECKBOX` state a row's well is drawn in.
///
/// A pure function so the mapping — which is where the toolkit's one surprise
/// lives — is asserted without a canvas.
///
/// `Indeterminate` maps to `CBS_CHECKEDDISABLED`, **not** to any `CBS_MIXED*`.
/// That is measured off the sheet (the `Delta` well carries the tick's two
/// strokes in grey, where the mixed glyph is one wide bar) and it is what
/// `CheckedListBox` asks Windows for: it converts an indeterminate item to
/// `ButtonState.Checked | ButtonState.Inactive`, i.e. a ticked well on a dead
/// control. A row on a disabled list lands on the same state, which is the same
/// thing the toolkit does.
fn check_glyph_state(check: CheckState, enabled: bool) -> i32 {
    match (check, enabled) {
        (CheckState::Unchecked, true) => state::CBS_UNCHECKEDNORMAL,
        (CheckState::Unchecked, false) => state::CBS_UNCHECKEDDISABLED,
        (CheckState::Checked, true) => state::CBS_CHECKEDNORMAL,
        (CheckState::Checked, false) => state::CBS_CHECKEDDISABLED,
        (CheckState::Indeterminate, _) => state::CBS_CHECKEDDISABLED,
    }
}

/// Draws one check well in the three `CheckState`s, themed then classic.
///
/// With visual styles ON the well is the real `BUTTON`/`BP_CHECKBOX` part — a
/// flat `#626262` square over `#F3F3F3` when empty, the solid accent `#005FB8`
/// with a white tick when checked, and the same tick in `#C3C3C3` grey when
/// indeterminate. `ground` is what the row has already painted underneath,
/// because the part does not cover its own rectangle: it rounds its corners and
/// blends its edges, so a well on a selected row must be told it is sitting on
/// the `Highlight` band or it will fringe against the wrong colour.
///
/// With them OFF it is the toolkit's own `DrawFrameControl(DFCS_BUTTONCHECK)`: a
/// SUNKEN two-ring bevel around a `Window`-coloured field, so it reads as a hole
/// in the row rather than as a stroked square, with `Indeterminate` swapping that
/// field for the control face and the tick for the greyed bar. The mark is a
/// vector geometry, never a « ✓ » character, so it keeps its shape and weight at
/// every DPI.
fn paint_check_box(
    c: &dyn ControlCanvas,
    rect: &Rect,
    state: CheckState,
    enabled: bool,
    ground: &D2D1_COLOR_F,
) {
    if c.draw_theme_part(
        theme::class::BUTTON,
        CP_CHECKBOX,
        check_glyph_state(state, enabled),
        *rect,
        *ground,
    ) {
        return;
    }

    let colors = c.visuals().colors;
    let interior = c.draw_edge(rect, Border3DStyle::Sunken, Border3DSide::ALL);
    let field = if state == CheckState::Indeterminate { colors.control } else { colors.window };
    c.fill_rect(&interior, &field);

    let (name, mark) = match state {
        CheckState::Unchecked => return,
        CheckState::Indeterminate => ("Minus", colors.control_dark),
        CheckState::Checked => ("Check", colors.window_text),
    };
    c.vector_icon(name, &interior, CHECK_MARK, &mark);
}

/// The `CP_BORDER` state an editable combo's frame is drawn in. Focus outranks
/// hot: a focused combo under the pointer keeps its accent frame.
fn combo_border_state(st: ControlState, enabled: bool) -> i32 {
    if !enabled {
        state::CBB_DISABLED
    } else if st.focused {
        state::CBB_FOCUSED
    } else if st.hot {
        state::CBB_HOT
    } else {
        state::CBB_NORMAL
    }
}

/// The `CP_READONLY` state a `DropDownList`'s face is drawn in.
///
/// There is deliberately no focused case: the part has none — the theme defines
/// `CBRO_NORMAL`/`HOT`/`PRESSED`/`DISABLED` and nothing else — and the sheet
/// agrees, showing the same `#FDFDFD` face on a `DropDownList` as a resting one.
/// `pressed` is the list dropping, which is the only thing that darkens it.
fn combo_readonly_state(st: ControlState, enabled: bool) -> i32 {
    if !enabled {
        state::CBRO_DISABLED
    } else if st.pressed {
        state::CBRO_PRESSED
    } else if st.hot {
        state::CBRO_HOT
    } else {
        state::CBRO_NORMAL
    }
}

/// The `CP_DROPDOWNBUTTONRIGHT` state the chevron is drawn in.
fn combo_button_state(st: ControlState, enabled: bool) -> i32 {
    if !enabled {
        state::CBXSR_DISABLED
    } else if st.pressed {
        state::CBXSR_PRESSED
    } else if st.hot {
        state::CBXSR_HOT
    } else {
        state::CBXSR_NORMAL
    }
}

/// Draws a combo in each of its three styles.
///
/// The three genuinely differ in the toolkit, and the difference is the whole
/// point of the property: `DropDown` and `Simple` carry an **editable field** —
/// framed like a text box — while `DropDownList` is a **button face**, because
/// there is nothing to type into. The drop-down button is one vertical scroll bar
/// wide (`SM_CXVSCROLL`, the metric the toolkit sizes it with); `Simple` has no
/// button at all, only the always-open list.
///
/// ## Themed, then classic
///
/// With visual styles ON the three pieces of chrome are the real `COMBOBOX`
/// parts: `CP_BORDER` for the editable frame, `CP_READONLY` for the whole
/// `DropDownList` face, and `CP_DROPDOWNBUTTONRIGHT` for the chevron. The last
/// one is the surprise, and it is the reason the button is no longer filled
/// before it is drawn: sampled, the part paints **only** a chevron — no face, no
/// bevel, no separator — which is exactly what the sheet shows, a chevron
/// floating on the field's own ground. Filling a `Control`-grey button under it
/// first would put back the Windows 7 look the theme has stopped drawing.
///
/// With them OFF every one of the three falls back to what it always was: a
/// sunken or raised `DrawEdge` bevel, a `Control`-faced button and the library's
/// own vector chevron.
///
/// `FlatStyle::Flat` and `Popup` never ask the theme at all. That is the
/// property's meaning — they are the two values that opt a control *out* of the
/// visual style — and their single flat ring is drawn from the system colours in
/// both branches.
fn paint_combo(combo: &ComboBox, c: &dyn ControlCanvas, bounds: Rect, st: ControlState) {
    let visuals = c.visuals();
    let colors = &visuals.colors;
    let ih = item_height_for(visuals, combo.item_height);
    let line_h = ih + COMBO_CHROME_V;
    let editable = combo.drop_down_style != ComboBoxStyle::DropDownList;
    let enabled = combo.base.control.enabled;

    // The single-line field: the whole control for DropDown/DropDownList, the top
    // strip for Simple.
    let field = Rect::new(bounds.left, bounds.top, bounds.right, bounds.top + line_h);
    let (ground, ink, style) = if editable {
        (
            or_system(combo.base.control.back_color, colors.window),
            or_system(combo.base.control.fore_color, colors.window_text),
            Border3DStyle::Sunken,
        )
    } else {
        (
            or_system(combo.base.control.back_color, colors.control),
            or_system(combo.base.control.fore_color, colors.control_text),
            Border3DStyle::Raised,
        )
    };
    c.fill_rect(&field, &ground);

    // Two gates before the theme is asked at all.
    //
    // `FlatStyle` is the property's own meaning: `Flat` and `Popup` are the two
    // values that opt a control OUT of the visual style, so they keep the system
    // colours in both branches. `Standard` and `System` are theme-drawn.
    //
    // The second gate is `BackColor`. A themed part is OPAQUE over the rectangle
    // it covers, so a `DropDownList` face — which the theme owns entirely, and
    // which nothing is painted back over — would silently swallow an explicit
    // `BackColor`. An editable combo needs no such gate: its client area is put
    // back in `ground` below, which is what makes the property visible there.
    let visual_styles = matches!(combo.flat_style, FlatStyle::Standard | FlatStyle::System);
    let ambient_face = combo.base.control.back_color.is_none();
    let themed = visual_styles
        && if editable {
            c.draw_theme_part(
                theme::class::COMBOBOX,
                CP_BORDER,
                combo_border_state(st, enabled),
                field,
                ground,
            )
        } else {
            ambient_face
                && c.draw_theme_part(
                    theme::class::COMBOBOX,
                    CP_READONLY,
                    combo_readonly_state(st, enabled),
                    field,
                    ground,
                )
        };
    let inner = if themed {
        // Two rings in, the same interior `draw_edge` reports for a sunken or
        // raised bevel, so the text and the button sit where they always did.
        let interior = edge_interior(&field, 2, c.scale());
        // An editable combo's client area is the control's, so the theme's own
        // white fill is put back in `BackColor`; a `DropDownList` IS the part's
        // face — painting over it would erase the very thing that was asked for.
        if editable {
            c.fill_rect(&interior, &ground);
        }
        interior
    } else {
        c.draw_edge(&field, field_edge(combo.flat_style, style), Border3DSide::ALL)
    };

    // The button sits at the trailing edge of the field's interior, sized by the
    // system rather than by the row: a combo button is a scroll bar wide.
    let simple = combo.drop_down_style == ComboBoxStyle::Simple;
    let btn_w = visuals.metrics.vertical_scroll_width.min(inner.right - inner.left);
    let button = Rect::new(inner.right - btn_w, inner.top, inner.right, inner.bottom);
    // `Simple` has no button, so its edit runs the full width of the field.
    let text_right = if simple { inner.right } else { button.left } - LIST_PAD_X;
    let text_area = Rect::new(inner.left + LIST_PAD_X, inner.top, text_right, inner.bottom);

    let shown =
        combo.selected_item().map(str::to_string).unwrap_or_else(|| combo.base.control.text.clone());

    match combo.drop_down_style {
        ComboBoxStyle::DropDown if st.focused => {
            // An editable combo shows its text SELECTED when it holds the focus,
            // which is the state the reference sheet captures: the run sits on the
            // system `Highlight` band and turns `HighlightText`. Without the
            // focus it is plain ink, which is the sheet's `Simple` combo.
            if !shown.is_empty() {
                let w = c.measure(&shown, &visuals.fonts.message);
                let hl = Rect::new(
                    text_area.left,
                    text_area.top + 1.0,
                    (text_area.left + w).min(text_area.right),
                    text_area.bottom - 1.0,
                );
                c.fill_rect(&hl, &colors.highlight);
                c.text_ellipsis(&shown, &text_area, &visuals.fonts.message, &colors.highlight_text);
            }
        }
        ComboBoxStyle::DropDown | ComboBoxStyle::DropDownList | ComboBoxStyle::Simple => {
            c.text_ellipsis(&shown, &text_area, &visuals.fonts.message, &ink);
        }
    }

    if simple {
        // The always-open list occupies the rest of the control's box, framed like
        // any other list.
        let list = Rect::new(bounds.left, field.bottom + 2.0, bounds.right, bounds.bottom);
        if list.bottom > list.top {
            paint_list(
                c,
                list,
                &ListPaint {
                    base:      &combo.base.control,
                    items:     &combo.items,
                    item_h:    ih,
                    top_index: 0,
                    border:    BorderStyle::Fixed3D,
                    checkbox:  None,
                    selected:  &|i| i as i32 == combo.base.selected_index,
                },
            );
        }
    } else {
        // What the chevron is composited onto. When the chrome came from the
        // theme, that is the part's own fill — `#FFFFFF` for an editable combo,
        // which `Window` is exactly, and `#FDFDFD` for a `DropDownList`, which it
        // is within two levels on the glyph's antialiased edge only. Naming that
        // second colour is not an option: it is the theme's, not the system's,
        // and writing it down is the mistake `crate::theme` exists to prevent.
        let under = if themed { colors.window } else { ground };
        if !(visual_styles
            && c.draw_theme_part(
                theme::class::COMBOBOX,
                CP_DROPDOWNBUTTONRIGHT,
                combo_button_state(st, enabled),
                button,
                under,
            ))
        {
            // A raised `Control`-faced button with the toolkit's chevron. The
            // arrow stays a VECTOR icon: the UI face has no arrow glyphs, so a
            // character like « ▾ » would render as a tofu box.
            c.fill_rect(&button, &colors.control);
            c.draw_edge(&button, Border3DStyle::Raised, Border3DSide::ALL);
            c.vector_icon("ChevronDown", &button, DROPDOWN_GLYPH, &colors.control_text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Defaults (asserted against the catalogue) ────────────────────────

    #[test]
    fn listcontrol_base_defaults_match_the_catalogue() {
        let b = ListControlBase::default();
        assert_eq!(b.selected_index, -1, "nothing selected");
        assert!(!b.formatting_enabled);
        assert!(b.data_source.is_none() && b.selected_value.is_none());
        assert!(b.display_member.is_empty() && b.value_member.is_empty() && b.format_string.is_empty());
    }

    #[test]
    fn combobox_defaults_match_the_catalogue() {
        let cb = ComboBox::default();
        assert_eq!(cb.drop_down_style, ComboBoxStyle::DropDown);
        assert_eq!(cb.drop_down_height, 106);
        assert_eq!(cb.max_drop_down_items, 8);
        assert_eq!(cb.max_length, 0);
        assert!(cb.integral_height);
        assert!(!cb.sorted);
        assert_eq!(cb.draw_mode, DrawMode::Normal);
        assert_eq!(cb.flat_style, FlatStyle::Standard);
        assert_eq!(cb.auto_complete_mode, AutoCompleteMode::None);
        assert_eq!(cb.auto_complete_source, AutoCompleteSource::None);
        // Reaches through Deref to the inherited slot.
        assert_eq!(cb.selected_index, -1);
    }

    #[test]
    fn listbox_defaults_match_the_catalogue() {
        let lb = ListBox::default();
        assert_eq!(lb.selection_mode, SelectionMode::One);
        assert_eq!(lb.border_style, BorderStyle::Fixed3D);
        assert!(lb.integral_height);
        assert!(lb.use_tab_stops);
        assert!(!lb.sorted && !lb.multi_column && !lb.horizontal_scrollbar);
        assert_eq!(lb.column_width, 0);
        assert_eq!(lb.top_index, 0);
    }

    #[test]
    fn checkedlistbox_defaults_match_the_catalogue() {
        let cl = CheckedListBox::default();
        assert!(!cl.check_on_click);
        assert!(!cl.three_d_check_boxes);
        assert!(!cl.use_compatible_text_rendering);
        // Inherited default via the composed ListBox.
        assert_eq!(cl.selection_mode, SelectionMode::One);
    }

    #[test]
    fn the_deref_chain_reaches_control_base() {
        let mut cb = ComboBox::new();
        cb.control_mut().text = "hi".into(); // ComboBox → … → ControlBase
        assert_eq!(cb.text, "hi");
        assert!(cb.enabled && cb.visible, "ControlBase defaults visible through Deref");

        let cl = CheckedListBox::new(); // CheckedListBox → ListBox → … → ControlBase
        assert!(cl.tab_stop);
    }

    // ── SelectionMode rules ──────────────────────────────────────────────

    #[test]
    fn selection_mode_none_never_selects() {
        let mut lb = ListBox::new();
        for s in ["a", "b", "c"] {
            lb.add_item(s);
        }
        lb.set_selection_mode(SelectionMode::None);
        lb.click(1, false, false);
        lb.set_selected(2, true);
        lb.set_selected_index(0);
        assert!(lb.selected_indices().is_empty());
        assert_eq!(lb.selected_index(), -1);
    }

    #[test]
    fn selection_mode_one_keeps_a_single_item() {
        let mut lb = ListBox::new();
        for s in ["a", "b", "c"] {
            lb.add_item(s);
        }
        lb.click(0, false, false);
        lb.click(2, true, true); // modifiers are meaningless in One
        assert_eq!(lb.selected_indices(), &[2]);
        assert_eq!(lb.selected_index(), 2);
    }

    #[test]
    fn multi_simple_toggles_on_every_plain_click() {
        let mut lb = ListBox::new();
        for s in ["a", "b", "c", "d"] {
            lb.add_item(s);
        }
        lb.set_selection_mode(SelectionMode::MultiSimple);
        lb.click(0, false, false);
        lb.click(2, false, false);
        lb.click(3, false, false);
        assert_eq!(lb.selected_indices(), &[0, 2, 3]);
        lb.click(2, false, false); // toggles 2 back off
        assert_eq!(lb.selected_indices(), &[0, 3]);
        // SelectedIndex mirrors the lowest selected row.
        assert_eq!(lb.selected_index(), 0);
    }

    #[test]
    fn multi_extended_uses_ctrl_and_shift() {
        let mut lb = ListBox::new();
        for s in ["a", "b", "c", "d", "e", "f"] {
            lb.add_item(s);
        }
        lb.set_selection_mode(SelectionMode::MultiExtended);
        // The reference's non-contiguous selection: Alpha, Gamma, Delta.
        lb.click(0, false, false); // replace → {0}
        lb.click(2, true, false); // ctrl toggle → {0,2}
        lb.click(3, true, false); // ctrl toggle → {0,2,3}
        assert_eq!(lb.selected_indices(), &[0, 2, 3]);

        // A bare click collapses back to one.
        lb.click(4, false, false);
        assert_eq!(lb.selected_indices(), &[4]);

        // Shift extends a contiguous range from the anchor.
        lb.click(1, false, false); // anchor → 1
        lb.click(4, false, true); // shift → 1..=4
        assert_eq!(lb.selected_indices(), &[1, 2, 3, 4]);
    }

    // ── Index clamping on removal ────────────────────────────────────────

    #[test]
    fn removing_a_lower_item_shifts_the_selection_down() {
        let mut lb = ListBox::new();
        for s in ["a", "b", "c", "d"] {
            lb.add_item(s);
        }
        lb.set_selection_mode(SelectionMode::MultiExtended);
        lb.click(2, false, false);
        lb.click(3, true, false);
        assert_eq!(lb.selected_indices(), &[2, 3]);
        lb.remove_item(0); // everything above shifts down one
        assert_eq!(lb.selected_indices(), &[1, 2]);
        assert_eq!(lb.items, vec!["b", "c", "d"]);
    }

    #[test]
    fn removing_the_selected_item_drops_it() {
        let mut lb = ListBox::new();
        for s in ["a", "b", "c"] {
            lb.add_item(s);
        }
        lb.set_selected_index(1);
        lb.remove_item(1);
        assert!(lb.selected_indices().is_empty());
        assert_eq!(lb.selected_index(), -1);
    }

    #[test]
    fn combobox_selected_index_clamps_and_follows_removal() {
        let mut cb = ComboBox::new();
        for s in ["a", "b", "c"] {
            cb.add_item(s);
        }
        cb.set_selected_index(99); // clamps to the last item
        assert_eq!(cb.selected_index, 2);
        assert_eq!(cb.selected_item(), Some("c"));
        assert_eq!(cb.text, "c", "Text follows the selection");
        cb.remove_item(0); // "c" slides from 2 to 1
        assert_eq!(cb.selected_index, 1);
        assert_eq!(cb.selected_item(), Some("c"));
    }

    #[test]
    fn top_index_is_clamped_to_the_item_range() {
        let mut lb = ListBox::new();
        for s in ["a", "b"] {
            lb.add_item(s);
        }
        lb.set_top_index(99);
        assert_eq!(lb.top_index, 1);
        lb.remove_item(0);
        assert_eq!(lb.top_index, 0);
        lb.remove_item(0);
        assert_eq!(lb.top_index, 0, "an empty list rests at 0");
    }

    // ── Sorted insertion (an ordering trap) ──────────────────────────────

    #[test]
    fn sorted_insert_keeps_the_selection_on_its_own_item() {
        let mut lb = ListBox::new();
        lb.sorted = true;
        lb.add_item("Beta"); // → [Beta]
        lb.set_selected_index(0); // select Beta
        lb.add_item("Alpha"); // sorts ahead of Beta → [Alpha, Beta]
        assert_eq!(lb.items, vec!["Alpha", "Beta"]);
        assert_eq!(lb.selected_index(), 1, "the selection followed Beta");
        assert_eq!(lb.selected_indices(), &[1]);
    }

    // ── CheckState cycling ───────────────────────────────────────────────

    #[test]
    fn a_click_cycle_never_lands_on_indeterminate() {
        let mut cl = CheckedListBox::new();
        cl.add_item("x", CheckState::Unchecked);
        cl.toggle_check(0);
        assert_eq!(cl.get_item_check_state(0), CheckState::Checked);
        cl.toggle_check(0);
        assert_eq!(cl.get_item_check_state(0), CheckState::Unchecked);
    }

    /// The trap: clicking an `Indeterminate` row goes to `Unchecked`, not to
    /// `Checked` — WinForms collapses any non-unchecked state on click.
    #[test]
    fn clicking_an_indeterminate_item_goes_to_unchecked() {
        let mut cl = CheckedListBox::new();
        cl.add_item("x", CheckState::Indeterminate);
        cl.toggle_check(0);
        assert_eq!(cl.get_item_check_state(0), CheckState::Unchecked);
    }

    #[test]
    fn check_on_click_toggles_while_plain_selection_does_not() {
        let mut cl = CheckedListBox::new();
        cl.add_item("x", CheckState::Unchecked);
        cl.add_item("y", CheckState::Unchecked);

        cl.click(0); // check_on_click is false: selects, does not check
        assert_eq!(cl.get_item_check_state(0), CheckState::Unchecked);
        assert_eq!(cl.selected_index(), 0);

        cl.check_on_click = true;
        cl.click(1);
        assert_eq!(cl.get_item_check_state(1), CheckState::Checked);
    }

    #[test]
    fn checked_indices_include_indeterminate_rows() {
        let mut cl = CheckedListBox::new();
        cl.add_item("a", CheckState::Checked);
        cl.add_item("b", CheckState::Unchecked);
        cl.add_item("c", CheckState::Indeterminate);
        assert_eq!(cl.checked_indices(), vec![0, 2]);
        assert!(cl.get_item_checked(2), "indeterminate counts as checked");
    }

    #[test]
    fn removing_a_checked_row_keeps_states_aligned() {
        let mut cl = CheckedListBox::new();
        cl.add_item("a", CheckState::Checked);
        cl.add_item("b", CheckState::Indeterminate);
        cl.add_item("c", CheckState::Unchecked);
        cl.remove_item(0);
        assert_eq!(cl.items, vec!["b", "c"]);
        assert_eq!(cl.get_item_check_state(0), CheckState::Indeterminate);
        assert_eq!(cl.get_item_check_state(1), CheckState::Unchecked);
    }

    #[test]
    fn checkedlistbox_refuses_the_multi_modes() {
        let mut cl = CheckedListBox::new();
        cl.set_selection_mode(SelectionMode::MultiExtended);
        assert_eq!(cl.selection_mode, SelectionMode::One, "coerced to One");
        cl.set_selection_mode(SelectionMode::None);
        assert_eq!(cl.selection_mode, SelectionMode::None);
    }

    // ── Pure geometry ────────────────────────────────────────────────────

    #[test]
    fn integral_height_snaps_to_whole_rows() {
        // 100 tall, 1px borders → 98 inner, 20 per row → 4 rows → 82.
        assert_eq!(integral_height(100.0, 20.0, 1.0), 82.0);
        // Exactly filled leaves nothing to trim.
        assert_eq!(integral_height(42.0, 20.0, 1.0), 42.0);
        // A zero row height cannot snap; the height passes through.
        assert_eq!(integral_height(37.0, 0.0, 1.0), 37.0);
    }

    #[test]
    fn item_height_auto_falls_back_to_the_font_row() {
        assert_eq!(item_height_dip(0), ITEM_HEIGHT_DEFAULT);
        assert_eq!(item_height_dip(24), 24.0);
    }

    /// The measuring fallback and the font-derived row must be the SAME number
    /// for the default UI font, or a list would measure one row height and paint
    /// another. 15 and 18 are what the live toolkit reports for `ListBox` and
    /// `CheckedListBox` at Segoe UI 9 pt.
    #[test]
    fn the_auto_row_is_the_toolkits_item_height() {
        // 12 DIP is the default UI font's em size — Segoe UI 9 pt.
        assert_eq!(auto_item_height(12.0), ITEM_HEIGHT_DEFAULT);
        assert_eq!(ITEM_HEIGHT_DEFAULT, 15.0);
        assert_eq!(checked_row_height(0, ITEM_HEIGHT_DEFAULT), 18.0);
        // An explicit ItemHeight wins, on both kinds of list.
        assert_eq!(checked_row_height(24, ITEM_HEIGHT_DEFAULT), 24.0);
        // A bigger UI font makes a taller row, without anything being scaled by
        // the DPI: the em size is already in DIP.
        assert!(auto_item_height(16.0) > ITEM_HEIGHT_DEFAULT);
    }

    // ── The state mappings (pure — no theme, no canvas) ──────────────────

    /// `Indeterminate` is a **greyed tick**, i.e. the same state a checked row on
    /// a dead control gets — see [`check_glyph_state`] and the module docs. This
    /// is the mapping the reference sheet forced; it is not the obvious one.
    #[test]
    fn an_indeterminate_row_asks_for_the_greyed_tick() {
        use windows::Win32::UI::Controls as sdk;
        assert_eq!(check_glyph_state(CheckState::Unchecked, true), sdk::CBS_UNCHECKEDNORMAL.0);
        assert_eq!(check_glyph_state(CheckState::Checked, true), sdk::CBS_CHECKEDNORMAL.0);
        assert_eq!(check_glyph_state(CheckState::Indeterminate, true), sdk::CBS_CHECKEDDISABLED.0);
        assert_ne!(check_glyph_state(CheckState::Indeterminate, true), sdk::CBS_MIXEDNORMAL.0);
        assert_ne!(check_glyph_state(CheckState::Indeterminate, true), sdk::CBS_MIXEDDISABLED.0);
        // A dead control greys the empty wells too, and lands an indeterminate
        // row on the state it was already using.
        assert_eq!(check_glyph_state(CheckState::Unchecked, false), sdk::CBS_UNCHECKEDDISABLED.0);
        assert_eq!(
            check_glyph_state(CheckState::Checked, false),
            check_glyph_state(CheckState::Indeterminate, false)
        );
    }

    /// Disabled outranks everything, and focus outranks hot — a combo under the
    /// pointer keeps its accent frame.
    #[test]
    fn the_combo_states_rank_disabled_then_focus_then_hot() {
        let hot = ControlState { hot: true, ..ControlState::default() };
        let focused_hot = ControlState { hot: true, focused: true, ..ControlState::default() };
        let pressed = ControlState { pressed: true, hot: true, ..ControlState::default() };

        assert_eq!(combo_border_state(ControlState::default(), true), state::CBB_NORMAL);
        assert_eq!(combo_border_state(hot, true), state::CBB_HOT);
        assert_eq!(combo_border_state(focused_hot, true), state::CBB_FOCUSED);
        assert_eq!(combo_border_state(focused_hot, false), state::CBB_DISABLED);

        // `CP_READONLY` has no focused state — the theme does not define one.
        assert_eq!(combo_readonly_state(focused_hot, true), state::CBRO_HOT);
        assert_eq!(combo_readonly_state(pressed, true), state::CBRO_PRESSED);
        assert_eq!(combo_readonly_state(pressed, false), state::CBRO_DISABLED);

        assert_eq!(combo_button_state(ControlState::default(), true), state::CBXSR_NORMAL);
        assert_eq!(combo_button_state(pressed, true), state::CBXSR_PRESSED);
        assert_eq!(combo_button_state(hot, false), state::CBXSR_DISABLED);
    }

    // ── The themed parts, pinned against the reference sheet ─────────────
    //
    // Every hex below was read out of
    // `C:\kubuno-build\winforms-ref\shots\03-listcontrol.png` first and matched
    // by a part second — the order the `theme` module insists on, because the
    // wrong part does not fail, it draws something plausible. Pinning them here
    // turns a Windows update that moves a part into a failing test rather than a
    // slow drift away from the toolkit.
    //
    // The sheet was captured at 200 %, so its GEOMETRY is twice these samples';
    // its colours are not, because a theme's palette does not scale.

    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, SelectObject,
        BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows::Win32::UI::Controls as sdk;
    use windows::Win32::UI::Controls::{
        CloseThemeData, DrawThemeBackground, IsAppThemed, IsThemeActive, HTHEME,
    };
    use windows::Win32::UI::HiDpi::OpenThemeDataForDpi;

    /// The `#F0F0F0` form ground the sheet was captured on, as a BGRA word — so a
    /// sampled pixel is comparable with the sheet directly.
    const FORM: u32 = 0xFFF0_F0F0;
    /// The `#FFFFFF` a list, a field and an editable combo put behind their own
    /// chrome.
    const FIELD: u32 = 0xFFFF_FFFF;

    /// Renders one themed part at 96 DPI over `bg` and hands back its pixels.
    ///
    /// The same GDI round trip `theme::render_part` performs, stopping short of
    /// the Direct2D upload so the assertions need no device, no window and no
    /// swap chain.
    ///
    /// `None` is a **skip**, not a failure: with visual styles off — including
    /// under [`theme::CLASSIC_ENV`], which is how the classic branch is
    /// exercised — the classic painting is the correct one and there is no
    /// themed pixel to assert.
    fn sample(class: PCWSTR, part: i32, state: i32, w: i32, h: i32, bg: u32) -> Option<Vec<u32>> {
        if std::env::var_os(theme::CLASSIC_ENV).is_some() {
            return None;
        }
        if !unsafe { IsThemeActive().as_bool() && IsAppThemed().as_bool() } {
            return None;
        }
        let handle = unsafe { OpenThemeDataForDpi(None, class, 96) };
        if handle.is_invalid() {
            return None;
        }
        let pixels = unsafe { render(handle, part, state, w, h, bg) };
        let _ = unsafe { CloseThemeData(handle) };
        pixels
    }

    /// Pre-fill, draw, flush, read back — see [`sample`].
    ///
    /// # Safety
    ///
    /// `handle` must be a live `HTHEME` and `w`/`h` positive. Every GDI object
    /// created here is released on every path.
    unsafe fn render(
        handle: HTHEME,
        part: i32,
        state: i32,
        w: i32,
        h: i32,
        bg: u32,
    ) -> Option<Vec<u32>> {
        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() {
            return None;
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                // Negative: TOP-DOWN. A bottom-up DIB stores its first row last,
                // so every row index below would be mirrored — invisible on a
                // symmetric part, and silently wrong on the `CP_READONLY` face,
                // whose whole tell is that its bottom edge is the darker one.
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bitmap = match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(b) if !b.is_invalid() && !bits.is_null() => b,
            _ => {
                let _ = DeleteDC(dc);
                return None;
            }
        };
        let previous = SelectObject(dc, bitmap.into());
        let pixels = std::slice::from_raw_parts_mut(bits.cast::<u32>(), (w as usize) * (h as usize));
        pixels.fill(bg);
        let rect = RECT { left: 0, top: 0, right: w, bottom: h };
        let drawn = DrawThemeBackground(handle, dc, part, state, &rect, None).is_ok();
        // GDI batches per thread: reading the bits without this can read them
        // BEFORE the theme has drawn — intermittently, and under load only.
        let _ = GdiFlush();
        let out = drawn.then(|| pixels.to_vec());
        SelectObject(dc, previous);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        out
    }

    /// `#RRGGBB` for a BGRA word, so a failure prints the colour a human can
    /// compare with the sheet rather than a decimal.
    fn hex(px: u32) -> String {
        format!("#{:02X}{:02X}{:02X}", (px >> 16) & 0xFF, (px >> 8) & 0xFF, px & 0xFF)
    }

    /// Sheet: the `ListBox` and `CheckedListBox` frames are `#ABADB3`, with one
    /// pixel of the list's own ground inside them (x=362 and x=363 on the sheet)
    /// before the first row begins — which is why [`fixed3d_frame`] reports its
    /// interior two device pixels in.
    #[test]
    fn the_list_frame_is_the_sheets_border() {
        let (w, h) = (121usize, 15usize);
        let Some(px) = sample(w!("EDIT"), EP_EDITTEXT, ETS_NORMAL, w as i32, h as i32, FIELD) else {
            eprintln!("[lists] visual styles unavailable — themed list frame not asserted");
            return;
        };
        let mid = h / 2;
        assert_eq!(hex(px[mid * w]), "#ABADB3", "the list frame");
        assert_eq!(hex(px[mid * w + 1]), "#FFFFFF", "the pad inside it");
        assert_eq!(hex(px[w / 2]), "#ABADB3", "and the same line along the top");
    }

    /// Sheet: both editable combos are framed in the accent `#0078D4` over
    /// `#FFFFFF` — `CBB_FOCUSED`, which is why [`ComboBox::paint`] paints
    /// focused. `CBB_NORMAL` is a different, greyer frame, which is the proof
    /// that the state id reaches the theme rather than being ignored.
    #[test]
    fn the_editable_combo_frame_is_the_sheets_accent() {
        let (w, h) = (240usize, 23usize);
        let at = |state| sample(w!("COMBOBOX"), CP_BORDER, state, w as i32, h as i32, FORM);
        let (Some(focused), Some(normal), Some(disabled)) =
            (at(state::CBB_FOCUSED), at(state::CBB_NORMAL), at(state::CBB_DISABLED))
        else {
            eprintln!("[lists] visual styles unavailable — themed combo frame not asserted");
            return;
        };
        let mid = h / 2;
        assert_eq!(hex(focused[mid * w]), "#0078D4", "the focused frame");
        assert_eq!(hex(focused[mid * w + 1]), "#FFFFFF", "the field behind it");
        assert_eq!(hex(normal[mid * w]), "#8D8D8D", "the resting frame");
        assert_eq!(hex(disabled[mid * w]), "#C8C8C8", "the dead frame");
    }

    /// Sheet: the `DropDownList` is not a field at all — it is a button face,
    /// `#FDFDFD` under a `#D2D2D2` top edge and over a `#BCBCBC` bottom one. The
    /// asymmetry is the tell, and it is why the top-down DIB in [`render`]
    /// matters: mirrored, this part still looks like a button.
    #[test]
    fn the_dropdownlist_face_is_the_sheets_button_face() {
        let (w, h) = (240usize, 23usize);
        let Some(px) =
            sample(w!("COMBOBOX"), CP_READONLY, state::CBRO_NORMAL, w as i32, h as i32, FORM)
        else {
            eprintln!("[lists] visual styles unavailable — themed readonly face not asserted");
            return;
        };
        assert_eq!(hex(px[(h / 2) * w + w / 2]), "#FDFDFD", "the face");
        assert_eq!(hex(px[w / 2]), "#D2D2D2", "the top edge");
        assert_eq!(hex(px[(h - 1) * w + w / 2]), "#BCBCBC", "the bottom edge, darker");
    }

    /// Sheet: the drop-down button is a **chevron on the field's own ground** —
    /// no face, no bevel, no separator. The corners prove it: they still hold the
    /// pre-fill the part was handed, so anything painted under the part would
    /// show through, which is why the themed branch fills nothing first.
    #[test]
    fn the_drop_down_button_is_a_chevron_and_nothing_else() {
        let (w, h) = (17usize, 21usize);
        let Some(px) = sample(
            w!("COMBOBOX"),
            CP_DROPDOWNBUTTONRIGHT,
            state::CBXSR_NORMAL,
            w as i32,
            h as i32,
            FORM,
        ) else {
            eprintln!("[lists] visual styles unavailable — themed chevron not asserted");
            return;
        };
        assert_eq!(hex(px[0]), "#F0F0F0", "the part paints no face: the corner is the pre-fill");
        assert_eq!(hex(px[w - 1]), "#F0F0F0", "nor at the other corner");
        assert_eq!(hex(px[(h - 1) * w]), "#F0F0F0", "nor along the bottom");
        // The two arms of the chevron cross the middle row, in the same dark
        // grey the sheet shows at 200 %.
        // The two arms of the chevron cross the middle row and nothing else does:
        // ink, its antialiasing, then the pre-fill again, symmetric about the
        // centre. The sheet shows the same grey arms at 200 %, wider and softer.
        let mid = h / 2;
        assert_eq!(hex(px[mid * w + 5]), "#6B6B6B", "the left arm's ink");
        assert_eq!(hex(px[mid * w + w - 7]), "#6B6B6B", "the right arm's, symmetric");
        assert_eq!(hex(px[mid * w + 6]), "#C2C2C2", "antialiased into the ground, not onto a face");
        assert_eq!(hex(px[mid * w + w / 2]), "#F0F0F0", "and the gap between them is the pre-fill");
    }

    /// Sheet: an empty well is `#626262` over `#F3F3F3`, a ticked one is the
    /// solid accent `#005FB8`, and an indeterminate one is the SAME tick in
    /// `#C3C3C3` grey — not the theme's mixed glyph, which is one wide light bar
    /// where the sheet has two separated strokes.
    #[test]
    fn the_check_wells_are_the_sheets_glyphs() {
        let (w, h) = (13usize, 13usize);
        let at = |state| sample(w!("BUTTON"), CP_CHECKBOX, state, w as i32, h as i32, FIELD);
        let (Some(empty), Some(ticked), Some(greyed), Some(mixed)) = (
            at(state::CBS_UNCHECKEDNORMAL),
            at(state::CBS_CHECKEDNORMAL),
            at(check_glyph_state(CheckState::Indeterminate, true)),
            at(sdk::CBS_MIXEDDISABLED.0),
        ) else {
            eprintln!("[lists] visual styles unavailable — themed check wells not asserted");
            return;
        };
        let mid = h / 2;
        assert_eq!(hex(empty[mid * w]), "#626262", "the empty well's frame");
        assert_eq!(hex(empty[mid * w + w / 2]), "#F3F3F3", "and its field");
        assert_eq!(hex(ticked[mid * w + 1]), "#005FB8", "the ticked well is a solid accent");
        assert_eq!(hex(greyed[mid * w + 1]), "#C3C3C3", "the indeterminate well is grey");

        // The refusal, pinned: at the centre of its middle row the mixed glyph
        // is its light BAR, where the greyed tick — the sheet's `Delta` well —
        // is still fill. Two different pictures, and only one is on the sheet.
        assert_eq!(hex(greyed[mid * w + w / 2]), "#C3C3C3", "the tick leaves the centre filled");
        assert_eq!(hex(mixed[mid * w + w / 2]), "#F0F0F0", "the mixed glyph bars it");
    }

    /// The part that was measured and **refused** — see the module docs.
    ///
    /// `LVP_LISTITEM` is the obvious choice for a selected row, and it carries no
    /// selection colour at all: opened the way [`crate::theme`] opens a class,
    /// all six `LISS_*` states render one identical hollow rectangle and the
    /// centre of the row is still the pre-fill. `LISS_SELECTED` and
    /// `LISS_SELECTEDNOTFOCUS` being byte-for-byte equal is the whole finding —
    /// there is no blue band and no grey one to be had here.
    ///
    /// If a future Windows starts defining them, this fails and the row goes back
    /// on the measuring bench instead of quietly keeping the system fill.
    #[test]
    fn the_listview_row_part_carries_no_selection_colour() {
        let (w, h) = (121usize, 15usize);
        let at =
            |state| sample(w!("LISTVIEW"), sdk::LVP_LISTITEM.0, state, w as i32, h as i32, FIELD);
        let Some(normal) = at(sdk::LISS_NORMAL.0) else {
            eprintln!("[lists] visual styles unavailable — the row refusal is not asserted");
            return;
        };
        let mid = h / 2;
        assert_eq!(
            hex(normal[mid * w + w / 2]),
            "#FFFFFF",
            "LVP_LISTITEM paints no fill: the centre is still the pre-fill"
        );
        for state in [
            sdk::LISS_HOT.0,
            sdk::LISS_SELECTED.0,
            sdk::LISS_DISABLED.0,
            sdk::LISS_SELECTEDNOTFOCUS.0,
            sdk::LISS_HOTSELECTED.0,
        ] {
            let Some(other) = at(state) else { return };
            assert_eq!(other, normal, "LISS state {state} differs from LISS_NORMAL — re-measure");
        }
    }
}
