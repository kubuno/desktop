//! What a view's code-behind uses, in one import: `use kubuno_views::prelude::*;`
//! (`vskubuno/docs/EVENTS.md` §5.4). The Kubuno templates start with it, and
//! `kubuno/createHandler` adds it when it writes a typed handler into a file without it.
//!
//! It brings the view-model traits ([`ViewModel`], [`Value`]), the typed-handler vocabulary
//! ([`event_handlers`](crate::event_handlers), [`Sender`], [`ElementRef`], [`EventArgs`], every
//! standard args type), the control classes ([`crate::controls`]: the types a sender is typed
//! with, and the bases of custom controls), the legacy [`HandlerTable`] /
//! [`handlers!`](crate::handlers), the UI-thread services of EVT-6 ([`UiHandle`] for async
//! handlers, [`UiDispatcher`], [`spawn_local`], [`delay`], [`Timer`]), and the control hierarchy
//! of EVT-7a (the level traits, `#[derive(Component)]`, [`EventCx`], [`PaintEventCx`],
//! [`ControlHost`]…). The structural element classes (`controls::items`) are left out: one of
//! them is named `Option`.

pub use crate::binding::{HandlerTable, Value, ValueConverter, ViewModel};
pub use crate::value_converter;
pub use crate::component::{
    BoundsSpecified, ButtonBase, ButtonBaseCore, ClassInfo, Component, ComponentCore, ContainerBase, ContainerBaseCore, ContainerControl, ContainerControlCore, Control, ControlCore,
    ControlHost, ControlStyles, CreateParams, EventCx, HostedControl, Keys, LabelBase, LabelBaseCore, Lineage, ListControl, ListControlCore, Message, PaintEventCx, PropertyValue, RangeBase,
    RangeBaseCore, ScrollableControl, ScrollableControlCore, Site, TextBoxBase, TextBoxBaseCore, UserControl, UserControlCore, View, ViewCore,
};
pub use crate::controls::{
    Accordion, Badge, Breadcrumb, Button, Callout, Card, CheckBox, CheckedListBox, ColorField, ComboBox, GradientField, DataTable, DatePicker, Dropdown, EmptyState, GroupBox, Icon, IconButton,
    Label, LinkLabel, ListBox, ListView, MaskedField, MonthCalendar, NumericField, PaintBox, Panel, ProgressBar, RadioButton, ScrollArea, SearchField, Separator, Slider, Spinner, Splitter,
    Stack, Stepper, Switch, Tabs, TextArea, TextField, Toolbar, TreeView, DockArea, WorkspaceShell, Avatar, PictureBox, Popover, Repeater, Sidebar, StatusBar,
    TableLayoutPanel,
};
pub use crate::event_handlers;
pub use crate::events::args::*;
pub use crate::events::{AnyElement, ArgsChain, Cancelable, ElementProps, ElementRef, ElementType, Event, EventArgs, EventSink, Handled, ReadOnlyArgs, Sender, Subscription};
// The geometry and canvas types of the overridable methods (`get_preferred_size(&self, canvas: &dyn
// Canvas, proposed: Size) -> Size`, `set_bounds_core(&mut self, bounds: Rect, …)`), EVT-7b.
pub use kubuno_ui::{Canvas, Rect, Size};
pub use crate::events::{delay, spawn_local, yield_now, AsyncResult, Cancelled, DispatchError, JoinHandle, Timer, UiDispatcher, UiHandle};
pub use crate::handlers;
// Debugging (vskubuno docs/DEBUGGING.md): `.break_on_err()` on a Result/Option.
pub use crate::debug::BreakOnError;
// Drawing (EVT-8): the WinForms-like `Graphics` of `e.graphics`, its value types, owner-draw modes.
pub use kubuno_ui::graphics::{
    Brush, Color, CompositingMode, DashStyle, DrawItemState, DrawMode, FillMode, Font, FontRole, FontStyle, GradientStop, Graphics, GraphicsPath, Image, InterpolationMode, LineCap,
    LineJoin, LinearGradientBrush, LinearGradientMode, Matrix, MatrixOrder, Pen, PenAlignment, PointF, RadialGradientBrush, RectExt, SizeF, SmoothingMode, StringAlignment,
    StringFormat, StringFormatFlags, StringTrimming, TextRenderingHint, WrapMode,
};
// Drag and drop (EVT-8).
pub use crate::dnd::{do_drag_drop, DragOperation};
