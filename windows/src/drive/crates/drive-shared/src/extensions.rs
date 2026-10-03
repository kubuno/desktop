//! Port of `Files.Shared/Extensions`.
//!
//! Only the extensions that carry real behavior are ported; most LINQ-style
//! helpers (`ForEach`, `IsEmpty`, `ExceptBy`, …) map directly onto Rust
//! iterator adapters and are intentionally omitted.

use std::future::Future;
use std::time::Duration;

/// Port of `StringExtensions.Left`.
pub fn left(s: &str, length: usize) -> &str {
    let end = s
        .char_indices()
        .nth(length)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    &s[..end]
}

/// Port of `StringExtensions.Right`.
pub fn right(s: &str, length: usize) -> &str {
    let count = s.chars().count();
    if length >= count {
        return s;
    }
    let start = s
        .char_indices()
        .nth(count - length)
        .map(|(i, _)| i)
        .unwrap_or(0);
    &s[start..]
}

/// Port of `LinqExtensions.AddSorted` — inserts while keeping the list sorted.
pub fn add_sorted<T: Ord>(list: &mut Vec<T>, item: T) {
    let index = list.binary_search(&item).unwrap_or_else(|i| i);
    list.insert(index, item);
}

/// Port of `TaskExtensions.WithTimeoutAsync` — resolves to `default_value`
/// when the future does not complete in time.
pub async fn with_timeout<T, F>(future: F, timeout: Duration, default_value: T) -> T
where
    F: Future<Output = T>,
{
    tokio::time::timeout(timeout, future)
        .await
        .unwrap_or(default_value)
}

/// Port of `SafetyExtensions.IgnoreExceptions` — runs a fallible operation,
/// logging and swallowing the error.
pub fn ignore_errors<T, E: std::fmt::Display>(
    result: Result<T, E>,
    context: &str,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(e) => {
            tracing::info!("{context}: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_left_right() {
        assert_eq!(left("hello", 3), "hel");
        assert_eq!(left("hi", 5), "hi");
        assert_eq!(right("hello", 3), "llo");
        assert_eq!(right("hi", 5), "hi");
    }

    #[test]
    fn add_sorted_keeps_order() {
        let mut v = vec![1, 3, 5];
        add_sorted(&mut v, 4);
        add_sorted(&mut v, 0);
        assert_eq!(v, vec![0, 1, 3, 4, 5]);
    }

    #[tokio::test]
    async fn timeout_returns_default() {
        let slow = async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            1
        };
        assert_eq!(with_timeout(slow, Duration::from_millis(10), -1).await, -1);
        assert_eq!(with_timeout(async { 2 }, Duration::from_secs(1), -1).await, 2);
    }
}
