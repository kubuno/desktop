//! Reading the `params` object of a request.

use serde_json::Value;

use crate::error::{ToolError, ToolResult};
use crate::targets::Target;

/// A required, non-blank string.
pub fn req_str<'a>(p: &'a Value, name: &str) -> ToolResult<&'a str> {
    match p.get(name).and_then(Value::as_str) {
        Some(s) if !s.trim().is_empty() => Ok(s),
        _ => Err(ToolError::validation(format!("`{name}` is required"))),
    }
}

/// An optional string (`None` when absent, null or blank).
pub fn opt_str<'a>(p: &'a Value, name: &str) -> Option<&'a str> {
    p.get(name).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

/// An optional string that may be empty (`""` is meaningful: the default schema).
pub fn str_or_empty<'a>(p: &'a Value, name: &str) -> &'a str {
    p.get(name).and_then(Value::as_str).map(str::trim).unwrap_or("")
}

pub fn opt_bool(p: &Value, name: &str, default: bool) -> ToolResult<bool> {
    match p.get(name) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(ToolError::validation(format!("`{name}` must be true or false"))),
    }
}

/// An optional integer in `min..=max` (out of range is refused, not clamped).
pub fn opt_int(p: &Value, name: &str, default: u64, min: u64, max: u64) -> ToolResult<u64> {
    match p.get(name) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => match v.as_u64() {
            Some(n) if (min..=max).contains(&n) => Ok(n),
            _ => Err(ToolError::validation(format!("`{name}` must be an integer from {min} to {max}"))),
        },
    }
}

/// The `target` parameter.
pub fn target(p: &Value) -> ToolResult<Target> {
    Target::from_json(p.get("target").ok_or_else(|| ToolError::validation("`target` is required"))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn readers_validate() {
        let p = json!({"a": "x", "b": "  ", "n": 5, "big": 999999999, "t": true, "neg": -1});
        assert_eq!(req_str(&p, "a").expect("a"), "x");
        assert!(req_str(&p, "b").is_err() && req_str(&p, "zz").is_err());
        assert_eq!(opt_str(&p, "b"), None);
        assert_eq!(opt_int(&p, "n", 1, 1, 10).expect("n"), 5);
        assert_eq!(opt_int(&p, "zz", 7, 1, 10).expect("default"), 7);
        assert!(opt_int(&p, "big", 1, 1, 10).is_err() && opt_int(&p, "neg", 1, 1, 10).is_err());
        assert!(opt_bool(&p, "t", false).expect("t"));
        assert!(opt_bool(&p, "a", false).is_err());
        assert!(target(&p).is_err());
    }
}
