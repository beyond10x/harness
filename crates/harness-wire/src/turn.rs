use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bound::{
    MAX_INSTRUCTION_BYTES, MAX_TOOL_ARGUMENT_BYTES, MAX_TOOL_DESCRIPTION_BYTES,
    MAX_TOOL_RESULT_BYTES, MAX_TOOLS, exceeds,
};
use crate::id::{ToolName, WireId};
use crate::item::Item;
use crate::{WireError, WireErrorCode};

/// Whether a person must decide before this tool runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Approval {
    NotRequired,
    Required,
}

/// One tool published to the model for one turn.
///
/// Publishing is what makes a tool callable. The loop refuses a call naming anything absent from
/// this list, so the published set is the complete authority of a turn — there is no second place
/// to look.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSpec {
    pub name: ToolName,
    pub description: String,
    pub input_schema: Value,
    /// Whether a person is asked before this tool runs.
    ///
    /// **Being retired in favour of [`ToolSpec::envelope`].** A tool that can assert its own
    /// `NotRequired` can opt out of the safety boundary, which is the one thing a boundary must not
    /// offer; `Envelope::needs_approval` derives the same answer from what the tool *does* and a
    /// ceiling the caller sets. It remains here while the ports that set it are migrated, and a
    /// tool that sets neither is described as the safest thing it could be.
    pub approval: Approval,
    /// What this tool does, how much a wrong call costs, and what it must reach.
    ///
    /// Defaults to a pure, cheap, repeatable read, which is what every tool this harness shipped
    /// before the envelope existed actually is.
    #[serde(default)]
    pub envelope: crate::Envelope,
}

/// How the model is asked to sample, when a caller asks at all.
///
/// Every field is optional and an absent field is sent as nothing, not as a default. A provider's
/// own default is a decision it is entitled to make and to change; substituting a number here
/// would silently pin a behaviour nobody chose, and would do it invisibly, because a request
/// carrying `temperature: 1.0` looks exactly like one where somebody asked for it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sampling {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

impl Sampling {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.temperature.is_none() && self.top_p.is_none() && self.reasoning_effort.is_none()
    }

    /// # Errors
    ///
    /// Returns [`WireErrorCode::Protocol`] for a value outside the range its field admits. The
    /// check happens here rather than at the provider because a rejected request costs a round
    /// trip and arrives back as a vendor error string nobody can act on.
    pub fn validate(&self) -> Result<(), WireError> {
        if let Some(value) = self.temperature
            && !(0.0..=2.0).contains(&value)
        {
            return Err(WireError::protocol(format!(
                "temperature {value} is outside 0.0..=2.0"
            )));
        }
        if let Some(value) = self.top_p
            && !(value > 0.0 && value <= 1.0)
        {
            return Err(WireError::protocol(format!(
                "top_p {value} is outside 0.0 (exclusive)..=1.0"
            )));
        }
        if self.reasoning_effort.as_ref().is_some_and(String::is_empty) {
            return Err(WireError::protocol(
                "reasoning effort was named but left empty",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnRequest {
    pub model: String,
    pub instructions: String,
    pub items: Vec<Item>,
    pub tools: Vec<ToolSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Sampling::is_empty")]
    pub sampling: Sampling,
}

impl TurnRequest {
    /// Checks every bound this crate declares, before a byte reaches a provider.
    ///
    /// # Errors
    ///
    /// Returns [`WireErrorCode::TooLarge`] for an oversized field and
    /// [`WireErrorCode::Protocol`] for a duplicate tool name.
    pub fn validate(&self) -> Result<(), WireError> {
        if self.model.is_empty() {
            return Err(WireError::protocol("a turn must name a model"));
        }
        self.sampling.validate()?;
        if self.instructions.len() > MAX_INSTRUCTION_BYTES {
            return Err(WireError::too_large(format!(
                "instructions are {} bytes, over the {MAX_INSTRUCTION_BYTES} byte bound",
                self.instructions.len()
            )));
        }
        if self.tools.len() > MAX_TOOLS {
            return Err(WireError::too_large(format!(
                "{} tools published, over the {MAX_TOOLS} bound",
                self.tools.len()
            )));
        }
        let mut seen = BTreeSet::new();
        for tool in &self.tools {
            if !seen.insert(tool.name.clone()) {
                return Err(WireError::protocol(format!(
                    "tool `{}` is published twice; the model could not address either",
                    tool.name
                )));
            }
            if tool.description.len() > MAX_TOOL_DESCRIPTION_BYTES {
                return Err(WireError::too_large(format!(
                    "tool `{}` description is over the {MAX_TOOL_DESCRIPTION_BYTES} byte bound",
                    tool.name
                )));
            }
        }
        for item in &self.items {
            match item {
                Item::ToolCall(call) if exceeds(&call.arguments, MAX_TOOL_ARGUMENT_BYTES) => {
                    return Err(WireError::too_large(format!(
                        "arguments of call `{}` are over the {MAX_TOOL_ARGUMENT_BYTES} byte bound",
                        call.call_id
                    )));
                }
                Item::ToolResult {
                    call_id, output, ..
                } if exceeds(output, MAX_TOOL_RESULT_BYTES) => {
                    return Err(WireError::too_large(format!(
                        "result of call `{call_id}` is over the {MAX_TOOL_RESULT_BYTES} byte bound"
                    )));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Refuses opaque items no adapter for `wire` can have produced.
    ///
    /// A reasoning blob from one provider is meaningless to another and, replayed, is at best a
    /// hard error and at worst silently poisoned context. Refusing by name is the only outcome
    /// that tells the caller which item is wrong.
    ///
    /// # Errors
    ///
    /// Returns [`WireErrorCode::Unsupported`] naming the first foreign item.
    pub fn check_opaque_items(&self, wire: &WireId) -> Result<(), WireError> {
        for (index, item) in self.items.iter().enumerate() {
            if let Some(origin) = item.opaque_wire()
                && origin != wire
            {
                return Err(WireError::new(
                    WireErrorCode::Unsupported,
                    format!(
                        "item {index} is an opaque `{origin}` item and cannot be replayed into `{wire}`"
                    ),
                    false,
                ));
            }
        }
        Ok(())
    }

    pub fn tool(&self, name: &ToolName) -> Option<&ToolSpec> {
        self.tools.iter().find(|tool| &tool.name == name)
    }
}

/// Why the model stopped producing this turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StopReason {
    /// The model finished and asked for nothing further.
    EndTurn,
    /// The model asked for one or more tools before it can continue.
    ToolCalls,
    /// The output bound cut the turn short.
    MaxOutputTokens,
    /// The provider ended the turn for a reason it named itself.
    Incomplete { reason: String },
}

/// Token counts as the provider reported them.
///
/// Every field is a report, never a host measurement. A provider that reports nothing produces no
/// `Usage` at all rather than a zeroed one, because a zero is a claim that no tokens were spent.
///
/// # `input_tokens` is the whole, and the cache figures are parts of it
///
/// Stated here because it was previously stated nowhere and only one wire existed to disagree
/// with. `input_tokens` counts **every** input token the turn was charged for, and
/// [`Usage::cached_input_tokens`] and [`Usage::cache_creation_input_tokens`] are subsets of it —
/// which is why [`crate`]'s only consumer computes the uncached count as a difference.
///
/// The second wire reports its three input figures **disjointly**, so its projection sums them
/// before filling this in. That is the projection's job, not the reader's: a value whose meaning
/// depended on which wire produced it would make every figure downstream ambiguous.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
    /// Input tokens the provider charged to **write** into its prompt cache, when it says so.
    ///
    /// # Why this is an `Option` and the others are not
    ///
    /// It is the one figure a provider may not report at all, and the distinction matters: a wire
    /// whose provider never mentions cache writes must not claim there were none. [`None`] is
    /// *unreported*; `Some(0)` is *reported as zero* (AGENTS.md invariant 7).
    ///
    /// It arrived with the second wire, which reports it and bills it above the plain input rate.
    /// Nothing here prices it separately — the rate card has no field for a cache-write premium —
    /// so it is counted inside `input_tokens` and priced at the input rate, which understates a
    /// cache-writing turn. Carrying the figure is what makes that understatement visible instead
    /// of invisible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnOutcome {
    pub stop_reason: StopReason,
    pub items: Vec<Item>,
    /// Absent when the provider reported none. Never defaulted to zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

impl TurnOutcome {
    /// Returns every tool call the model asked for, in the order it asked.
    pub fn tool_calls(&self) -> impl Iterator<Item = &crate::item::ToolCall> {
        self.items.iter().filter_map(Item::as_tool_call)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::CallId;
    use crate::item::ToolCall;
    use serde_json::json;

    fn tool(name: &str) -> ToolSpec {
        ToolSpec {
            name: ToolName::new(name).expect("valid tool name"),
            description: "does a thing".to_owned(),
            input_schema: json!({"type": "object"}),
            approval: Approval::NotRequired,
            envelope: crate::Envelope::default(),
        }
    }

    #[test]
    fn a_sampling_value_outside_its_range_refuses_before_the_round_trip() {
        for (sampling, expected) in [
            (
                Sampling {
                    temperature: Some(3.0),
                    ..Sampling::default()
                },
                "temperature",
            ),
            (
                Sampling {
                    top_p: Some(0.0),
                    ..Sampling::default()
                },
                "top_p",
            ),
            (
                Sampling {
                    reasoning_effort: Some(String::new()),
                    ..Sampling::default()
                },
                "reasoning effort",
            ),
        ] {
            let error = sampling.validate().expect_err("out of range");
            assert_eq!(error.code, WireErrorCode::Protocol);
            assert!(error.message.contains(expected), "{}", error.message);
        }
    }

    #[test]
    fn a_turn_with_no_sampling_serializes_without_the_field() {
        let encoded = serde_json::to_value(request(Vec::new(), Vec::new())).expect("serializes");
        // Absence has to survive the encoding too: a `"sampling": {}` on the wire is a different
        // statement from having asked for nothing.
        assert!(encoded.get("sampling").is_none(), "{encoded}");
    }

    fn request(items: Vec<Item>, tools: Vec<ToolSpec>) -> TurnRequest {
        TurnRequest {
            model: "test-model".to_owned(),
            instructions: "be useful".to_owned(),
            items,
            tools,
            max_output_tokens: None,
            sampling: Sampling::default(),
        }
    }

    #[test]
    fn a_valid_turn_passes() {
        assert!(
            request(vec![Item::user("hi")], vec![tool("a")])
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn a_turn_without_a_model_refuses() {
        let mut turn = request(Vec::new(), Vec::new());
        turn.model = String::new();
        assert_eq!(
            turn.validate().expect_err("empty model refuses").code,
            WireErrorCode::Protocol
        );
    }

    #[test]
    fn duplicate_tool_names_refuse_by_name() {
        let error = request(Vec::new(), vec![tool("a"), tool("a")])
            .validate()
            .expect_err("a duplicate tool refuses");
        assert_eq!(error.code, WireErrorCode::Protocol);
        assert!(error.message.contains("`a`"), "{}", error.message);
    }

    #[test]
    fn oversized_instructions_refuse() {
        let mut turn = request(Vec::new(), Vec::new());
        turn.instructions = "x".repeat(MAX_INSTRUCTION_BYTES + 1);
        assert_eq!(
            turn.validate().expect_err("oversized instructions").code,
            WireErrorCode::TooLarge
        );
    }

    #[test]
    fn oversized_tool_arguments_refuse() {
        let call = Item::ToolCall(ToolCall {
            call_id: CallId::new("call-1").expect("valid"),
            name: ToolName::new("a").expect("valid"),
            arguments: json!({ "blob": "x".repeat(MAX_TOOL_ARGUMENT_BYTES) }),
        });
        assert_eq!(
            request(vec![call], vec![tool("a")])
                .validate()
                .expect_err("oversized arguments")
                .code,
            WireErrorCode::TooLarge
        );
    }

    #[test]
    fn oversized_tool_results_refuse() {
        let result = Item::ToolResult {
            call_id: CallId::new("call-1").expect("valid"),
            output: json!("x".repeat(MAX_TOOL_RESULT_BYTES)),
            failed: false,
        };
        assert_eq!(
            request(vec![result], Vec::new())
                .validate()
                .expect_err("oversized result")
                .code,
            WireErrorCode::TooLarge
        );
    }

    #[test]
    fn too_many_tools_refuse() {
        let tools = (0..=MAX_TOOLS)
            .map(|index| tool(&format!("t{index}")))
            .collect();
        assert_eq!(
            request(Vec::new(), tools)
                .validate()
                .expect_err("too many tools")
                .code,
            WireErrorCode::TooLarge
        );
    }

    #[test]
    fn opaque_items_from_another_wire_refuse_by_index() {
        let turn = request(
            vec![
                Item::user("hi"),
                Item::Opaque {
                    wire: WireId::new("anthropic-messages").expect("valid"),
                    payload: json!({}),
                },
            ],
            Vec::new(),
        );
        let wire = WireId::new("openai-responses").expect("valid");
        let error = turn
            .check_opaque_items(&wire)
            .expect_err("a foreign opaque item refuses");
        assert_eq!(error.code, WireErrorCode::Unsupported);
        assert!(error.message.contains("item 1"), "{}", error.message);
    }

    #[test]
    fn opaque_items_from_the_same_wire_pass() {
        let wire = WireId::new("openai-responses").expect("valid");
        let turn = request(
            vec![Item::Opaque {
                wire: wire.clone(),
                payload: json!({}),
            }],
            Vec::new(),
        );
        assert!(turn.check_opaque_items(&wire).is_ok());
    }

    #[test]
    fn a_cache_write_figure_nobody_reported_is_absent_rather_than_zero() {
        // `Some(0)` is *reported as zero*; `None` is *not reported*. A wire whose provider never
        // mentions cache writes must not claim on its behalf that there were none.
        let usage = Usage {
            model: "m".to_owned(),
            input_tokens: 10,
            output_tokens: 2,
            cached_input_tokens: 0,
            cache_creation_input_tokens: None,
        };
        let encoded = serde_json::to_value(&usage).expect("serializes");
        assert!(
            encoded.get("cache_creation_input_tokens").is_none(),
            "{encoded}"
        );
        // And a wire that does report it round trips the number rather than the absence.
        let reported = Usage {
            cache_creation_input_tokens: Some(0),
            ..usage.clone()
        };
        let encoded = serde_json::to_value(&reported).expect("serializes");
        assert_eq!(encoded["cache_creation_input_tokens"], serde_json::json!(0));
        assert_eq!(
            serde_json::from_value::<Usage>(encoded).expect("deserializes"),
            reported
        );
    }

    #[test]
    fn absent_usage_stays_absent() {
        let outcome = TurnOutcome {
            stop_reason: StopReason::EndTurn,
            items: Vec::new(),
            usage: None,
        };
        let encoded = serde_json::to_value(&outcome).expect("serializes");
        assert!(encoded.get("usage").is_none(), "{encoded}");
    }
}
