//! Data binding — `XML_VIEWS.md` §3: `{Binding Path}` / `{Binding Path,
//! Mode=TwoWay}`, evaluated against a small [`ViewModel`] trait (get/set by
//! path, a closed [`Value`] enum) rather than a reflection system. Because a
//! view is rebuilt every frame from live state anyway (§0), one-way binding is
//! "read `vm.get(path)` at the moment the element is built, every frame";
//! two-way binding is the same read plus, when the live widget reports a
//! change, `vm.set(path, new_value)` — see [`crate::node`] for where that
//! write actually happens (on the interaction that produced the new value,
//! not on every frame).
//!
//! [`PropSource`] is the thing a compiled node actually stores per property:
//! either the literal value the XML attribute carried, or a [`BindingSpec`]
//! plus the fallback to use while the path resolves to nothing (an unset key,
//! a type mismatch). It is resolved fresh every frame by
//! [`PropSource::resolve`], which is the "bind" half of §5's "compile once /
//! bind+paint every frame" split.

use std::collections::HashMap;

/// A dynamically typed value a [`ViewModel`] reads or writes.
///
/// Four variants: every property [`crate::registry::PropKind`] declares
/// (`Bool`, `F32`, `String`, and `Enum` — carried as its variant name, a
/// `String`) fits one of the first three; [`Value::List`] is the one
/// addition — rows of named fields, what an `ItemsSource="{Binding Path}"`
/// on `ListBox`/`CheckedListBox`/`ListView`/`TreeView`/`DataTable`/`Dropdown`/
/// `ComboBox` reads (`registry::families::data`'s module doc names this the
/// "`ItemsSource` gap"; this is what closes it). Still not a `Box<dyn Any>`:
/// a row is exactly as closed a shape as the other three (a named list of
/// more [`Value`]s), and exhaustively matchable.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    F32(f32),
    Str(String),
    /// Rows of an `ItemsSource` binding, in order — see [`Rows`]: a shared snapshot, so reading
    /// it every frame clones a pointer, and a list node knows it is unchanged from its stamp.
    List(Rows),
    /// Any Rust value handed to a property of a custom control (`#[property] messages:
    /// Shared<Vec<Message>>`) — see [`ObjectValue`]. Never written in XML: only bound.
    Object(ObjectValue),
}

/// The next [`Rows`]/[`ObjectValue`] stamp: one per content, never reused.
fn next_stamp() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// The rows of a [`Value::List`]: an immutable, shared snapshot (`Arc`) and its **stamp**, a
/// number that changes whenever the content may have changed and never otherwise.
///
/// Cloning is a pointer copy, so a view model can hand its list to a binding every frame for
/// free; a list node (`ListBox`, `ListView`, `DataTable`, `Repeater`…) compares the stamp with
/// the one it built its items from and does nothing when they are equal — the change detection
/// of a long list costs one integer comparison per frame, not a copy and a diff of every row.
///
/// Keep a `Rows` in the view model and change it in place ([`Rows::make_mut`], [`Rows::push`],
/// [`Rows::set`]): each change gets a new stamp. A `Vec<Row>` converted anew every frame
/// (`From<Vec<Row>>`) works too, but each conversion is a new content for the nodes (they then
/// compare the rows themselves before rebuilding anything).
#[derive(Clone)]
pub struct Rows {
    rows: std::sync::Arc<Vec<Row>>,
    stamp: u64,
}

impl Rows {
    /// An empty list.
    pub fn new() -> Self {
        Self::from_vec(Vec::new())
    }

    /// `rows`, as a new content.
    pub fn from_vec(rows: Vec<Row>) -> Self {
        Self { rows: std::sync::Arc::new(rows), stamp: next_stamp() }
    }

    /// The stamp of this content: equal stamps mean equal rows (the converse does not hold).
    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    /// Whether `other` is this very content (same stamp, or the same shared snapshot): what a
    /// node checks every frame.
    pub fn same(&self, other: &Rows) -> bool {
        self.stamp == other.stamp || std::sync::Arc::ptr_eq(&self.rows, &other.rows)
    }

    /// The rows, to change them in place (copied first when a binding still holds the previous
    /// snapshot): the list gets a new stamp.
    pub fn make_mut(&mut self) -> &mut Vec<Row> {
        self.stamp = next_stamp();
        std::sync::Arc::make_mut(&mut self.rows)
    }

    /// Appends a row (a new stamp).
    pub fn push(&mut self, row: Row) {
        self.make_mut().push(row);
    }

    /// Replaces row `index` (a new stamp); `false` when there is no such row.
    pub fn set(&mut self, index: usize, row: Row) -> bool {
        if index >= self.rows.len() {
            return false;
        }
        self.make_mut()[index] = row;
        true
    }

    /// Removes row `index` (a new stamp), if there is one.
    pub fn remove(&mut self, index: usize) -> Option<Row> {
        (index < self.rows.len()).then(|| self.make_mut().remove(index))
    }

    /// Removes every row (a new stamp).
    pub fn clear(&mut self) {
        self.make_mut().clear();
    }

    /// The rows as a plain vector (a copy).
    pub fn to_vec(&self) -> Vec<Row> {
        self.rows.as_ref().clone()
    }
}

impl Default for Rows {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for Rows {
    type Target = [Row];
    fn deref(&self) -> &[Row] {
        &self.rows
    }
}

impl PartialEq for Rows {
    /// The same content, or the same rows.
    fn eq(&self, other: &Self) -> bool {
        self.same(other) || self.rows == other.rows
    }
}

impl std::fmt::Debug for Rows {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.rows.iter()).finish()
    }
}

impl From<Vec<Row>> for Rows {
    fn from(rows: Vec<Row>) -> Self {
        Self::from_vec(rows)
    }
}

impl FromIterator<Row> for Rows {
    fn from_iter<I: IntoIterator<Item = Row>>(iter: I) -> Self {
        Self::from_vec(iter.into_iter().collect())
    }
}

impl<'a> IntoIterator for &'a Rows {
    type Item = &'a Row;
    type IntoIter = std::slice::Iter<'a, Row>;
    fn into_iter(self) -> Self::IntoIter {
        self.rows.iter()
    }
}

/// The item of a `<Repeater>` whose template is painting (or whose event is being handled): what
/// a handler named in an item template reads to know which item raised it —
/// `kubuno_views::binding::current_item()`.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemContext {
    /// Its index in the bound list.
    pub index: usize,
    /// Its identity (the `ItemKey` field's value, else the index), stable across list changes.
    pub key: String,
    /// Its row.
    pub row: Row,
}

thread_local! {
    static CURRENT_ITEM: std::cell::RefCell<Option<ItemContext>> = const { std::cell::RefCell::new(None) };
}

/// The `<Repeater>` item being painted or handled on this thread, if any (see [`ItemContext`]).
pub fn current_item() -> Option<ItemContext> {
    CURRENT_ITEM.with(|c| c.borrow().clone())
}

/// Runs `f` with `item` as the [`current_item`].
pub(crate) fn with_current_item<R>(item: ItemContext, f: impl FnOnce() -> R) -> R {
    let before = CURRENT_ITEM.with(|c| c.replace(Some(item)));
    let out = f();
    CURRENT_ITEM.with(|c| *c.borrow_mut() = before);
    out
}

/// The change detection of a bound list, kept by a list node: [`ListWatch::changed`] answers the
/// rows only when they are not those it answered last time — the same snapshot (its stamp) costs
/// one comparison; a new snapshot of equal rows (a view model that converts its `Vec` every
/// frame) a comparison of the rows, never a rebuild of the node's items.
#[derive(Default, Clone)]
pub struct ListWatch {
    seen: Option<Rows>,
}

impl ListWatch {
    /// The rows `spec` resolves to in `vm`, when they changed since the last call; `None` when
    /// they did not, or when `spec` is absent or does not resolve to a list (the node keeps what
    /// it shows).
    pub fn changed(&mut self, vm: &dyn ViewModel, spec: Option<&BindingSpec>) -> Option<Rows> {
        let spec = spec?;
        let Some(Value::List(rows)) = vm.get(&spec.path) else { return None };
        if self.seen.as_ref().is_some_and(|seen| *seen == rows) {
            // Keep the newest snapshot: the next frame then compares stamps only.
            self.seen = Some(rows);
            return None;
        }
        self.seen = Some(rows.clone());
        Some(rows)
    }

    /// The rows last answered.
    pub fn current(&self) -> Option<&Rows> {
        self.seen.as_ref()
    }

    /// Forgets the rows (the next [`ListWatch::changed`] answers them again).
    pub fn reset(&mut self) {
        self.seen = None;
    }
}

/// A Rust value carried by a binding ([`Value::Object`]): shared (`Arc`), compared by identity,
/// read back by type ([`ObjectValue::downcast_ref`]). What a custom control's property of any
/// type receives (see [`crate::component::Shared`]).
#[derive(Clone)]
pub struct ObjectValue {
    value: std::sync::Arc<dyn std::any::Any + Send + Sync>,
    stamp: u64,
}

impl ObjectValue {
    /// `value`, as a new content.
    pub fn new<T: std::any::Any + Send + Sync>(value: T) -> Self {
        Self::from_arc(std::sync::Arc::new(value))
    }

    /// A value already shared (handing the same `Arc` again is the same content).
    pub fn from_arc<T: std::any::Any + Send + Sync>(value: std::sync::Arc<T>) -> Self {
        Self { value, stamp: next_stamp() }
    }

    /// The value, when it is a `T`.
    pub fn downcast_ref<T: std::any::Any>(&self) -> Option<&T> {
        self.value.downcast_ref::<T>()
    }

    /// The shared value, when it is a `T`.
    pub fn downcast_arc<T: std::any::Any + Send + Sync>(&self) -> Option<std::sync::Arc<T>> {
        self.value.clone().downcast::<T>().ok()
    }

    /// The stamp of this handle (two handles of one `Arc` have different stamps but are equal).
    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    /// The identity of the shared value (its address): equal for every handle of one `Arc`.
    pub fn identity(&self) -> usize {
        std::sync::Arc::as_ptr(&self.value) as *const () as usize
    }
}

impl PartialEq for ObjectValue {
    /// The same shared value.
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.value, &other.value)
    }
}

impl std::fmt::Debug for ObjectValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Object#{}", self.stamp)
    }
}

// Plain Rust values as property values (`control.set_property("Maximum", 10.0)`, the `kubuno` crate).
impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::F32(v)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::F32(v as f32)
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::F32(v as f32)
    }
}

impl From<u32> for Value {
    fn from(v: u32) -> Self {
        Value::F32(v as f32)
    }
}

impl From<usize> for Value {
    fn from(v: usize) -> Self {
        Value::F32(v as f32)
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Str(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Str(v.to_string())
    }
}

impl From<Vec<Row>> for Value {
    fn from(v: Vec<Row>) -> Self {
        Value::List(Rows::from_vec(v))
    }
}

impl From<Rows> for Value {
    fn from(v: Rows) -> Self {
        Value::List(v)
    }
}

impl From<ObjectValue> for Value {
    fn from(v: ObjectValue) -> Self {
        Value::Object(v)
    }
}

/// One row of a [`Value::List`] — named fields, in declaration order (a
/// `DataTable`/`ListView` reads its columns' `Binding` names off a row this
/// way, in the order the caller built it; a `HashMap` would not preserve
/// that order and a handful of short-lived fields gains nothing from one).
/// The view model builds these; nothing in this crate ever needs to look one
/// up by anything other than a field name (see [`Row::get`]/[`Row::text`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row(Vec<(String, Value)>);

impl Row {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one named field, chainable — `Row::new().with("Text", Value::Str(name))`.
    pub fn with(mut self, name: impl Into<String>, value: Value) -> Self {
        self.0.push((name.into(), value));
        self
    }

    /// The field named `name`, if the row has one.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    /// Every field, in the order they were added.
    pub fn fields(&self) -> &[(String, Value)] {
        &self.0
    }

    /// The field named `name`, rendered as display text — `Str` as itself,
    /// `F32`/`Bool` via their own `Display`, `List`/a missing field as empty.
    /// What every row-consuming node (`registry::families::data`, `text`'s
    /// `Dropdown`/`ComboBox`) reads a label/value/cell from: a row field is
    /// declared by the view model as whatever [`Value`] shape is natural for
    /// it (a `bool` for a checkbox column, a number for an amount…), and
    /// every one of those has an obvious text rendering except a nested list,
    /// which no `DisplayMember`/`Binding`/`Text` in this crate ever names.
    pub fn text(&self, name: &str) -> String {
        match self.get(name) {
            Some(Value::Str(s)) => s.clone(),
            Some(Value::F32(f)) => f.to_string(),
            Some(Value::Bool(b)) => b.to_string(),
            Some(Value::List(_)) | Some(Value::Object(_)) | None => String::new(),
        }
    }
}

/// A type a [`Value`] can be read back as. Implemented for the three
/// primitive types [`PropSource`] is ever generic over.
pub trait FromValue: Sized {
    /// The shape a property of this type wants (what a typed binding converts to, DATA-2).
    const KIND: crate::format::ValueKind;
    fn from_value(v: &Value) -> Option<Self>;
}

impl FromValue for bool {
    const KIND: crate::format::ValueKind = crate::format::ValueKind::Bool;
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

impl FromValue for f32 {
    const KIND: crate::format::ValueKind = crate::format::ValueKind::Number;
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::F32(f) => Some(*f),
            _ => None,
        }
    }
}

impl FromValue for String {
    const KIND: crate::format::ValueKind = crate::format::ValueKind::Text;
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Str(s) => Some(s.clone()),
            _ => None,
        }
    }
}

/// The code-behind's state, read and written by path (§3: "a small `Bindable`
/// trait the code-behind's state struct implements"). A path is whatever the
/// view model chooses — a field name, as in every example in `XML_VIEWS.md`
/// §7 (`Notifications`, `SyncIntervalMin`, `Proxy`…) — this trait does not
/// interpret it.
pub trait ViewModel {
    fn get(&self, path: &str) -> Option<Value>;
    fn set(&mut self, path: &str, value: Value);

    /// What a binding reads for a property of shape `want` (DATA-2 typed conversions): the value
    /// at `spec.path`, converted and formatted per the binding's `FormatString`, `NullValue` and
    /// `Culture` ([`crate::format::to_target`]). `None` falls the property back to its default.
    /// Data components answer it themselves (`crate::scope::BindingProvider`); a view model
    /// rarely overrides it.
    fn get_bound(&self, spec: &BindingSpec, want: crate::format::ValueKind) -> Option<Value> {
        let value = self.get(&spec.path)?;
        crate::format::to_target(value, want, &spec.format)
    }

    /// What a two-way binding writes: `value` parsed back per the binding's format
    /// ([`crate::format::from_target`]), then [`ViewModel::set`].
    fn set_bound(&mut self, spec: &BindingSpec, value: Value) {
        let value = if spec.format.is_empty() { value } else { crate::format::from_target(value, &spec.format) };
        self.set(&spec.path, value);
    }

    /// Runs this view model's own typed handler named `handler`, if it has one
    /// (`vskubuno/docs/EVENTS.md` §5.4, EVT-4). Returns whether it had one. Tried by
    /// [`HandlerTable::dispatch_args`] before the table's own entries.
    ///
    /// The default has none. Code-behinds do not implement this by hand: they mark an
    /// `impl` with `#[kubuno_views::event_handlers]` and paint with
    /// [`crate::runtime::Runtime::frame_typed`], whose [`crate::events::TypedViewModel`]
    /// answers it with the generated [`crate::events::EventSink`].
    fn dispatch_event(
        &mut self,
        _handler: &str,
        _sender: &crate::events::ElementRef<'_>,
        _args: &mut dyn crate::events::EventArgs,
    ) -> bool {
        false
    }
}

/// A trivial [`ViewModel`] over a flat `HashMap<String, Value>` — enough for
/// the example view model and for this crate's own tests, without asking
/// every test to write a bespoke struct.
#[derive(Debug, Default, Clone)]
pub struct MapViewModel(pub HashMap<String, Value>);

impl MapViewModel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, path: impl Into<String>, value: Value) -> Self {
        self.0.insert(path.into(), value);
        self
    }
}

impl ViewModel for MapViewModel {
    fn get(&self, path: &str) -> Option<Value> {
        self.0.get(path).cloned()
    }

    fn set(&mut self, path: &str, value: Value) {
        self.0.insert(path.to_string(), value);
    }
}

// The `{Binding …}` grammar (modes, triggers, formats, parts, keys, issues) lives in the
// platform-neutral `kubuno-views-syntax` (`binding`, WV-1), re-exported here under its historical
// paths; this module adds what a binding does at run time.
pub use kubuno_views_syntax::binding::{
    binding_parts, canonical_key, is_binding_expr, BindingFormat, BindingIssue, BindingMode, BindingPart, BindingSyntax, UpdateSourceTrigger, BINDING_KEYS,
};

/// A parsed `{Binding Path[, Mode=…][, Converter=…]…}` attribute value.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BindingSpec {
    pub path: String,
    pub mode: BindingMode,
    /// `FormatString=`, `NullValue=`, `Culture=` (DATA-2, [`crate::format`]).
    pub format: BindingFormat,
    /// `Converter=Name`: a [`ValueConverter`] of the [`converter`] registry, applied to the source
    /// value before the format (and backwards on a write-back).
    pub converter: Option<String>,
    /// `ConverterParameter=…`, handed to the converter.
    pub converter_parameter: Option<String>,
    /// `FallbackValue=…`: what the property shows while the path does not resolve (instead of the
    /// property's default).
    pub fallback_value: Option<String>,
    /// `UpdateSourceTrigger=…`.
    pub update_trigger: UpdateSourceTrigger,
    /// What this binding remembers while the view runs (a `OneTime` value, the last value written).
    pub state: BindingState,
}

impl BindingSpec {
    /// A one-way binding of `path`, without formatting.
    pub fn of(path: impl Into<String>) -> Self {
        Self { path: path.into(), ..Self::default() }
    }

    /// Writes `value` back through `vm` (what a two-way property does on a user change):
    /// [`ViewModel::set_bound`], so the binding's format parses it back.
    pub fn write(&self, vm: &mut dyn ViewModel, value: Value) {
        vm.set_bound(self, value);
    }

    /// What a control does with a user change of a bound property whose mode
    /// [`writes_back`](BindingMode::writes_back): the value goes through the converter backwards,
    /// then to the source — now (`PropertyChanged`), when the focus leaves the element
    /// (`LostFocus`), or when the code calls [`update_sources`] (`Explicit`). Until then the
    /// property keeps showing the value the user entered.
    pub fn update_source(&self, vm: &mut dyn ViewModel, value: Value) {
        self.state.remember_written(&value);
        // Inside an item template the row's view model only lives while the item paints: a
        // deferred write would reach the wrong one, so it is written now.
        let deferred = self.update_trigger != UpdateSourceTrigger::PropertyChanged && current_item().is_none();
        if deferred {
            pending::defer(self, value);
            return;
        }
        self.push_to_source(vm, value);
    }

    /// The converter backwards, then the format, then the source.
    fn push_to_source(&self, vm: &mut dyn ViewModel, value: Value) {
        let Some(name) = self.converter.as_deref() else {
            vm.set_bound(self, value);
            return;
        };
        let Some(conv) = converter(name) else {
            tracing::warn!(converter = %name, path = %self.path, "unknown converter: the binding writes its value unconverted");
            vm.set_bound(self, value);
            return;
        };
        let value = if self.format.is_empty() { value } else { crate::format::from_target(value, &self.format) };
        if let Some(back) = conv.convert_back(value, self.converter_parameter.as_deref()) {
            let mut plain = self.clone();
            plain.format = BindingFormat::default();
            vm.set_bound(&plain, back);
        }
    }

    /// The value this binding gives a property of shape `want`: the source value through the
    /// converter and the format; `None` when the path does not resolve (or the converter declines).
    pub fn read(&self, vm: &dyn ViewModel, want: crate::format::ValueKind) -> Option<Value> {
        if let Some(value) = pending::value_of(self) {
            return crate::format::to_target(value, want, &BindingFormat::default());
        }
        if !self.mode.reads_source() {
            return self.state.last_written().and_then(|v| crate::format::to_target(v, want, &BindingFormat::default()));
        }
        if self.mode == BindingMode::OneTime {
            if let Some(v) = self.state.once() {
                return crate::format::to_target(v, want, &BindingFormat::default());
            }
        }
        let value = match self.converter.as_deref() {
            None => crate::resources::get_bound(vm, self, want),
            Some(name) => match converter(name) {
                Some(conv) => {
                    let raw = crate::resources::get(vm, self);
                    conv.convert(raw, self.converter_parameter.as_deref()).and_then(|v| crate::format::to_target(v, want, &self.format))
                }
                None => {
                    tracing::warn!(converter = %name, path = %self.path, "unknown converter: the binding shows its value unconverted");
                    crate::resources::get_bound(vm, self, want)
                }
            },
        };
        if self.mode == BindingMode::OneTime {
            if let Some(v) = &value {
                self.state.set_once(v.clone());
            }
        }
        value
    }

    /// `FallbackValue` in shape `want`, when the binding has one that converts.
    pub fn fallback(&self, want: crate::format::ValueKind) -> Option<Value> {
        let text = self.fallback_value.as_ref()?;
        crate::format::to_target(Value::Str(text.clone()), want, &BindingFormat::default())
    }
}

/// The run-time memory of one binding (shared by the clones of its [`BindingSpec`]): the value a
/// `OneTime` binding read, per item of a `<Repeater>` (its key), and the last value written back.
/// Never part of a binding's identity: two specs compare equal whatever their states hold.
#[derive(Clone, Default)]
pub struct BindingState(std::sync::Arc<std::sync::Mutex<StateInner>>);

#[derive(Default)]
struct StateInner {
    once: Vec<(Option<String>, Value)>,
    written: Vec<(Option<String>, Value)>,
}

impl BindingState {
    fn lock(&self) -> std::sync::MutexGuard<'_, StateInner> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn item_key() -> Option<String> {
        current_item().map(|i| i.key)
    }

    fn once(&self) -> Option<Value> {
        let key = Self::item_key();
        self.lock().once.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())
    }

    fn set_once(&self, value: Value) {
        let key = Self::item_key();
        let mut inner = self.lock();
        if !inner.once.iter().any(|(k, _)| *k == key) {
            inner.once.push((key, value));
        }
    }

    fn remember_written(&self, value: &Value) {
        let key = Self::item_key();
        let mut inner = self.lock();
        match inner.written.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value.clone(),
            None => inner.written.push((key, value.clone())),
        }
    }

    fn last_written(&self) -> Option<Value> {
        let key = Self::item_key();
        self.lock().written.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())
    }

    /// Forgets what the binding remembered (a `OneTime` binding reads its source again).
    pub fn reset(&self) {
        let mut inner = self.lock();
        inner.once.clear();
        inner.written.clear();
    }

    fn same(&self, other: &BindingState) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

impl PartialEq for BindingState {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for BindingState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BindingState")
    }
}

/// The write-backs waiting for their trigger (`LostFocus`, `Explicit`), per view runtime ("owner").
mod pending {
    use super::{BindingSpec, UpdateSourceTrigger, Value, ViewModel};
    use std::cell::{Cell, RefCell};

    struct Entry {
        owner: u64,
        spec: BindingSpec,
        value: Value,
    }

    thread_local! {
        static OWNER: Cell<u64> = const { Cell::new(0) };
        static PENDING: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn owner() -> u64 {
        OWNER.with(Cell::get)
    }

    pub(super) fn set_owner(owner: u64) -> u64 {
        OWNER.with(|o| o.replace(owner))
    }

    pub(super) fn defer(spec: &BindingSpec, value: Value) {
        let owner = owner();
        PENDING.with(|p| {
            let mut p = p.borrow_mut();
            match p.iter_mut().find(|e| e.owner == owner && e.spec.state.same(&spec.state)) {
                Some(e) => e.value = value,
                None => p.push(Entry { owner, spec: spec.clone(), value }),
            }
        });
    }

    pub(super) fn value_of(spec: &BindingSpec) -> Option<Value> {
        if spec.update_trigger == UpdateSourceTrigger::PropertyChanged {
            return None;
        }
        let owner = owner();
        PENDING.with(|p| p.borrow().iter().find(|e| e.owner == owner && e.spec.state.same(&spec.state)).map(|e| e.value.clone()))
    }

    /// Writes the pending values of the current owner whose trigger `which` accepts (and whose
    /// path `path` names, when given); returns how many were written.
    pub(super) fn commit(vm: &mut dyn ViewModel, which: impl Fn(UpdateSourceTrigger) -> bool, path: Option<&str>) -> usize {
        let owner = owner();
        let ready: Vec<Entry> = PENDING.with(|p| {
            let mut p = p.borrow_mut();
            let (ready, keep): (Vec<Entry>, Vec<Entry>) =
                std::mem::take(&mut *p).into_iter().partition(|e| e.owner == owner && which(e.spec.update_trigger) && path.is_none_or(|path| e.spec.path == path));
            *p = keep;
            ready
        });
        let n = ready.len();
        for e in ready {
            e.spec.push_to_source(vm, e.value);
        }
        n
    }
}

/// A new view-runtime identity for [`enter_binding_owner`] (each runtime takes one).
pub fn new_binding_owner() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Restores the previous binding owner when dropped (see [`enter_binding_owner`]).
pub struct BindingOwnerGuard(u64);

impl Drop for BindingOwnerGuard {
    fn drop(&mut self) {
        pending::set_owner(self.0);
    }
}

/// Marks the deferred write-backs made until the guard drops as `owner`'s (the runtime painting
/// or dispatching now), so [`commit_lost_focus`] and [`update_sources`] only write a view's own.
pub fn enter_binding_owner(owner: u64) -> BindingOwnerGuard {
    BindingOwnerGuard(pending::set_owner(owner))
}

/// Writes the `UpdateSourceTrigger=LostFocus` values of the current owner (the runtime calls it
/// when the focus moves); returns how many were written.
pub fn commit_lost_focus(vm: &mut dyn ViewModel) -> usize {
    pending::commit(vm, |t| t == UpdateSourceTrigger::LostFocus, None)
}

/// WPF's `BindingExpression.UpdateSource()`: writes the deferred values of the current view
/// (`UpdateSourceTrigger=Explicit`, and `LostFocus` ones not written yet) through `vm`; with
/// `path`, only the bindings of that path. Returns how many were written. Call it from a handler
/// (`kubuno_views::binding::update_sources(self, None)` before saving).
pub fn update_sources(vm: &mut dyn ViewModel, path: Option<&str>) -> usize {
    pending::commit(vm, |t| t != UpdateSourceTrigger::PropertyChanged, path)
}

/// Converts the value of a binding between its source and its property (WPF `IValueConverter`),
/// named in XML with `Converter=Name` (and `ConverterParameter=…`).
///
/// Register one with [`register_converter`], or mark its `impl` with
/// `#[kubuno_views::value_converter]` (the type must implement `Default`): it is then registered
/// before `main`, under the type's name or the one given (`#[value_converter("Initials")]`).
pub trait ValueConverter: Send + Sync {
    /// Source → property. `None`: no value (the property shows its fallback).
    fn convert(&self, value: Option<Value>, parameter: Option<&str>) -> Option<Value>;

    /// Property → source, for a write-back. `None` (the default) drops the write.
    fn convert_back(&self, _value: Value, _parameter: Option<&str>) -> Option<Value> {
        None
    }
}

type ConverterMap = std::sync::RwLock<Vec<(String, std::sync::Arc<dyn ValueConverter>)>>;

fn converters() -> &'static ConverterMap {
    static MAP: std::sync::OnceLock<ConverterMap> = std::sync::OnceLock::new();
    MAP.get_or_init(|| std::sync::RwLock::new(Vec::new()))
}

/// Registers `converter` under `name` (a later registration of the same name replaces it; a
/// project converter may replace a built-in one).
pub fn register_converter(name: &str, converter: impl ValueConverter + 'static) {
    let mut map = converters().write().unwrap_or_else(std::sync::PoisonError::into_inner);
    map.retain(|(n, _)| n != name);
    map.push((name.to_string(), std::sync::Arc::new(converter)));
}

/// The converter named `name`: a registered one, else a built-in one ([`BUILTIN_CONVERTERS`]).
pub fn converter(name: &str) -> Option<std::sync::Arc<dyn ValueConverter>> {
    let name = name.trim();
    if let Some(c) = converters().read().unwrap_or_else(std::sync::PoisonError::into_inner).iter().find(|(n, _)| n == name).map(|(_, c)| c.clone()) {
        return Some(c);
    }
    BUILTIN_CONVERTERS.iter().find(|b| b.name == name).map(|b| std::sync::Arc::new(Builtin(b.name)) as std::sync::Arc<dyn ValueConverter>)
}

/// The names of the registered (project) converters.
pub fn registered_converters() -> Vec<String> {
    converters().read().unwrap_or_else(std::sync::PoisonError::into_inner).iter().map(|(n, _)| n.clone()).collect()
}

/// A converter every view knows (the designer lists them with their documentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinConverter {
    pub name: &'static str,
    /// The shape it produces (`"Bool"`, `"Text"`, `"Number"`, `"Any"`).
    pub output: &'static str,
    /// Whether it converts back (a two-way binding through it writes).
    pub two_way: bool,
    pub doc: &'static str,
}

/// The built-in converters.
pub const BUILTIN_CONVERTERS: &[BuiltinConverter] = &[
    BuiltinConverter { name: "Not", output: "Bool", two_way: true, doc: "The opposite of a boolean (`Enabled=\"{Binding IsBusy, Converter=Not}\"`)." },
    BuiltinConverter { name: "IsEmpty", output: "Bool", two_way: false, doc: "True when the value is empty: an empty or blank text, an empty list, zero, no value." },
    BuiltinConverter { name: "IsNotEmpty", output: "Bool", two_way: false, doc: "True when the value is not empty (the opposite of IsEmpty)." },
    BuiltinConverter { name: "ToUpper", output: "Text", two_way: true, doc: "The text in capitals." },
    BuiltinConverter { name: "ToLower", output: "Text", two_way: true, doc: "The text in lower case." },
    BuiltinConverter { name: "Trim", output: "Text", two_way: true, doc: "The text without its leading and trailing spaces." },
    BuiltinConverter { name: "Equals", output: "Bool", two_way: true, doc: "True when the value equals ConverterParameter (a radio button per choice); writing true writes the parameter back." },
    BuiltinConverter { name: "NotEquals", output: "Bool", two_way: false, doc: "True when the value differs from ConverterParameter." },
    BuiltinConverter { name: "BoolToText", output: "Text", two_way: true, doc: "ConverterParameter='yes text|no text': the first text for true, the second for false." },
    BuiltinConverter { name: "Count", output: "Number", two_way: false, doc: "The number of rows of a list, of characters of a text." },
];

/// A built-in converter, by name.
struct Builtin(&'static str);

fn is_empty_value(v: &Option<Value>) -> bool {
    match v {
        None => true,
        Some(Value::Str(s)) => s.trim().is_empty(),
        Some(Value::F32(f)) => *f == 0.0,
        Some(Value::Bool(b)) => !b,
        Some(Value::List(l)) => l.is_empty(),
        Some(Value::Object(_)) => false,
    }
}

fn value_text(v: &Value) -> Option<String> {
    match v {
        Value::Str(s) => Some(s.clone()),
        Value::F32(f) => Some(f.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::List(_) | Value::Object(_) => None,
    }
}

fn value_bool(v: &Value) -> Option<bool> {
    crate::format::to_target(v.clone(), crate::format::ValueKind::Bool, &BindingFormat::default()).and_then(|v| bool::from_value(&v))
}

impl ValueConverter for Builtin {
    fn convert(&self, value: Option<Value>, parameter: Option<&str>) -> Option<Value> {
        let text = |f: fn(&str) -> String| value.as_ref().and_then(value_text).map(|t| Value::Str(f(&t)));
        match self.0 {
            "Not" => value.as_ref().and_then(value_bool).map(|b| Value::Bool(!b)),
            "IsEmpty" => Some(Value::Bool(is_empty_value(&value))),
            "IsNotEmpty" => Some(Value::Bool(!is_empty_value(&value))),
            "ToUpper" => text(|t| t.to_uppercase()),
            "ToLower" => text(|t| t.to_lowercase()),
            "Trim" => text(|t| t.trim().to_string()),
            "Equals" | "NotEquals" => {
                let same = value.as_ref().and_then(value_text).zip(parameter).is_some_and(|(v, p)| v == p.trim());
                Some(Value::Bool(same == (self.0 == "Equals")))
            }
            "BoolToText" => {
                let on = value.as_ref().and_then(value_bool)?;
                let (yes, no) = parameter.unwrap_or("true|false").split_once('|').unwrap_or((parameter.unwrap_or("true"), ""));
                Some(Value::Str(if on { yes } else { no }.to_string()))
            }
            "Count" => match value? {
                Value::List(l) => Some(Value::F32(l.len() as f32)),
                Value::Str(s) => Some(Value::F32(s.chars().count() as f32)),
                _ => None,
            },
            _ => value,
        }
    }

    fn convert_back(&self, value: Value, parameter: Option<&str>) -> Option<Value> {
        match self.0 {
            "Not" => value_bool(&value).map(|b| Value::Bool(!b)),
            "ToUpper" | "ToLower" | "Trim" => Some(value),
            // Only the choice that becomes true writes (a radio button that turns off writes nothing).
            "Equals" => value_bool(&value).filter(|on| *on).and(parameter).map(|p| Value::Str(p.trim().to_string())),
            "BoolToText" => {
                let text = value_text(&value)?;
                let (yes, _) = parameter.unwrap_or("true|false").split_once('|').unwrap_or((parameter.unwrap_or("true"), ""));
                Some(Value::Bool(text == yes))
            }
            _ => None,
        }
    }
}

/// Registers a converter type before `main` — what `#[kubuno_views::value_converter]` expands to,
/// usable by hand: `kubuno_views::register_value_converter!("Initials", Initials);` (`Initials:
/// ValueConverter + Default`).
#[macro_export]
macro_rules! register_value_converter {
    ($name:expr, $ty:ty) => {
        const _: () = {
            #[used]
            #[cfg_attr(windows, unsafe(link_section = ".CRT$XCU"))]
            #[cfg_attr(any(target_os = "linux", target_os = "android", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd"), unsafe(link_section = ".init_array"))]
            #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__DATA,__mod_init_func"))]
            static __KUBUNO_REGISTER_CONVERTER: extern "C" fn() = {
                extern "C" fn register() {
                    $crate::binding::register_converter($name, <$ty as ::core::default::Default>::default());
                }
                register
            };
        };
    };
}

/// [`parse_binding`] and the problems of the expression (see [`BindingIssue`]).
pub fn parse_binding_report(raw: &str) -> (Option<BindingSpec>, Vec<BindingIssue>) {
    (parse_binding(raw), kubuno_views_syntax::binding::binding_issues(raw))
}

/// Parses a `{Binding Path}` / `{Binding Path, Mode=TwoWay}` attribute value (or a `{Res key}`,
/// [`crate::resources`]) with the grammar of `kubuno-views-syntax`.
/// `None` for anything that is not the `Binding` shape at all — a caller that
/// already checked [`is_binding_expr`] treats `None` here as a malformed
/// binding, not "not a binding".
pub fn parse_binding(raw: &str) -> Option<BindingSpec> {
    kubuno_views_syntax::binding::parse_binding_syntax(raw).map(BindingSpec::from)
}

impl From<BindingSyntax> for BindingSpec {
    /// The parsed binding, with a fresh run-time state.
    fn from(b: BindingSyntax) -> Self {
        BindingSpec {
            path: b.path,
            mode: b.mode,
            format: b.format,
            converter: b.converter,
            converter_parameter: b.converter_parameter,
            fallback_value: b.fallback_value,
            update_trigger: b.update_trigger,
            state: BindingState::default(),
        }
    }
}

/// What a compiled node stores for one XML property: the literal value the
/// attribute carried, or a binding plus the fallback to fall back on.
#[derive(Debug, Clone)]
pub enum PropSource<T> {
    Literal(T),
    Bound { spec: BindingSpec, fallback: T },
}

impl<T: Clone + FromValue> PropSource<T> {
    /// The value for this frame: the literal, or the binding resolved
    /// against `vm` (the fallback when the path is unset or holds a value of
    /// the wrong shape).
    pub fn resolve(&self, vm: &dyn ViewModel) -> T {
        match self {
            PropSource::Literal(v) => v.clone(),
            // The binding's value; else its `FallbackValue`; else the property's default.
            PropSource::Bound { spec, fallback } => spec
                .read(vm, T::KIND)
                .as_ref()
                .and_then(T::from_value)
                .or_else(|| spec.fallback(T::KIND).as_ref().and_then(T::from_value))
                .unwrap_or_else(|| fallback.clone()),
        }
    }

    /// The binding this property carries, when it is bound at all — what a
    /// two-way write-back (`XML_VIEWS.md` §3) needs: the path, and whether
    /// `Mode=TwoWay` actually asked for a write-back.
    pub fn binding(&self) -> Option<&BindingSpec> {
        match self {
            PropSource::Literal(_) => None,
            PropSource::Bound { spec, .. } => Some(spec),
        }
    }
}

/// A named handler (`OnClick="save"`, §2): a boxed closure over the live
/// [`ViewModel`] and the value the interaction produced (the new checked
/// state for a toggle, the new text for a change…). `'static` because the
/// table is built once by the code-behind and handed to
/// [`crate::runtime::Runtime::frame`] every frame.
pub type Handler = Box<dyn FnMut(&mut dyn ViewModel, Value)>;

/// The handler table a view's code-behind builds with [`crate::handlers!`]
/// (or by hand) and hands to [`crate::runtime::Runtime::frame`] — §2's
/// "a `HashMap<&'static str, Box<dyn FnMut(&mut State, ViewEvent)>>`
/// populated in the view struct's constructor", narrowed to the [`Value`]
/// payload this crate actually carries.
#[derive(Default)]
pub struct HandlerTable {
    handlers: HashMap<String, Handler>,
    typed: HashMap<String, TypedHandler>,
}

/// A handler that receives the event's sender and its typed, mutable args
/// (`vskubuno/docs/EVENTS.md` §1/§5.4) instead of the legacy [`Value`]: it can read a
/// `MouseEventArgs`'s position, mark a `KeyEventArgs` handled or cancel a `Validating`
/// (downcast with `args.downcast_mut::<CancelEventArgs>()`). The typed `#[handlers]`
/// methods of EVT-4 are generated on top of this.
pub type TypedHandler = Box<dyn FnMut(&mut dyn ViewModel, &crate::events::ElementRef<'_>, &mut dyn crate::events::EventArgs)>;

impl HandlerTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, name: impl Into<String>, handler: Handler) {
        self.handlers.insert(name.into(), handler);
    }

    /// Registers a typed handler under `name`. When both kinds exist under one name,
    /// the typed one wins (§5.4: "dispatch tries the typed sink first, then the legacy
    /// table").
    pub fn insert_typed(&mut self, name: impl Into<String>, handler: TypedHandler) {
        self.typed.insert(name.into(), handler);
    }

    /// Runs the handler named `name` for a typed event: the view model's own typed
    /// handler ([`ViewModel::dispatch_event`], a `#[kubuno_views::event_handlers]` method)
    /// first, else this table's typed handler with `sender` and `args`, else its legacy
    /// handler with `args.legacy_value()` (exactly the value that event always produced).
    /// Returns whether one ran.
    pub fn dispatch_args(
        &mut self,
        name: &str,
        vm: &mut dyn ViewModel,
        sender: &crate::events::ElementRef<'_>,
        args: &mut dyn crate::events::EventArgs,
    ) -> bool {
        if vm.dispatch_event(name, sender, args) {
            return true;
        }
        if let Some(h) = self.typed.get_mut(name) {
            h(vm, sender, args);
            return true;
        }
        let value = args.legacy_value();
        self.dispatch(name, vm, value)
    }

    /// Runs the handler named `name`, if the table has one. Silent when it
    /// does not — an `OnClick`/`OnToggled` naming a handler nobody registered
    /// is a validator-time concern ([`crate::validate`] does not check this;
    /// see that module's doc for why), not a runtime panic.
    pub fn dispatch(&mut self, name: &str, vm: &mut dyn ViewModel, value: Value) -> bool {
        if let Some(h) = self.handlers.get_mut(name) {
            h(vm, value);
            true
        } else {
            false
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.handlers.contains_key(name) || self.typed.contains_key(name)
    }
}

/// Builds a [`HandlerTable`] declaratively — `XML_VIEWS.md` §7's
/// `kubuno_views::handlers! { "offline_toggled" => |s, on| { … } }`.
///
/// The closure's first parameter is `&mut dyn ViewModel` and the second is
/// the [`Value`] the interaction produced; a handler that only wants the view
/// model narrows the value itself (`Value::Bool(on) => …`).
///
/// The body is any expression: a block (`|vm, v| { … }`) or a call — the form
/// `kubuno/createHandler` (the designer's "create handler") writes for a handler it
/// generated as a `fn` next to the table (`"save" => |vm, value| save(vm, value),`).
#[macro_export]
macro_rules! handlers {
    ($($name:literal => |$vm:pat_param, $val:pat_param| $body:expr),* $(,)?) => {{
        #[allow(unused_mut)] // `handlers! {}` inserts nothing.
        let mut table = $crate::binding::HandlerTable::new();
        $(
            table.insert(
                $name,
                Box::new(move |$vm: &mut dyn $crate::binding::ViewModel, $val: $crate::binding::Value| $body)
                    as $crate::binding::Handler,
            );
        )*
        table
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The entry `kubuno/createHandler` writes (a call, not a block) compiles, next to block
    /// bodies, and an empty table does too (no `unused_mut` in the user's crate).
    #[test]
    fn handlers_accepts_a_call_body_as_the_designer_writes_it() {
        fn on_save(vm: &mut dyn ViewModel, value: Value) {
            vm.set("Saved", value);
        }
        let mut table = crate::handlers! {
            "block" => |vm, _v| { vm.set("Block", Value::Bool(true)); },
            "on_save" => |vm, value| on_save(vm, value),
        };
        let mut vm = MapViewModel::new();
        assert!(table.dispatch("on_save", &mut vm, Value::Str("x".into())));
        assert!(table.dispatch("block", &mut vm, Value::Bool(false)));
        assert_eq!(vm.get("Saved"), Some(Value::Str("x".into())));
        assert_eq!(vm.get("Block"), Some(Value::Bool(true)));
        let empty = crate::handlers! {};
        assert!(!empty.contains("x"));
    }

    #[test]
    fn parses_one_way_binding() {
        let spec = parse_binding("{Binding Title}").unwrap();
        assert_eq!(spec.path, "Title");
        assert_eq!(spec.mode, BindingMode::OneWay);
    }

    #[test]
    fn parses_two_way_binding_with_mode() {
        let spec = parse_binding("{Binding SyncIntervalMin, Mode=TwoWay}").unwrap();
        assert_eq!(spec.path, "SyncIntervalMin");
        assert_eq!(spec.mode, BindingMode::TwoWay);
    }

    #[test]
    fn parses_named_path_and_source() {
        let spec = parse_binding("{Binding Source=customers, Path=Name, Mode=TwoWay}").unwrap();
        assert_eq!(spec.path, "customers.Name");
        assert_eq!(spec.mode, BindingMode::TwoWay);
        assert_eq!(parse_binding("{Binding Source=customers}").unwrap().path, "customers");
        assert_eq!(parse_binding("{Binding Path=Title}").unwrap().path, "Title");
        assert_eq!(parse_binding("{Binding Path=Name, Source=errors}").unwrap().path, "errors.Name");
        assert!(parse_binding("{Binding Mode=TwoWay}").is_none());
    }

    #[test]
    fn parses_formatting_options() {
        let spec = parse_binding("{Binding Source=orders, Path=amount, FormatString='#,##0.00', NullValue='(none)', Culture=fr-FR, Mode=TwoWay}").unwrap();
        assert_eq!(spec.path, "orders.amount");
        assert_eq!(spec.mode, BindingMode::TwoWay);
        assert_eq!(spec.format.format_string.as_deref(), Some("#,##0.00"));
        assert_eq!(spec.format.null_value.as_deref(), Some("(none)"));
        assert_eq!(spec.format.culture.as_deref(), Some("fr-FR"));
        assert!(parse_binding("{Binding Total}").unwrap().format.is_empty());
        assert_eq!(parse_binding("{Binding Born, FormatString=d}").unwrap().format.format_string.as_deref(), Some("d"));
    }

    #[test]
    fn typed_reads_convert_between_shapes() {
        let vm = MapViewModel::new().with("Age", Value::Str("42".into())).with("Amount", Value::F32(1234.5));
        let age: PropSource<f32> = PropSource::Bound { spec: BindingSpec::of("Age"), fallback: 0.0 };
        assert_eq!(age.resolve(&vm), 42.0, "a text that parses reaches a numeric property");
        let spec = parse_binding("{Binding Amount, FormatString=N1, Culture=en-US}").unwrap();
        let amount: PropSource<String> = PropSource::Bound { spec, fallback: String::new() };
        assert_eq!(amount.resolve(&vm), "1,234.5");
        let mut vm = vm;
        parse_binding("{Binding Amount, FormatString=N2, Culture=fr-FR, Mode=TwoWay}").unwrap().write(&mut vm, Value::Str("2\u{202F}000,25".into()));
        assert_eq!(vm.get("Amount"), Some(Value::F32(2000.25)));
    }

    #[test]
    fn rejects_non_binding_braces() {
        assert!(parse_binding("{NotBinding Foo}").is_none());
        assert!(parse_binding("plain text").is_none());
        assert!(parse_binding("{Binding}").is_none());
        assert!(parse_binding("{Bindingx Foo}").is_none());
    }

    #[test]
    fn literal_prop_source_resolves_to_itself() {
        let vm = MapViewModel::new();
        let src: PropSource<bool> = PropSource::Literal(true);
        assert!(src.resolve(&vm));
        assert!(src.binding().is_none());
    }

    #[test]
    fn bound_prop_source_reads_the_view_model() {
        let vm = MapViewModel::new().with("On", Value::Bool(true));
        let src = PropSource::Bound {
            spec: BindingSpec { path: "On".to_string(), mode: BindingMode::TwoWay, ..Default::default() },
            fallback: false,
        };
        assert!(src.resolve(&vm));
        assert_eq!(src.binding().unwrap().mode, BindingMode::TwoWay);
    }

    #[test]
    fn bound_prop_source_falls_back_when_unset() {
        let vm = MapViewModel::new();
        let src = PropSource::Bound {
            spec: BindingSpec { path: "Missing".to_string(), mode: BindingMode::OneWay, ..Default::default() },
            fallback: 42.0_f32,
        };
        assert_eq!(src.resolve(&vm), 42.0);
    }

    #[test]
    fn row_reads_fields_by_name_and_renders_text() {
        let row = Row::new().with("Text", Value::Str("Alice".to_string())).with("Age", Value::F32(30.0)).with("Active", Value::Bool(true));
        assert_eq!(row.get("Text"), Some(&Value::Str("Alice".to_string())));
        assert_eq!(row.text("Text"), "Alice");
        assert_eq!(row.text("Age"), "30");
        assert_eq!(row.text("Active"), "true");
        assert_eq!(row.text("Missing"), "");
    }

    #[test]
    fn view_model_can_hand_back_a_list_of_rows() {
        let rows = vec![Row::new().with("Text", Value::Str("A".to_string())), Row::new().with("Text", Value::Str("B".to_string()))];
        let vm = MapViewModel::new().with("Users", Value::from(rows.clone()));
        assert_eq!(vm.get("Users"), Some(Value::from(rows)));
    }

    #[test]
    fn handler_table_dispatches_by_name() {
        let mut table = handlers! {
            "offline_toggled" => |vm, v| {
                if let Value::Bool(on) = v {
                    vm.set("Offline", Value::Bool(on));
                }
            },
        };
        let mut vm = MapViewModel::new();
        assert!(table.dispatch("offline_toggled", &mut vm, Value::Bool(true)));
        assert_eq!(vm.get("Offline"), Some(Value::Bool(true)));
        assert!(!table.dispatch("no_such_handler", &mut vm, Value::Bool(false)));
    }

    #[test]
    fn parses_the_wpf_keys() {
        let spec = parse_binding("{Binding Name, Mode=OneTime, UpdateSourceTrigger=LostFocus, Converter=ToUpper, ConverterParameter='a, b', FallbackValue='(none)'}").unwrap();
        assert_eq!(spec.path, "Name");
        assert_eq!(spec.mode, BindingMode::OneTime);
        assert_eq!(spec.update_trigger, UpdateSourceTrigger::LostFocus);
        assert_eq!(spec.converter.as_deref(), Some("ToUpper"));
        assert_eq!(spec.converter_parameter.as_deref(), Some("a, b"));
        assert_eq!(spec.fallback_value.as_deref(), Some("(none)"));
        assert_eq!(parse_binding("{Binding X, Mode=OneWayToSource}").unwrap().mode, BindingMode::OneWayToSource);
        assert_eq!(parse_binding("{Binding X, UpdateSourceTrigger=Default}").unwrap().update_trigger, UpdateSourceTrigger::PropertyChanged);
        assert!(BindingMode::TwoWay.writes_back() && BindingMode::OneWayToSource.writes_back() && !BindingMode::OneTime.writes_back());
    }

    #[test]
    fn parts_carry_their_ranges() {
        let raw = "{Binding Title, Mode=TwoWay}";
        let parts = binding_parts(raw).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(&raw[parts[0].value_range.clone()], "Title");
        assert_eq!(&raw[parts[1].key_range.clone().unwrap()], "Mode");
        assert_eq!(&raw[parts[1].value_range.clone()], "TwoWay");
        assert!(binding_parts("{Res title}").is_none());
        assert!(binding_parts("{BindingX}").is_none());
        assert_eq!(binding_parts("{Binding}").unwrap(), Vec::new());
    }

    #[test]
    fn the_report_names_unknown_keys_and_values() {
        let raw = "{Binding Title, Mod=TwoWay, Mode=Both, UpdateSourceTrigger=Never, Extra, Mode=OneWay}";
        let (spec, issues) = parse_binding_report(raw);
        assert_eq!(spec.unwrap().path, "Title");
        let texts: Vec<&str> = issues.iter().map(|i| &raw[i.range.clone()]).collect();
        assert_eq!(texts, ["Mod", "Both", "Never", "Extra", "Mode"]);
        assert!(issues[0].message.contains("did you mean `Mode`") || issues[0].message.contains("unknown binding key `Mod`"));
        assert!(parse_binding_report("{Binding Title, StringFormat='N2', TargetNullValue=-}").1.is_empty());
    }

    #[test]
    fn converters_apply_both_ways() {
        let vm = MapViewModel::new().with("Busy", Value::Bool(true)).with("Name", Value::Str("ada".into())).with("Items", Value::from(vec![Row::new(), Row::new()]));
        let enabled: PropSource<bool> = PropSource::Bound { spec: parse_binding("{Binding Busy, Converter=Not}").unwrap(), fallback: false };
        assert!(!enabled.resolve(&vm));
        let upper: PropSource<String> = PropSource::Bound { spec: parse_binding("{Binding Name, Converter=ToUpper}").unwrap(), fallback: String::new() };
        assert_eq!(upper.resolve(&vm), "ADA");
        let empty: PropSource<bool> = PropSource::Bound { spec: parse_binding("{Binding Missing, Converter=IsEmpty}").unwrap(), fallback: false };
        assert!(empty.resolve(&vm));
        let count: PropSource<String> = PropSource::Bound { spec: parse_binding("{Binding Items, Converter=Count, StringFormat=N0, Culture=en-US}").unwrap(), fallback: String::new() };
        assert_eq!(count.resolve(&vm), "2");
        let text: PropSource<String> = PropSource::Bound { spec: parse_binding("{Binding Busy, Converter=BoolToText, ConverterParameter='Occupé|Libre'}").unwrap(), fallback: String::new() };
        assert_eq!(text.resolve(&vm), "Occupé");
        let mut vm = vm;
        parse_binding("{Binding Busy, Mode=TwoWay, Converter=Not}").unwrap().update_source(&mut vm, Value::Bool(true));
        assert_eq!(vm.get("Busy"), Some(Value::Bool(false)));
        let choice = parse_binding("{Binding Theme, Mode=TwoWay, Converter=Equals, ConverterParameter=Dark}").unwrap();
        choice.update_source(&mut vm, Value::Bool(true));
        assert_eq!(vm.get("Theme"), Some(Value::Str("Dark".into())));
        choice.update_source(&mut vm, Value::Bool(false));
        assert_eq!(vm.get("Theme"), Some(Value::Str("Dark".into())), "a choice turning off writes nothing");
    }

    #[test]
    fn project_converters_register_by_name() {
        struct Initials;
        impl ValueConverter for Initials {
            fn convert(&self, value: Option<Value>, _: Option<&str>) -> Option<Value> {
                match value? {
                    Value::Str(s) => Some(Value::Str(s.split_whitespace().filter_map(|w| w.chars().next()).collect())),
                    _ => None,
                }
            }
        }
        register_converter("TestInitials", Initials);
        assert!(registered_converters().iter().any(|n| n == "TestInitials"));
        let vm = MapViewModel::new().with("Name", Value::Str("Ada Lovelace".into()));
        let p: PropSource<String> = PropSource::Bound { spec: parse_binding("{Binding Name, Converter=TestInitials}").unwrap(), fallback: String::new() };
        assert_eq!(p.resolve(&vm), "AL");
    }

    #[test]
    fn fallback_value_shows_while_the_path_does_not_resolve() {
        let vm = MapViewModel::new();
        let p: PropSource<String> = PropSource::Bound { spec: parse_binding("{Binding Missing, FallbackValue='(inconnu)'}").unwrap(), fallback: String::new() };
        assert_eq!(p.resolve(&vm), "(inconnu)");
        let n: PropSource<f32> = PropSource::Bound { spec: parse_binding("{Binding Missing, FallbackValue=12}").unwrap(), fallback: 0.0 };
        assert_eq!(n.resolve(&vm), 12.0);
    }

    #[test]
    fn one_time_reads_once_and_one_way_to_source_never() {
        let mut vm = MapViewModel::new().with("Title", Value::Str("first".into()));
        let once: PropSource<String> = PropSource::Bound { spec: parse_binding("{Binding Title, Mode=OneTime}").unwrap(), fallback: String::new() };
        assert_eq!(once.resolve(&vm), "first");
        vm.set("Title", Value::Str("second".into()));
        assert_eq!(once.resolve(&vm), "first");
        let to_source: PropSource<String> = PropSource::Bound { spec: parse_binding("{Binding Title, Mode=OneWayToSource}").unwrap(), fallback: "default".into() };
        assert_eq!(to_source.resolve(&vm), "default");
        to_source.binding().unwrap().update_source(&mut vm, Value::Str("typed".into()));
        assert_eq!(vm.get("Title"), Some(Value::Str("typed".into())));
        assert_eq!(to_source.resolve(&vm), "typed", "it keeps showing what was entered");
    }

    #[test]
    fn deferred_triggers_wait_for_their_moment() {
        let owner = new_binding_owner();
        let _guard = enter_binding_owner(owner);
        let mut vm = MapViewModel::new().with("Name", Value::Str("old".into()));
        let lost: PropSource<String> = PropSource::Bound { spec: parse_binding("{Binding Name, Mode=TwoWay, UpdateSourceTrigger=LostFocus}").unwrap(), fallback: String::new() };
        lost.binding().unwrap().update_source(&mut vm, Value::Str("new".into()));
        assert_eq!(vm.get("Name"), Some(Value::Str("old".into())));
        assert_eq!(lost.resolve(&vm), "new", "the property shows the pending value");
        assert_eq!(commit_lost_focus(&mut vm), 1);
        assert_eq!(vm.get("Name"), Some(Value::Str("new".into())));
        let explicit = parse_binding("{Binding Name, Mode=TwoWay, UpdateSourceTrigger=Explicit}").unwrap();
        explicit.update_source(&mut vm, Value::Str("later".into()));
        assert_eq!(commit_lost_focus(&mut vm), 0);
        {
            // Another view's pending writes are not this one's.
            let _other = enter_binding_owner(new_binding_owner());
            assert_eq!(update_sources(&mut vm, None), 0);
        }
        assert_eq!(update_sources(&mut vm, Some("Other")), 0);
        assert_eq!(update_sources(&mut vm, Some("Name")), 1);
        assert_eq!(vm.get("Name"), Some(Value::Str("later".into())));
    }
}
