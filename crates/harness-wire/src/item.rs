use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::id::{CallId, ToolName, WireId};

/// One tool call the model asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    pub call_id: CallId,
    pub name: ToolName,
    pub arguments: Value,
}

/// What a tool port produced for one call.
///
/// A failed call is still an outcome the model must see. Hiding a tool failure teaches the model
/// the effect happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolOutcome {
    pub output: Value,
    pub failed: bool,
}

impl ToolOutcome {
    pub fn ok(output: Value) -> Self {
        Self {
            output,
            failed: false,
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            output: Value::String(message.into()),
            failed: true,
        }
    }
}

/// One entry of the conversation the loop replays on every turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Item {
    UserText {
        text: String,
    },
    AssistantText {
        text: String,
    },
    ToolCall(ToolCall),
    ToolResult {
        call_id: CallId,
        output: Value,
        failed: bool,
    },
    /// A provider item this crate deliberately does not understand.
    ///
    /// Reasoning items are the reason it exists. Under stateless turns the provider keeps nothing,
    /// so dropping them costs the model its own chain of thought across a tool round trip and it
    /// re-derives the plan every call. Carrying them verbatim is what makes a stateless loop as
    /// capable as a provider-threaded one. The payload is never read, and `wire` is what stops it
    /// being handed to a provider that never produced it.
    Opaque {
        wire: WireId,
        payload: Value,
    },
}

impl Item {
    pub fn user(text: impl Into<String>) -> Self {
        Self::UserText { text: text.into() }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self::AssistantText { text: text.into() }
    }

    pub fn result(call_id: CallId, outcome: ToolOutcome) -> Self {
        Self::ToolResult {
            call_id,
            output: outcome.output,
            failed: outcome.failed,
        }
    }

    /// Returns the wire that produced this item, when it is one no wire may reinterpret.
    pub fn opaque_wire(&self) -> Option<&WireId> {
        match self {
            Self::Opaque { wire, .. } => Some(wire),
            _ => None,
        }
    }

    pub fn as_tool_call(&self) -> Option<&ToolCall> {
        match self {
            Self::ToolCall(call) => Some(call),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call_id() -> CallId {
        CallId::new("call-1").expect("literal is a valid call id")
    }

    #[test]
    fn items_round_trip_without_vendor_fields() {
        let items = vec![
            Item::user("hello"),
            Item::assistant("hi"),
            Item::ToolCall(ToolCall {
                call_id: call_id(),
                name: ToolName::new("workspace.read").expect("valid"),
                arguments: json!({"path": "README.md"}),
            }),
            Item::result(call_id(), ToolOutcome::ok(json!({"bytes": 12}))),
            Item::Opaque {
                wire: WireId::new("openai-responses").expect("valid"),
                payload: json!({"type": "reasoning"}),
            },
        ];
        let encoded = serde_json::to_value(&items).expect("items serialize");
        let decoded: Vec<Item> = serde_json::from_value(encoded).expect("items deserialize");
        assert_eq!(decoded, items);
    }

    #[test]
    fn failed_outcome_keeps_the_failure_visible() {
        let outcome = ToolOutcome::failed("no such path");
        assert!(outcome.failed);
        let Item::ToolResult { failed, .. } = Item::result(call_id(), outcome) else {
            panic!("result builds a tool result item");
        };
        assert!(failed);
    }

    #[test]
    fn opaque_items_report_their_wire() {
        let wire = WireId::new("anthropic-messages").expect("valid");
        let item = Item::Opaque {
            wire: wire.clone(),
            payload: json!({}),
        };
        assert_eq!(item.opaque_wire(), Some(&wire));
        assert_eq!(Item::user("x").opaque_wire(), None);
    }
}
