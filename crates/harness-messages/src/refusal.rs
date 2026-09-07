//! Provider-declared refusal diagnostics. Never changes terminal or retry semantics.

use serde_json::Value;

const MAX_CATEGORY_BYTES: usize = 128;
const MAX_EXPLANATION_BYTES: usize = 2048;

pub(super) fn message(event: &Value) -> Option<String> {
    let delta = event.get("delta")?;
    if delta.get("stop_reason").and_then(Value::as_str) != Some("refusal") {
        return None;
    }
    let details = delta.get("stop_details");
    let category = field(details, "category", MAX_CATEGORY_BYTES);
    let explanation = field(details, "explanation", MAX_EXPLANATION_BYTES);
    Some(format!(
        "Provider declined the response. Category: {category}. Explanation: {explanation}"
    ))
}

fn field(details: Option<&Value>, name: &str, maximum: usize) -> String {
    match details.and_then(|value| value.get(name)) {
        None | Some(Value::Null) => "not supplied".to_owned(),
        Some(Value::String(value)) if value.is_empty() => "not supplied".to_owned(),
        Some(Value::String(value)) if value.len() > maximum => {
            format!("omitted: exceeds {maximum} byte diagnostic bound")
        }
        Some(Value::String(value))
            if !value
                .chars()
                .any(|ch| ch.is_control() && ch != '\n' && ch != '\t') =>
        {
            value.clone()
        }
        Some(_) => "omitted: invalid diagnostic field".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn absent_details_remain_absent_and_success_never_warns() {
        let refused = json!({"delta":{"stop_reason":"refusal"}});
        assert_eq!(
            message(&refused).unwrap(),
            "Provider declined the response. Category: not supplied. Explanation: not supplied"
        );
        assert!(
            message(
                &json!({"delta":{"stop_reason":"end_turn", "stop_details":{"category":"cyber"}}})
            )
            .is_none()
        );
    }

    #[test]
    fn diagnostics_are_bounded_without_silent_truncation_or_terminal_controls() {
        let refused = json!({"delta":{"stop_reason":"refusal", "stop_details":{
            "category":"x".repeat(MAX_CATEGORY_BYTES+1),
            "explanation":"untrusted\u{1b}[2J",
            "fallback_credit":"synthetic-sensitive-metadata"
        }}});
        let result = message(&refused).unwrap();
        assert!(result.contains("exceeds 128 byte diagnostic bound"));
        assert!(result.contains("invalid diagnostic field"));
        assert!(!result.contains("synthetic-sensitive-metadata"));
        assert!(!result.contains('\u{1b}'));
    }
}
