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

/// A call this run refused **by its own rule**, named rather than left to be read out of a sentence.
///
/// # The silence this closes
///
/// A refusal is an outcome and not an error (`AGENTS.md` invariant 9), so the model is told in
/// words and the run keeps turning. That is right for the model and, on its own, wrong for
/// everybody else: on the record the refusal is `ToolCompleted { failed: true }` — the same shape
/// as a compile error, a missing file, a program that exited 1 — and its only distinguishing mark
/// is the sentence's text. An evaluation asking *did the surface refuse what is outside it?* had to
/// grep that sentence or answer `0 refusal(s)` for a run where the refusal plainly happened, and
/// matching prose is a second description of the decision that drifts from the first.
///
/// So the decision is carried as a value beside the sentence. The sentence is unchanged — it is
/// still what the model reads — and [`Refusal::message`] is the one place it is written, so the
/// typed fact and the prose cannot disagree.
///
/// Sibling of `harness_substrate::Withheld`, and the two answer different questions: `Withheld` is
/// a tool this machine would never admit, decided before publication; this is a call the published
/// tool refused, decided when it arrived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Refusal {
    /// The `run` tool was asked for a program outside the set this run declared.
    ///
    /// `declared` is the set as the provider holds it, in its own order, so a reader sees what the
    /// run could have started and not only what it could not.
    ProgramNotDeclared {
        program: String,
        declared: Vec<String>,
    },
}

impl Refusal {
    /// The `LoopEvent::Warning` code this refusal is reported under.
    ///
    /// A constant on the refusal rather than a literal at the emitting site: the code is part of
    /// what the refusal *is*, and a second spelling of it in the loop would be a second thing to
    /// keep in step with this one.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::ProgramNotDeclared { .. } => "program-refused",
        }
    }

    /// The sentence the model reads, and the one the record carries.
    ///
    /// **Written here and nowhere else.** Both providers refuse with it and the loop warns with it,
    /// so the text on the wire, the text in the conversation and the text in the record are one
    /// string with one author.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::ProgramNotDeclared { program, declared } => format!(
                "`{program}` is not a program this run may start. Declared: {}.",
                if declared.is_empty() {
                    "none".to_owned()
                } else {
                    declared.join(", ")
                }
            ),
        }
    }
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
    /// Which rule of this run's own refused the call, when one did.
    ///
    /// [`None`] on every ordinary failure — a tool that ran and did not work is not a refusal, and
    /// saying so would make *the run would not do this* unreadable again by making it the common
    /// case. Skipped when absent, so a record written before this field existed and one written
    /// after are byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<Refusal>,
}

impl ToolOutcome {
    pub fn ok(output: Value) -> Self {
        Self {
            output,
            failed: false,
            refusal: None,
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            output: Value::String(message.into()),
            failed: true,
            refusal: None,
        }
    }

    /// A failure the run made by rule, with the rule named.
    ///
    /// Still a failure and still a sentence: the model reads exactly what it read before this type
    /// existed, and the effect still did not happen.
    #[must_use]
    pub fn refused(refusal: Refusal) -> Self {
        Self {
            output: Value::String(refusal.message()),
            failed: true,
            refusal: Some(refusal),
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
                name: ToolName::new("workspace_read").expect("valid"),
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
