//! What a delegate running on its own thread may ask the run it belongs to.
//!
//! # One approver, one hook file, one event stream — however many children
//!
//! A group of delegates runs side by side, but three of the things a child needs are not
//! duplicable and must not be duplicated. A person answering an approval is one person, and two
//! prompts written to one terminal at once are a prompt nobody can answer. An operator's hook is a
//! program they wrote expecting to be asked about one call at a time. And the run's
//! [`LoopSink`](crate::LoopSink) is the record: a single ordered stream a renderer indents and a
//! JSONL reader parses.
//!
//! So a child thread owns its model port and its tool port — genuinely forked, genuinely parallel
//! — and **borrows nothing else**. Everything else it needs, it asks the parent thread for over a
//! channel, and the parent answers them one at a time in the order they arrive. The parent is
//! sitting in a receive loop for exactly as long as any child is running, so there is no moment at
//! which a question goes unanswered.
//!
//! # Every proxy fails closed
//!
//! Each of these blocks on an answer from the parent. If the parent is gone — the run was torn
//! down, the channel hung up — the answer is the refusing one: an approval nobody gave is a
//! denial, and a hook that could not be consulted did not say yes (design 0002 § 3). A proxy that
//! defaulted to *proceed* on a dead channel would be a way to run a call past the gate by
//! arranging for the gate to be unreachable.
//!
//! Design: `docs/design/0002-sub-agents-structured-output-hooks.md` § 2, milestone M4.

use std::sync::mpsc::{Receiver, Sender, channel};

use harness_wire::{ToolCall, ToolOutcome, ToolSpec};

use crate::approval::{ApprovalDecision, ApprovalPort};
use crate::event::{LoopEvent, LoopSink};
use crate::hook::{AfterCall, HookDecision, HookPort};

/// What a child on a worker thread sends to the run that started it.
///
/// The three that carry a `reply` are questions and the child blocks on the answer; the fourth is
/// a statement and it does not.
pub(crate) enum FromChild {
    /// Something the child's own loop emitted. `at` is its index in the group, which is how the
    /// parent knows whose `call_id` to wrap it in.
    Event { at: usize, event: LoopEvent },
    /// The child's approver was asked. The `ApprovalRequired` and `ApprovalResolved` events around
    /// this are the child's own and arrive as [`FromChild::Event`]; what crosses here is only the
    /// decision, because the person deciding is the parent's.
    Decide {
        call: ToolCall,
        invoked: ToolSpec,
        reply: Sender<ApprovalDecision>,
    },
    /// The operator's `before-call` hook, for a call inside the child.
    BeforeCall {
        call: ToolCall,
        invoked: ToolSpec,
        reply: Sender<HookDecision>,
    },
    /// The operator's `after-call` hook, for a call inside the child.
    AfterCall {
        call: ToolCall,
        invoked: ToolSpec,
        outcome: ToolOutcome,
        reply: Sender<AfterCall>,
    },
}

/// Asks the parent thread one question and waits for its answer.
///
/// `closed` is what the child is told when the channel is not there to carry the question or the
/// answer back. It is always the refusing answer — see this module's header.
fn ask<T>(
    tx: &Sender<FromChild>,
    question: impl FnOnce(Sender<T>) -> FromChild,
    closed: impl FnOnce() -> T,
) -> T {
    let (reply, answer) = channel();
    if tx.send(question(reply)).is_err() {
        return closed();
    }
    answer.recv().unwrap_or_else(|_| closed())
}

/// The child's own [`LoopSink`], which forwards rather than records.
///
/// Nothing is filtered here and nothing is wrapped here: the parent wraps, because the wrapping
/// needs the `call_id` of the `delegate` call and the child does not have it. What the child
/// contributes is `at` — which of the group it is.
pub(crate) struct ChildSink {
    pub(crate) at: usize,
    pub(crate) tx: Sender<FromChild>,
}

impl LoopSink for ChildSink {
    fn emit(&mut self, event: LoopEvent) {
        // A send that fails is a parent that has stopped listening, which happens only when the
        // whole group is being torn down. There is nowhere else to put the event and nothing the
        // child can do about it; it is dropped rather than made into a second failure.
        let _ = self.tx.send(FromChild::Event { at: self.at, event });
    }
}

/// The child's [`ApprovalPort`]: the parent's approver, asked from another thread.
///
/// Two children that both need a decision queue behind one another, which is the behaviour a
/// person in front of a terminal needs. Nothing here times a person out — a delegate waiting on an
/// approval is the run waiting on an approval, exactly as it is in a run with no delegates at all.
pub(crate) struct ChildApprovals {
    pub(crate) tx: Sender<FromChild>,
}

impl ApprovalPort for ChildApprovals {
    fn decide(&mut self, call: &ToolCall, invoked: &ToolSpec) -> ApprovalDecision {
        let name = invoked.name.clone();
        ask(
            &self.tx,
            |reply| FromChild::Decide {
                call: call.clone(),
                invoked: invoked.clone(),
                reply,
            },
            || {
                ApprovalDecision::denied(format!(
                    "`{name}` needs a person's decision and the run this delegate belongs to is no \
                     longer there to put it to them, so retrying cannot approve it either; do what \
                     can be done without it and say what could not"
                ))
            },
        )
    }
}

/// The child's [`HookPort`]: the operator's programs, run on the parent thread.
///
/// # Why the programs do not run here
///
/// A hook is an argv the shell spawns, and the shell attached exactly one [`HookPort`] to this
/// run. Handing a clone of it to four threads would mean *how many copies of my guard are running*
/// depended on how many sub-tasks the model happened to ask for — the same objection that keeps a
/// hooked run from batching its tool calls. So they are asked one at a time, here, in the order
/// the children got to them.
///
/// [`HookPort::on_stop`] is deliberately left at its default. A delegate is `nested`, and
/// [`crate::AgentLoop::stop_hook`] returns before consulting anything for a nested run: a child's
/// ending is not the run's ending. Implementing it would be implementing a point that cannot fire.
pub(crate) struct ChildHooks {
    pub(crate) tx: Sender<FromChild>,
}

impl HookPort for ChildHooks {
    fn before_call(&mut self, call: &ToolCall, invoked: &ToolSpec) -> HookDecision {
        ask(
            &self.tx,
            |reply| FromChild::BeforeCall {
                call: call.clone(),
                invoked: invoked.clone(),
                reply,
            },
            || {
                HookDecision::failed(
                    "the run this delegate belongs to is no longer there to consult the \
                     operator's hooks",
                )
            },
        )
    }

    fn after_call(
        &mut self,
        call: &ToolCall,
        invoked: &ToolSpec,
        outcome: &ToolOutcome,
    ) -> AfterCall {
        ask(
            &self.tx,
            |reply| FromChild::AfterCall {
                call: call.clone(),
                invoked: invoked.clone(),
                outcome: outcome.clone(),
                reply,
            },
            || {
                AfterCall::failed(
                    "the run this delegate belongs to is no longer there to consult the \
                     operator's hooks",
                )
            },
        )
    }
}

/// Answers every child question until the last child has finished.
///
/// # It ends when the children do, and it cannot end early
///
/// The loop runs until the channel is disconnected, which happens exactly when every clone of the
/// sender has been dropped — one per child thread, plus the one the caller drops before calling
/// this. So *the parent is listening for as long as a child could still ask something*, which is
/// the property that makes the approver and the hooks reachable from a worker thread at all.
///
/// # What it does not do
///
/// It takes no decision of its own. An approval is the run's approver's, a hook's answer is the
/// operator's program's, and the events it wraps are the child's — including the
/// `ApprovalRequired`, `ApprovalResolved` and `HookRan` events for the very questions answered
/// here, which the child emits into its own stream exactly as it would in a run of its own.
///
/// The two ports carry their own object lifetimes (`'p`, `'h`) rather than borrowing them from the
/// references that reach them. Written out because the elided form ties a trait object's lifetime
/// to the exclusive reference holding it, and a `&mut` is invariant: the parent's ports live as
/// long as the run and are lent here for the length of one group, which the elided form cannot say.
pub(crate) fn answer_children<'p, 'h>(
    rx: &Receiver<FromChild>,
    call_ids: &[harness_wire::CallId],
    approvals: &mut (dyn ApprovalPort + 'p),
    mut hooks: Option<&mut (dyn HookPort + 'h)>,
    sink: &mut dyn LoopSink,
) {
    while let Ok(message) = rx.recv() {
        match message {
            FromChild::Event { at, event } => {
                let Some(call_id) = call_ids.get(at) else {
                    continue;
                };
                sink.emit(LoopEvent::Delegated {
                    call_id: call_id.clone(),
                    event: Box::new(event),
                });
            }
            FromChild::Decide {
                call,
                invoked,
                reply,
            } => {
                let decision = approvals.decide(&call, &invoked);
                // A child that stopped waiting is a child whose run ended; the decision is still
                // the approver's and it was still taken, so nothing is retried and nothing is
                // reported here.
                let _ = reply.send(decision);
            }
            FromChild::BeforeCall {
                call,
                invoked,
                reply,
            } => {
                let decision = hooks.as_deref_mut().map_or(HookDecision::Proceed, |hooks| {
                    hooks.before_call(&call, &invoked)
                });
                let _ = reply.send(decision);
            }
            FromChild::AfterCall {
                call,
                invoked,
                outcome,
                reply,
            } => {
                let answer = hooks
                    .as_deref_mut()
                    .map_or_else(AfterCall::default, |hooks| {
                        hooks.after_call(&call, &invoked, &outcome)
                    });
                let _ = reply.send(answer);
            }
        }
    }
}

/// What a panicking child is reported as, from the payload a caught unwind carries.
///
/// The same three cases the tool catalogue's own batch reads, because a panic payload is a `&str`
/// for `panic!("…")`, a `String` for a formatted one, and anything at all for a payload nobody
/// chose to make readable.
pub(crate) fn panic_words(payload: &dyn std::any::Any) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        return (*text).to_owned();
    }
    if let Some(text) = payload.downcast_ref::<String>() {
        return text.clone();
    }
    "no message".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_wire::{Approval, CallId, Envelope, ToolName};
    use serde_json::json;

    fn a_call() -> (ToolCall, ToolSpec) {
        let name = ToolName::new("file_write").expect("valid");
        (
            ToolCall {
                call_id: CallId::new("call-1").expect("valid"),
                name: name.clone(),
                arguments: json!({}),
            },
            ToolSpec {
                name,
                description: "writes".to_owned(),
                input_schema: json!({"type": "object"}),
                approval: Approval::NotRequired,
                envelope: Envelope::default(),
            },
        )
    }

    #[test]
    fn an_approval_nobody_is_left_to_answer_is_a_denial_naming_the_tool() {
        let (tx, rx) = channel();
        drop(rx);
        let (call, invoked) = a_call();
        let decision = ChildApprovals { tx }.decide(&call, &invoked);
        let ApprovalDecision::Denied { reason } = decision else {
            panic!("a delegate whose parent is gone must not be approved by default");
        };
        assert!(reason.contains("`file_write`"), "{reason}");
    }

    #[test]
    fn a_hook_that_cannot_be_consulted_blocks_the_call_rather_than_letting_it_through() {
        let (tx, rx) = channel();
        drop(rx);
        let (call, invoked) = a_call();
        let decision = ChildHooks { tx }.before_call(&call, &invoked);
        assert!(
            !decision.is_proceed(),
            "a hook that could not run did not say yes: {decision:?}"
        );
    }

    #[test]
    fn an_after_call_hook_that_cannot_be_consulted_says_so_to_the_model_and_to_the_record() {
        let (tx, rx) = channel();
        drop(rx);
        let (call, invoked) = a_call();
        let answered = ChildHooks { tx }.after_call(&call, &invoked, &ToolOutcome::failed("no"));
        assert!(
            answered.note.is_some(),
            "the model is told the check never happened"
        );
        assert!(
            matches!(answered.decision, HookDecision::Failed { .. }),
            "the record says the point failed: {:?}",
            answered.decision
        );
    }

    #[test]
    fn a_childs_event_reaches_the_parents_sink_wrapped_in_the_delegate_calls_id() {
        let (tx, rx) = channel();
        let mut sink = crate::event::VecLoopSink::new();
        let mut child = ChildSink { at: 0, tx };
        child.emit(LoopEvent::Warning {
            code: "from-the-child".to_owned(),
            message: "heard".to_owned(),
        });
        // The senders the loop waits on are the children's; dropping the last one is what ends it.
        drop(child);
        let ids = vec![CallId::new("call-7").expect("valid")];
        answer_children(&rx, &ids, &mut crate::approval::DenyAll, None, &mut sink);
        let [LoopEvent::Delegated { call_id, event }] = sink.events() else {
            panic!("one wrapped event: {:?}", sink.events());
        };
        assert_eq!(call_id.as_str(), "call-7");
        assert!(matches!(**event, LoopEvent::Warning { .. }));
    }
}
