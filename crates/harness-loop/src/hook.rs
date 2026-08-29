//! Hooks: the operator's own programs, consulted at three moments of a run.
//!
//! # A hook can only narrow
//!
//! `before-call` fires **after** the approver said yes, and its block is one more refusal. It
//! cannot approve, cannot change the call, and cannot reach a tool the run did not publish. A hook
//! that widened would be a second gate nobody reviews (AGENTS.md invariant 12). `stop` can keep a
//! run working — that is narrowing too: it takes away the model's right to stop, never adds a
//! right to act.
//!
//! # The loop spawns nothing
//!
//! This is a port, exactly as [`crate::ApprovalPort`] is. The implementation that runs a process
//! lives in the shell, where the file naming the hooks was read from a path the operator gave.
//! Nothing here is ever discovered from the workspace: a hook found in a repository would be a
//! program the *repository* runs on the operator's machine.
//!
//! Design: `docs/design/0002-sub-agents-structured-output-hooks.md` § 3.

use harness_wire::{ToolCall, ToolOutcome, ToolSpec};
use serde::{Deserialize, Serialize};

/// Where in a run a hook is consulted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HookPoint {
    /// After approval and before the tool port runs the call.
    BeforeCall,
    /// After the outcome exists and before the model reads it.
    AfterCall,
    /// When the run would otherwise end `Completed`.
    Stop,
}

impl HookPoint {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BeforeCall => "before-call",
            Self::AfterCall => "after-call",
            Self::Stop => "stop",
        }
    }
}

/// What a hook decided, or that it could not decide.
///
/// `Failed` is its own variant rather than folded into either answer, because the two points read
/// it differently: before a call it is a block (a hook that could not run did not say yes), at a
/// stop it is a proceed with a warning (a hook that crashed must not keep a run alive for ever).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum HookDecision {
    Proceed,
    Block { reason: String },
    Failed { reason: String },
}

impl HookDecision {
    pub fn block(reason: impl Into<String>) -> Self {
        Self::Block {
            reason: reason.into(),
        }
    }

    pub fn failed(reason: impl Into<String>) -> Self {
        Self::Failed {
            reason: reason.into(),
        }
    }

    pub fn is_proceed(&self) -> bool {
        matches!(self, Self::Proceed)
    }
}

/// What an `after-call` hook said: a note for the model, and whether the hook itself could run.
///
/// Two fields rather than one `Option<String>` because the record and the model need different
/// halves of the same firing. The model needs the note — that the formatter ran, that a check
/// failed. A reader of the record needs to know a hook *crashed*, which a note alone could never
/// tell it: the outcome's `failed` is the tool's own and an after-call hook may not touch it
/// ([`HookPort::after_call`]), so a failure that only became a note left
/// [`crate::LoopEvent::HookRan`] saying `proceed` about a hook that did not.
///
/// There is deliberately no block here: at this point the effect has already happened, so there is
/// nothing left to refuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AfterCall {
    /// What the model reads beside the result, under `hook_notes`. [`None`] is silence.
    pub note: Option<String>,
    /// What the record says this hook decided: [`HookDecision::Proceed`] or
    /// [`HookDecision::Failed`], never a block.
    pub decision: HookDecision,
}

impl Default for AfterCall {
    /// Nothing to say, and nothing went wrong — what a port that does not implement the point
    /// answers.
    fn default() -> Self {
        Self {
            note: None,
            decision: HookDecision::Proceed,
        }
    }
}

impl AfterCall {
    /// A hook that ran and left the model a note.
    pub fn note(text: impl Into<String>) -> Self {
        Self {
            note: Some(text.into()),
            decision: HookDecision::Proceed,
        }
    }

    /// A hook that could not run, its reason carried **twice on purpose**: as the decision, so the
    /// record shows the point failed, and as the note, so the model still learns that the check it
    /// would have read never happened. Dropping either half loses one of the two readers.
    pub fn failed(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            note: Some(reason.clone()),
            decision: HookDecision::failed(reason),
        }
    }
}

/// The operator's programs, at the three moments they may speak.
///
/// Every method is defaulted to *say nothing*, so a port that only wants one point implements one
/// method. Blocking calls, like the approver: the loop stops until the hook returns, so a decision
/// cannot land after the effect.
pub trait HookPort {
    /// Before `invoked` runs for `call`, and after a person (or the ceiling) allowed it.
    ///
    /// `invoked` is the **entry** that will run — `file_write`, `run` — not the verb the model
    /// called it through, so a hook filtering by tool name filters on what actually happens.
    ///
    /// Anything but [`HookDecision::Proceed`] stops the call: a block is a refusal the model is
    /// told about, and [`HookDecision::Failed`] is the same refusal with a different sentence,
    /// because a hook that could not run did not say yes. Neither can approve a call the approver
    /// already denied — that call never arrives here.
    fn before_call(&mut self, call: &ToolCall, invoked: &ToolSpec) -> HookDecision {
        let _ = (call, invoked);
        HookDecision::Proceed
    }

    /// After `outcome` exists for `call`. Returns the note the model reads beside the result and
    /// what the record says this hook decided. A hook here cannot change the outcome and cannot
    /// mark it failed: the effect has already happened, and `before-call` is the point that speaks
    /// in time to prevent it. [`AfterCall::failed`] says the *hook* could not run, never that the
    /// tool did not.
    ///
    /// The note lands under `hook_notes` on the result — an object grows the array, anything else
    /// is wrapped as `{"output", "hook_notes"}`. It is counted against
    /// [`harness_wire::MAX_TOOL_RESULT_BYTES`] like everything else the model reads: a note that
    /// puts a result over the bound refuses that result by name rather than trimming either.
    ///
    /// # It does not fire for a call that never ran, and that is intended
    ///
    /// A call refused before the tool — a name the run never published, arguments over
    /// [`harness_wire::MAX_TOOL_ARGUMENT_BYTES`], an approver's denial, a `before-call` block —
    /// does not reach this point at all: there is no outcome a tool produced, and that is the
    /// only thing this point is about. Those refusals are not silent, they are simply somewhere else in the
    /// record: `ToolCompleted { failed: true }` for every one of them, with
    /// `ApprovalResolved { approved: false }` beside a denial and `HookRan { point: before-call }`
    /// beside a block. An audit that has to see refusals reads those; a hook that has to see what
    /// a tool did reads this.
    fn after_call(
        &mut self,
        call: &ToolCall,
        invoked: &ToolSpec,
        outcome: &ToolOutcome,
    ) -> AfterCall {
        let _ = (call, invoked, outcome);
        AfterCall::default()
    }

    /// When the run would end with `text` as its answer. A block's reason becomes one more user
    /// item and the loop turns again, at most [`crate::MAX_STOP_HOOK_CONTINUES`] times.
    ///
    /// `text` is what a consumer would have read: the structured answer where the run has one,
    /// otherwise the model's prose. Unlike the other two points, [`HookDecision::Failed`] here
    /// **fails open** — the run ends, with a `hook-failed` warning — because a crashed hook that
    /// blocked every stop would be a run with no end.
    fn on_stop(&mut self, text: &str) -> HookDecision {
        let _ = text;
        HookDecision::Proceed
    }
}

/// No hooks at all. The loop's default.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NoHooks;

impl HookPort for NoHooks {}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_wire::{Approval, CallId, Envelope, ToolName};
    use serde_json::json;

    #[test]
    fn no_hooks_says_nothing_at_every_point() {
        let call = ToolCall {
            call_id: CallId::new("call-1").expect("valid"),
            name: ToolName::new("file_write").expect("valid"),
            arguments: json!({}),
        };
        let spec = ToolSpec {
            name: call.name.clone(),
            description: "writes".to_owned(),
            input_schema: json!({"type": "object"}),
            approval: Approval::NotRequired,
            envelope: Envelope::default(),
        };
        let mut hooks = NoHooks;
        assert!(hooks.before_call(&call, &spec).is_proceed());
        assert_eq!(
            hooks.after_call(&call, &spec, &ToolOutcome::ok(json!({}))),
            AfterCall::default(),
            "no note, and nothing that says a hook failed"
        );
        assert!(hooks.on_stop("done").is_proceed());
    }

    #[test]
    fn an_after_call_failure_is_carried_to_the_record_and_to_the_model_at_once() {
        // The two readers need different halves and neither may be dropped: without the decision
        // the record says a hook that crashed proceeded, and without the note the model reads a
        // result whose check silently never ran.
        let failed = AfterCall::failed("the formatter could not be started");
        assert_eq!(
            failed.decision,
            HookDecision::failed("the formatter could not be started"),
            "without the decision the record says a hook that crashed proceeded"
        );
        assert_eq!(
            failed.note.as_deref(),
            Some("the formatter could not be started"),
            "without the note the model reads a result whose check silently never ran"
        );
        assert_eq!(
            AfterCall::note("rustfmt ran").decision,
            HookDecision::Proceed
        );
    }

    #[test]
    fn decisions_and_points_round_trip_in_kebab_case() {
        let encoded = serde_json::to_value(HookDecision::block("tests failed")).expect("encodes");
        assert_eq!(encoded, json!({"kind": "block", "reason": "tests failed"}));
        assert_eq!(
            serde_json::to_value(HookPoint::BeforeCall).expect("encodes"),
            json!("before-call")
        );
        assert_eq!(HookPoint::Stop.as_str(), "stop");
    }
}
