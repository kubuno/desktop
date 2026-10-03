//! `kubuno/bindingPaths { uri, openFiles }` → `{ paths }`: the paths a view's data context answers
//! (its `#[bind]` fields, a user control's properties, the arms of a hand-written `impl ViewModel`
//! — see [`crate::binding_sources::data_context`]). Kept for older clients: the designer now asks
//! `kubuno/bindingSources`, which also carries types, locations, the item row of a template, the
//! data components, the resources and the converters.

use std::collections::HashMap;

use lsp_types::Uri;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingPathsParams {
    /// The `.kbview`.
    pub uri: Uri,
    #[serde(default)]
    pub open_files: HashMap<String, String>,
}

#[derive(Debug, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BindingPathsResult {
    /// The paths, in declaration order, without duplicates.
    pub paths: Vec<String>,
}

/// `kubuno/bindingPaths` (call inside [`crate::sources::with_overlays`]).
pub fn binding_paths(uri: &Uri) -> BindingPathsResult {
    let Some(view) = crate::fs_uri::to_path(uri) else { return BindingPathsResult::default() };
    let mut paths: Vec<String> = Vec::new();
    for m in crate::binding_sources::data_context(&view).members {
        if !paths.contains(&m.path) {
            paths.push(m.path);
        }
    }
    BindingPathsResult { paths }
}

/// The paths of `text`'s `impl ViewModel` (`None` when it has none).
pub fn paths_in(text: &str) -> Option<Vec<String>> {
    crate::binding_sources::legacy_paths(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_paths_are_the_patterns_of_the_get_match() {
        let text = r#"
use kubuno_views::prelude::*;
pub struct Vm { status: String, count: i32 }
impl ViewModel for Vm {
    fn get(&self, path: &str) -> Option<Value> {
        // "Commented" => nothing
        match path {
            "Status" => Some(Value::Str(self.status.clone())),
            "Count" | "Total" => Some(Value::F32(self.count as f32)),
            "Big" if self.count > 9 => Some(Value::Bool(true)),
            "Status" => None,
            _ => None,
        }
    }
    fn set(&mut self, path: &str, value: Value) {
        if let ("Status", Value::Str(s)) = (path, value) { self.status = s; }
    }
}
"#;
        assert_eq!(paths_in(text), Some(vec!["Status".to_string(), "Count".into(), "Total".into(), "Big".into()]));
    }

    #[test]
    fn a_file_without_a_view_model_has_no_paths() {
        assert_eq!(paths_in("fn main() {}"), None);
    }
}
