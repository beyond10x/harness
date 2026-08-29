//! Sub-agents: `delegate`, a fresh context on the same gate.
//!
//! A delegate is a second [`crate::AgentLoop`] run to completion **inside** the first one's tool
//! call, over a conversation that starts empty, returning its final text as the tool result. It
//! shares the parent's model port, tool port, approver, hooks, cancellation token and the
//! remainder of the parent's budget; it has its own turn ceiling and no `delegate` of its own.
//! Delegation widens nothing: the child can do exactly what the parent's catalogue admits, entry
//! for entry, and every call inside it meets the same gate.
//!
//! Design: `docs/design/0002-sub-agents-structured-output-hooks.md` § 2.

use harness_wire::{Approval, Envelope, Idempotency, Risk, ToolName, ToolSpec};
use serde_json::json;

/// The tool name delegation is published under when the caller names none.
pub const DEFAULT_DELEGATE_NAME: &str = "delegate";

/// Model turns a delegate may take before its result says it stopped on that bound.
///
/// Its own ceiling rather than the parent's remainder, because a child that loops must not spend
/// the parent's remaining fifty turns finding out.
pub const DELEGATE_MAX_TURNS: u64 = 20;

/// How deep delegation goes. One: a delegate publishes no `delegate` of its own.
///
/// A tree of delegates, and delegates side by side, is milestone M4 of design 0002 — wanted when a
/// run shows the need, not before, because each level is a context nobody can read afterwards.
pub const MAX_DELEGATION_DEPTH: u32 = 1;

/// What a delegate is told about being one, appended to the parent's standing instruction.
///
/// The parent's instruction is kept whole in front of it — the environment block, the project
/// instructions, the catalogue brief — because a delegate that does not know where it is cannot
/// use the tools it was given. What this adds is the four things that are only true of a child:
/// it cannot read the conversation it came from, nobody is there to answer a question, its final
/// message is the entire result, and that message travels back as one tool result and is refused
/// by name if it is too large.
///
/// The bound is described rather than quoted, because quoting a `usize` constant in a `&str`
/// constant means writing the figure twice and having the second copy go stale.
pub const DELEGATE_PREAMBLE: &str = "You are a delegate. Another agent handed you one \
    self-contained task and is blocked until you answer it. You cannot see the conversation the \
    task came from, so nothing in it is implied: work only from what you were given and from what \
    your own tools can find out. There is nobody to ask — no question you write reaches a person, \
    and asking one only spends a turn — so where something is underspecified, decide, act, and say \
    in your report what you decided and why. You report exactly once, in the text of your final \
    message: that message is the whole of what the agent that sent you reads, and everything else \
    — what you read, what you tried, your reasoning — is discarded with your context. Write it \
    for somebody who saw none of that. Keep it short, a few hundred words at most: it travels back \
    as a single tool result, and one over this harness's result bound is refused by name and the \
    agent that sent you gets nothing at all.";

/// What the model is told `delegate` is for.
pub const DELEGATE_DESCRIPTION: &str = "Hand a self-contained sub-task to a fresh context with \
    the same tools: research, a survey of many files, a change you can state in one sentence. It \
    cannot see this conversation, so the task must say everything it needs. It reports once, in \
    text, when it is done; you read only that report.";

/// Whether, and how, a run may delegate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delegation {
    /// The tool the model calls to delegate.
    pub name: ToolName,
    /// The child's own turn ceiling.
    pub max_turns: u64,
    /// How many further levels may delegate. `1` is a child that cannot; `0` publishes nothing.
    pub depth: u32,
}

impl Default for Delegation {
    fn default() -> Self {
        Self {
            name: ToolName::new(DEFAULT_DELEGATE_NAME)
                .expect("the default delegate name is a legal tool name"),
            max_turns: DELEGATE_MAX_TURNS,
            depth: MAX_DELEGATION_DEPTH,
        }
    }
}

impl Delegation {
    #[must_use]
    pub fn with_max_turns(mut self, max_turns: u64) -> Self {
        self.max_turns = max_turns;
        self
    }

    /// The tool the model sees.
    ///
    /// The delegate call itself touches nothing: every effect inside it is a call of its own,
    /// gated on its own entry's envelope. So the envelope is honest at `risk: Low` with no
    /// effects, and the gate never asks about the delegation — it asks about what the delegate
    /// then does. Non-idempotent because two delegations of one task are two runs.
    pub fn spec(&self) -> ToolSpec {
        self.spec_with_agents(&[])
    }

    /// The same tool, offering the named agents this run actually has.
    ///
    /// The names are a schema `enum` for the reason the `skill` tool's are: a model that has to
    /// guess a name spends a call finding out, and the provider can refuse an `enum` violation
    /// without this loop being asked at all. A run with no agents gets no `agent` key, so the
    /// option does not exist rather than existing and always failing.
    #[must_use]
    pub fn spec_with_agents(&self, agents: &[String]) -> ToolSpec {
        let mut spec = self.bare_spec();
        if agents.is_empty() {
            if let Some(properties) = spec.input_schema["properties"].as_object_mut() {
                properties.remove("agent");
            }
        } else {
            spec.input_schema["properties"]["agent"]["enum"] = json!(agents);
        }
        spec
    }

    fn bare_spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name.clone(),
            description: DELEGATE_DESCRIPTION.to_owned(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "Everything the delegate needs: the goal, the constraints, and what to report."
                    },
                    // **Optional, and its absence is the generic delegate.** A run with no agents
                    // publishes this schema without the key at all — see `spec_with_agents` — so a
                    // model on such a run cannot spend a call naming one.
                    "agent": {
                        "type": "string",
                        "description": "Which named agent to run this as. Omit for a delegate with these same tools."
                    }
                },
                "required": ["task"],
                "additionalProperties": false
            }),
            approval: Approval::NotRequired,
            envelope: Envelope {
                effects: Vec::new(),
                risk: Risk::Low,
                idempotency: Idempotency::NonIdempotent,
                access: Vec::new(),
            },
        }
    }

    /// What a child of this run may delegate: one level less, or nothing.
    pub fn for_child(&self) -> Option<Self> {
        let depth = self.depth.checked_sub(1)?;
        (depth > 0).then(|| Self {
            name: self.name.clone(),
            max_turns: self.max_turns,
            depth,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_delegation_is_one_level_deep_and_its_child_cannot_delegate() {
        let delegation = Delegation::default();
        assert_eq!(delegation.depth, MAX_DELEGATION_DEPTH);
        assert_eq!(delegation.max_turns, DELEGATE_MAX_TURNS);
        assert_eq!(delegation.for_child(), None);
        let two = Delegation {
            depth: 2,
            ..Delegation::default()
        };
        assert_eq!(two.for_child().map(|child| child.depth), Some(1));
    }

    #[test]
    fn the_spec_requires_a_task_and_asks_nobody() {
        let spec = Delegation::default().spec();
        assert_eq!(spec.name.as_str(), DEFAULT_DELEGATE_NAME);
        assert_eq!(spec.input_schema["required"], json!(["task"]));
        assert!(!spec.envelope.mutates());
        assert!(!spec.envelope.needs_approval(Risk::Low));
    }
}
