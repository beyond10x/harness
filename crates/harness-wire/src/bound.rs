//! Declared size bounds.
//!
//! Every bound here is checked where the value crosses a boundary, not where it is convenient. A
//! value over a bound is a typed refusal; nothing is silently truncated, because a truncated tool
//! argument reads downstream exactly like an argument the model meant to send.

use serde_json::Value;

/// Largest JSON encoding of one tool call's arguments.
pub const MAX_TOOL_ARGUMENT_BYTES: usize = 64 * 1024;

/// Largest JSON encoding of one tool result.
pub const MAX_TOOL_RESULT_BYTES: usize = 256 * 1024;

/// Largest instruction text admitted onto a turn.
pub const MAX_INSTRUCTION_BYTES: usize = 256 * 1024;

/// Largest description admitted for one published tool.
pub const MAX_TOOL_DESCRIPTION_BYTES: usize = 16 * 1024;

/// Largest number of tools published on one turn.
pub const MAX_TOOLS: usize = 512;

/// Returns the byte length of `value`'s compact JSON encoding.
///
/// A value that cannot be encoded counts as over any bound, so an unencodable value refuses at the
/// same place an oversized one does rather than escaping the check.
pub fn encoded_len(value: &Value) -> Option<usize> {
    serde_json::to_vec(value).ok().map(|bytes| bytes.len())
}

/// Returns `true` when `value` does not fit within `limit` bytes of compact JSON.
pub fn exceeds(value: &Value, limit: usize) -> bool {
    encoded_len(value).is_none_or(|len| len > limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn small_values_fit() {
        assert!(!exceeds(
            &json!({"path": "README.md"}),
            MAX_TOOL_ARGUMENT_BYTES
        ));
    }

    #[test]
    fn oversized_values_refuse() {
        let big = json!({ "blob": "x".repeat(MAX_TOOL_ARGUMENT_BYTES) });
        assert!(exceeds(&big, MAX_TOOL_ARGUMENT_BYTES));
    }

    #[test]
    fn encoded_len_measures_compact_json() {
        assert_eq!(encoded_len(&json!({"a": 1})), Some(7));
    }
}
