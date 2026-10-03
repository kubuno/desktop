//! `<ErrorProvider>` (`vskubuno/docs/DATA.md` §6, WinForms `ErrorProvider`): the field errors of the
//! current row of a binding source (conversion, `NOT NULL`, length, `RowValidating`), the binding
//! source's last `DataError` (a failed fill or save), and errors set from code.
//!
//! - **The glyph** (DATA-2): the view runtime draws the WinForms error icon, adapted to Kubuno (a
//!   round danger-coloured badge with an exclamation mark), next to every control bound to a field
//!   in error, with the message as its tooltip — `IconAlignment`, `IconPadding`, `BlinkStyle` and
//!   `BlinkRate` as in WinForms. No code: the provider answers
//!   [`BindingProvider::field_error`] for the paths of its `DataSource`.
//! - **Bindings**: `{Binding Source=errors, Path=Email}` (the message), `Email.HasError` (for a
//!   text field's `Invalid`), `HasErrors`, `Summary`.

use std::collections::BTreeMap;

use kubuno_views::binding::{BindingFormat, Value};
use kubuno_views::format::ValueKind;
use kubuno_views::prelude::*;
use kubuno_views::scope::{BindingProvider, ComponentScope, ErrorBlinkStyle, ErrorIconAlignment, FieldError};

use crate::binding_source::BindingSource;

/// `<ErrorProvider>` (see the module doc).
#[derive(Component)]
#[kubuno(extends = Component, overrides(Component))]
#[toolbox(icon = "alert", category = "Data")]
#[default_property("DataSource")]
pub struct ErrorProvider {
    base: ComponentCore,
    /// The x:Name of the BindingSource whose current row's errors are shown.
    #[property]
    #[category("Data")]
    pub data_source: String,
    /// When the error icon blinks: BlinkIfDifferentError (when it appears or its message changes), AlwaysBlink, NeverBlink.
    #[property]
    #[category("Appearance")]
    #[default_value("BlinkIfDifferentError")]
    pub blink_style: ErrorBlinkStyle,
    /// How fast the error icon blinks, in milliseconds.
    #[property]
    #[category("Appearance")]
    #[default_value(250)]
    pub blink_rate: u32,
    /// Where the error icon is placed next to its control.
    #[property]
    #[category("Appearance")]
    #[default_value("MiddleRight")]
    pub icon_alignment: ErrorIconAlignment,
    /// The space between the control and its error icon, in pixels.
    #[property]
    #[category("Appearance")]
    #[default_value(4)]
    pub icon_padding: f32,
    /// Errors set from code, by field (they stay until cleared).
    manual: BTreeMap<String, String>,
}

impl Default for ErrorProvider {
    fn default() -> Self {
        Self {
            base: ComponentCore::default(),
            data_source: String::new(),
            blink_style: ErrorBlinkStyle::BlinkIfDifferentError,
            blink_rate: 250,
            icon_alignment: ErrorIconAlignment::MiddleRight,
            icon_padding: 4.0,
            manual: BTreeMap::new(),
        }
    }
}

impl Component for ErrorProvider {
    fn as_binding_provider(&self) -> Option<&dyn BindingProvider> {
        Some(self)
    }
    fn as_binding_provider_mut(&mut self) -> Option<&mut dyn BindingProvider> {
        Some(self)
    }
}

/// The paths of a binding source that are not fields.
const SOURCE_PATHS: &[&str] = &[
    "",
    "List",
    "Position",
    "Count",
    "PositionText",
    "HasChanges",
    "IsEditing",
    "CanMovePrevious",
    "CanMoveNext",
    "HasCurrent",
    "PageIndex",
    "PageCount",
    "PageText",
    "TotalCount",
    "CanPreviousPage",
    "CanNextPage",
    "IsBusy",
    "RowsRead",
];

impl ErrorProvider {
    pub fn new(data_source: impl Into<String>) -> Self {
        Self { data_source: data_source.into(), ..Self::default() }
    }

    pub fn name(&self) -> &str {
        crate::events::name_of(&self.base)
    }

    /// Sets the error of `field` (an empty message clears it), WinForms `SetError`.
    pub fn set_error(&mut self, field: &str, message: &str) {
        if message.is_empty() {
            self.manual.remove(field);
        } else {
            self.manual.insert(field.to_string(), message.to_string());
        }
    }

    /// Clears the errors set from code.
    pub fn clear(&mut self) {
        self.manual.clear();
    }

    /// The error of `field`: set from code, else the current row's column error.
    pub fn error(&self, field: &str, source: Option<&BindingSource>) -> Option<String> {
        if let Some(m) = self.manual.iter().find(|(k, _)| k.eq_ignore_ascii_case(field)).map(|(_, v)| v.clone()) {
            return Some(m);
        }
        source.and_then(|s| s.column_error(field)).map(str::to_string)
    }

    /// Every error: the code's, the current row's, then the binding source's last failure (when it
    /// is not already one of them).
    pub fn errors(&self, source: Option<&BindingSource>) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self.manual.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        if let Some(s) = source {
            for (field, message) in s.current_errors() {
                if !out.iter().any(|(f, _)| f.eq_ignore_ascii_case(&field) && !field.is_empty()) {
                    out.push((field, message));
                }
            }
            if let Some((field, message)) = s.last_error() {
                let already = out.iter().any(|(_, m)| message.contains(m.as_str()));
                if !already {
                    out.push((field.clone(), message.clone()));
                }
            }
        }
        out
    }

    /// What a binding path below this provider reads: `HasErrors`, `Summary`, `<Field>` (its
    /// message, `""` when none) or `<Field>.HasError`.
    pub fn get_path(&self, path: &str, source: Option<&BindingSource>) -> Option<Value> {
        match path {
            "HasErrors" => Some(Value::Bool(!self.errors(source).is_empty())),
            "Summary" => Some(Value::Str(crate::error::Joined(&self.errors(source)).to_string())),
            _ => match path.strip_suffix(".HasError") {
                Some(field) => Some(Value::Bool(self.error(field, source).is_some())),
                None => Some(Value::Str(self.error(path, source).unwrap_or_default())),
            },
        }
    }

    /// The error shown next to a control bound to `path` (`customers.Email`): the field's, when
    /// the path is a field of this provider's data source.
    pub fn error_for_path(&self, path: &str, source: Option<&BindingSource>) -> Option<String> {
        let (source_name, rest) = path.split_once('.')?;
        if source_name != self.data_source.trim() {
            return None;
        }
        let field = rest.strip_prefix("Current.").unwrap_or(rest);
        if SOURCE_PATHS.contains(&field) || field.contains('.') {
            return None;
        }
        self.error(field, source)
    }
}

impl BindingProvider for ErrorProvider {
    fn binding_get(&self, path: &str, want: ValueKind, format: &BindingFormat, scope: &ComponentScope) -> Option<Value> {
        let value = scope.with_ref::<BindingSource, _>(self.data_source.trim(), |bs| self.get_path(path, Some(bs))).unwrap_or_else(|| self.get_path(path, None))?;
        kubuno_views::format::to_target(value, want, format)
    }

    fn binding_set(&mut self, _path: &str, _value: Value, _format: &BindingFormat, _scope: &ComponentScope) -> bool {
        // Read-only: the errors come from the data and from code.
        true
    }

    fn field_error(&self, path: &str, scope: &ComponentScope) -> Option<FieldError> {
        let message = scope.with_ref::<BindingSource, _>(self.data_source.trim(), |bs| self.error_for_path(path, Some(bs))).unwrap_or_else(|| self.error_for_path(path, None))?;
        Some(FieldError { message, alignment: self.icon_alignment, padding: self.icon_padding, blink: self.blink_style, blink_rate: self.blink_rate })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{DataColumn, Table};
    use crate::value::{DbKind, DbType, DbValue};

    #[test]
    fn shows_row_errors_code_errors_and_failures() {
        let mut t = Table::new("t", vec![DataColumn::new("age", DbType::new(DbKind::Int, "INTEGER"))]);
        t.load_row(vec![DbValue::Int(1)]);
        let mut bs = BindingSource::new();
        bs.load(t);
        let mut ep = ErrorProvider::new("bs");
        assert_eq!(ep.get_path("HasErrors", Some(&bs)), Some(Value::Bool(false)));
        bs.set_path("age", &Value::Str("x".into())).expect("kept");
        assert_eq!(ep.get_path("age", Some(&bs)), Some(Value::Str("Enter a whole number.".into())));
        assert_eq!(ep.get_path("age.HasError", Some(&bs)), Some(Value::Bool(true)));
        assert_eq!(ep.error_for_path("bs.age", Some(&bs)).as_deref(), Some("Enter a whole number."));
        assert_eq!(ep.error_for_path("bs.Current.age", Some(&bs)).as_deref(), Some("Enter a whole number."));
        assert_eq!(ep.error_for_path("other.age", Some(&bs)), None, "another source's field");
        assert_eq!(ep.error_for_path("bs.Position", Some(&bs)), None);
        assert_eq!(ep.error_for_path("errors.age.HasError", Some(&bs)), None);
        ep.set_error("email", "Check the address.");
        assert_eq!(ep.get_path("Summary", Some(&bs)), Some(Value::Str("email: Check the address.; age: Enter a whole number.".into())));
        bs.cancel_edit();
        ep.clear();
        bs.report_error("", "database error: connection refused");
        assert_eq!(ep.get_path("Summary", Some(&bs)), Some(Value::Str("database error: connection refused".into())));
        assert_eq!(ep.get_path("age", None), Some(Value::Str(String::new())));
    }
}
