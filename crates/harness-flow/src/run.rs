//! Walking a plan.
//!
//! The walk knows about order and failure and nothing else. What a step *is* — a turn of the loop,
//! a tool call, a command — is the caller's, behind [`StepRunner`], which is what lets the whole
//! scheduler be tested without a provider, a credential or a network.
//!
//! It can also be **told no**, at the two moments a section boundary is crossed:
//! [`StepRunner::entering`] before a section's attempt runs anything, [`StepRunner::leaving`] after
//! that attempt has said what it hands over. A [`Gate::Refused`] becomes one of the two moves the
//! walk already has — skip the section as failed, or re-enter it — so being governed costs the walk
//! no new concept and it still evaluates nothing. The reason is carried and never read, exactly as
//! a step's `run` payload is.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::{FlowEvent, FlowSink, Group, Moment, Node, NodeId, Plan, Step};

/// What a group hands its siblings when it leaves.
///
/// Keyed by the names the group's `gives` declared. Values are the caller's: this crate carries
/// them and reads none of them, exactly as it carries a step's `run` payload.
pub type Handoff = BTreeMap<String, Value>;

/// Where a step sits, and what it is allowed to know.
///
/// # The context boundary, as the runner sees it
///
/// `scope` is the group whose conversation this step belongs to. Every step with the same `scope`
/// shares one context and stays warm; a step in a different scope starts from `available` and
/// nothing else.
///
/// `available` holds the handoffs of the sibling groups that finished before this one — **their
/// declared `gives`, never their transcripts.** That is the context rule falling out of the
/// structural one: a sibling cannot depend on a step inside a group, so it must not see that step's
/// conversation either.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StepContext {
    /// The group this step's conversation belongs to.
    pub scope: String,
    /// Which attempt of that scope this is, from 1.
    pub attempt: u32,
    /// What finished siblings handed over, by the names they promised.
    pub available: Handoff,
}

/// What a step did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    /// It ran and did what it was for.
    Passed,
    /// It ran and did not.
    ///
    /// A failure, not an error: the walk carries on and skips whatever needed this step, which is
    /// what a person would do. A workflow that could not represent a failed step would need one.
    Failed,
}

/// What a caller answers when the walk asks whether it may cross a section boundary.
///
/// # Why the walk asks, and why it does not decide
///
/// A workflow that can only be stopped by its own steps failing cannot be governed: there is
/// nowhere to say *not this section, not now*, and nowhere to say *no, do that again*. The two
/// moments a boundary is crossed are the only places where saying so means anything — inside a
/// section a run is one conversation, and interrupting one mid-turn is a different feature with a
/// different name.
///
/// **This crate evaluates no gate.** It asks, carries the answer's words, and turns a refusal into
/// one of the two things it already does: skip a section as failed, or re-enter it. Who is asked,
/// what they read to decide, and whether there is anybody to ask at all stays outside — the same
/// split that keeps a step's `run` payload opaque.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// Cross it.
    Proceed,
    /// Do not.
    Refused {
        /// The caller's own words, reported in [`crate::FlowEvent::TransitionRefused`] and read by
        /// nothing here.
        reason: String,
    },
}

/// What the caller does when a step comes up.
pub trait StepRunner {
    /// Runs one step. `path` is its position in the document — `root.shape.specify`.
    ///
    /// `context` says which conversation the step belongs to and what crossed into it. A runner
    /// that ignores it gets the old behaviour — one context for everything — which is a choice
    /// rather than an accident.
    fn run(&mut self, path: &str, step: &Step, context: &StepContext) -> StepOutcome;

    /// What a group hands its siblings as it leaves.
    ///
    /// Called once per group, after its last attempt, with the names the document promised. The
    /// default hands over nothing, which is correct for a runner that keeps one context: there is
    /// no boundary for anything to cross.
    ///
    /// A handoff missing a promised name fails the group, **once and without a retreat**. `gives`
    /// is a contract the document wrote down; a group that promised `specification_id` and produced
    /// nothing with that name has not finished, and letting its siblings run on would hand them a
    /// hole they cannot see. A second attempt would buy the same answer again: what is wrong is the
    /// section, not the run of it.
    fn handoff(&mut self, _scope: &str, _gives: &[NodeId]) -> Handoff {
        Handoff::new()
    }

    /// May this section run now? Asked once per attempt, before that attempt runs anything.
    ///
    /// The root is a group and is asked like one, so a caller that refuses it runs nothing at all.
    /// A [`Gate::Refused`] skips the section **as failed** — every step inside it is named as
    /// skipped, the group is left failed, and whatever needed it stops — because *it may not run*
    /// and *it ran and did not work* have the same consequence for everything downstream, and
    /// inventing a third outcome would make every reader of a record learn one more word.
    ///
    /// The default proceeds, which is a walk nobody is governing.
    fn entering(&mut self, _path: &str, _attempt: u32) -> Gate {
        Gate::Proceed
    }

    /// Is this attempt's result accepted? Asked once per attempt, **after** [`StepRunner::handoff`],
    /// so a caller decides having seen what the section is handing over.
    ///
    /// `failed` says whether the attempt already failed on its own. Refusing one that did changes
    /// nothing — it is recorded and that is all. Refusing one that came out **clean** marks it
    /// failed, which means the document's own [`crate::Repeat`] bound re-enters the section if it
    /// has an attempt left, and exhausts it if it does not. **That is how a caller forces a
    /// retreat**: not by a new verb, but by declining the result, with the notation deciding what
    /// happens next.
    ///
    /// The default proceeds, which is a walk nobody is governing.
    fn leaving(&mut self, _path: &str, _attempt: u32, _failed: bool, _handoff: &Handoff) -> Gate {
        Gate::Proceed
    }
}

/// What a walk did.
///
/// # Tallies and a verdict are different things
///
/// `ran`, `failed`, `skipped` and `retreats` are **cumulative over every attempt**: a group that
/// failed once and passed on its second attempt contributes the failure *and* the pass, because
/// both happened and a report that erased the first would be describing a run nobody had.
///
/// [`Report::clean`] is the **outcome** — did the flow come out clean in the end — and it is not
/// derived from the tallies. Deriving it was the bug this split fixes: a workflow that retreats and
/// then succeeds is a successful run, and a `clean()` that counted the failed attempt would call
/// every retreat a failure and make the whole feature look broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Report {
    /// Steps that ran, counting a repeated step once per attempt.
    pub ran: usize,
    /// Of those, how many failed.
    pub failed: usize,
    /// Steps that never ran, because something they needed failed.
    pub skipped: usize,
    /// How many times a group was re-entered.
    pub retreats: usize,
    /// Whether the flow came out clean.
    outcome_clean: bool,
}

impl Report {
    /// `true` when the flow came out clean, whatever it took to get there.
    pub fn clean(&self) -> bool {
        self.outcome_clean
    }
}

pub(crate) fn walk(
    root: &Group,
    plan: &Plan,
    runner: &mut dyn StepRunner,
    sink: &mut dyn FlowSink,
) -> Report {
    let steps = count_steps(root);
    sink.emit(FlowEvent::FlowStarted {
        flow: root.id.clone(),
        steps,
    });
    let mut report = Report::default();
    let failed = walk_group(root, plan, runner, sink, &mut report, &Handoff::new()).failed;
    report.outcome_clean = !failed;
    sink.emit(FlowEvent::FlowFinished {
        flow: root.id.clone(),
        ran: report.ran,
        failed: report.failed,
        skipped: report.skipped,
        retreats: report.retreats,
        clean: report.outcome_clean,
    });
    report
}

/// What a group leaves behind: whether it failed, and what it handed over.
///
/// Both halves reach the record — `GroupLeft` names what the section had — but only a group that
/// came out clean hands `handoff` to its siblings. See `attempt_group`.
struct Left {
    failed: bool,
    handoff: Handoff,
}

/// Walks one group, re-entering it while it does not come out clean and the document still allows
/// an attempt.
///
/// **This is the retreat.** The bound lives in the document rather than in the guard, and every
/// attempt re-runs the *whole* scope: a run that went back and then did not re-verify would have
/// skipped a check rather than retreated. See [`crate::Repeat`].
///
/// Each attempt is bracketed by the two gates of [`Gate`]: refused on the way in, the section is
/// left failed without running; refused on the way out, the attempt is failed and the bound above
/// decides whether that becomes a retreat or the end of it.
///
/// **A broken promise is the one failure no bound retries.** A section that did not produce what
/// its `gives` declared has not had a bad attempt, it is not the section the document describes —
/// see the break condition below.
///
/// Answers whether the group failed on its last attempt, which is what makes it opaque to its
/// siblings: they wait on *the group*, and one that did not come out clean stops whatever needed
/// it — and a section nobody allowed to run is failed in exactly that sense, so governing one costs
/// its siblings no new word.
fn walk_group(
    group: &Group,
    plan: &Plan,
    runner: &mut dyn StepRunner,
    sink: &mut dyn FlowSink,
    report: &mut Report,
    inherited: &Handoff,
) -> Left {
    let mut attempt = 1;
    // Written by whichever attempt turns out to be the last one, and read once, below it.
    let mut handoff;
    let mut broke_its_promise;
    let failed = loop {
        if let Gate::Refused { reason } = runner.entering(&plan.path, attempt) {
            return refused_entry(group, plan, sink, report, attempt, &reason);
        }

        let ran_badly = attempt_group(group, plan, runner, sink, report, attempt, inherited);
        let last = attempt >= plan.attempts;

        // The handoff is asked for on the attempt that is about to leave — one that came out clean,
        // or the last one the document allows. A group that retreated three times hands over what
        // it ended up with, not three drafts of it.
        (handoff, broke_its_promise) = if !ran_badly || last {
            ask_handoff(group, plan, runner, sink)
        } else {
            (Handoff::new(), false)
        };

        let mut failed = ran_badly || broke_its_promise;
        // Asked after the handoff, so whoever answers has seen what the section is handing over,
        // and once per attempt, so a retreat is a question and not a fait accompli.
        if let Gate::Refused { reason } = runner.leaving(&plan.path, attempt, failed, &handoff) {
            sink.emit(FlowEvent::TransitionRefused {
                path: plan.path.clone(),
                moment: Moment::Leave,
                attempt,
                reason,
            });
            // On an attempt that already failed there is nothing left to change and the refusal is
            // only recorded. On a clean one the refusal *is* the failure, and the retreat below is
            // the document's own answer to it.
            failed = true;
        }

        // A broken promise ends the section where it stands, whatever the bound says. `gives` is a
        // contract the *document* wrote down, and an attempt that came out clean and still did not
        // produce a promised name did not have a bad run — it is not doing what it says it does.
        // Re-entering it would buy the same answer again at full price; a caller who wants that
        // retreat has the leave gate, which is a decision somebody made rather than one the walk
        // took on its own.
        if !failed || last || broke_its_promise {
            break failed;
        }
        report.retreats += 1;
        sink.emit(FlowEvent::GroupRepeating {
            path: plan.path.clone(),
            attempt,
            of: plan.attempts,
        });
        attempt += 1;
    };

    sink.emit(FlowEvent::GroupLeft {
        path: plan.path.clone(),
        failed,
        attempts: attempt,
        exhausted: failed && attempt >= plan.attempts && plan.attempts > 1 && !broke_its_promise,
        gave: handoff.keys().cloned().collect(),
    });
    Left { failed, handoff }
}

/// Collects what a group promised its siblings, and says whether it broke that promise.
fn ask_handoff(
    group: &Group,
    plan: &Plan,
    runner: &mut dyn StepRunner,
    sink: &mut dyn FlowSink,
) -> (Handoff, bool) {
    if group.gives.is_empty() {
        return (Handoff::new(), false);
    }
    let handoff = runner.handoff(&plan.path, &group.gives);
    let missing: Vec<String> = group
        .gives
        .iter()
        .filter(|name| !handoff.contains_key(*name))
        .cloned()
        .collect();
    if missing.is_empty() {
        return (handoff, false);
    }
    sink.emit(FlowEvent::HandoffIncomplete {
        path: plan.path.clone(),
        missing,
    });
    (handoff, true)
}

/// A section nobody allowed to start, reported as the failed section it now is.
///
/// Every step inside is named, because *it never ran* is exactly what a reader of a green-looking
/// record must be able to see. No handoff is asked for: a section that did not run has nothing to
/// hand over, and asking would invite a runner to invent one.
fn refused_entry(
    group: &Group,
    plan: &Plan,
    sink: &mut dyn FlowSink,
    report: &mut Report,
    attempt: u32,
    reason: &str,
) -> Left {
    sink.emit(FlowEvent::TransitionRefused {
        path: plan.path.clone(),
        moment: Moment::Enter,
        attempt,
        reason: reason.to_owned(),
    });
    let because = format!("entering `{}` was refused: {reason}", plan.path);
    for node in &group.nodes {
        let path = format!("{}.{}", plan.path, node.id());
        report.skipped += skip(node, &path, &because, sink);
    }
    sink.emit(FlowEvent::GroupLeft {
        path: plan.path.clone(),
        failed: true,
        attempts: attempt,
        // Not exhausted: what stopped this section was a refusal, not a bound it used up.
        exhausted: false,
        gave: Vec::new(),
    });
    Left {
        failed: true,
        handoff: Handoff::new(),
    }
}

/// One pass over a group's layers.
fn attempt_group(
    group: &Group,
    plan: &Plan,
    runner: &mut dyn StepRunner,
    sink: &mut dyn FlowSink,
    report: &mut Report,
    attempt: u32,
    inherited: &Handoff,
) -> bool {
    sink.emit(FlowEvent::GroupEntered {
        path: plan.path.clone(),
        layers: plan.layers.len(),
        attempt,
        of: plan.attempts,
    });

    let mut broken: BTreeSet<NodeId> = BTreeSet::new();
    let mut any_failed = false;
    // Seeded from what crossed into this group, and grown by the sibling groups that finish inside
    // it. A step never sees another scope's conversation - only what that scope promised.
    let mut available = inherited.clone();

    for layer in &plan.layers {
        sink.emit(FlowEvent::LayerReady {
            path: plan.path.clone(),
            nodes: layer.nodes.clone(),
        });
        for id in &layer.nodes {
            let node = group
                .nodes
                .iter()
                .find(|node| node.id() == id)
                .expect("the plan names only nodes of this group");
            let path = format!("{}.{}", plan.path, id);

            // A node whose dependency did not come out clean does not run. The reason names the
            // dependency, because *why did this not run* is the question a reader has next.
            if let Some(blocker) = node.needs().iter().find(|need| broken.contains(*need)) {
                let because = format!("`{blocker}` did not finish clean");
                let skipped = skip(node, &path, &because, sink);
                report.skipped += skipped;
                broken.insert(id.clone());
                any_failed = true;
                continue;
            }

            match node {
                Node::Step(step) => {
                    sink.emit(FlowEvent::StepStarted { path: path.clone() });
                    let context = StepContext {
                        scope: plan.path.clone(),
                        attempt,
                        available: available.clone(),
                    };
                    let outcome = runner.run(&path, step, &context);
                    let failed = outcome == StepOutcome::Failed;
                    report.ran += 1;
                    if failed {
                        report.failed += 1;
                        broken.insert(id.clone());
                        any_failed = true;
                    }
                    sink.emit(FlowEvent::StepFinished { path, failed });
                }
                Node::Group(inner) => {
                    let inner_plan = plan
                        .groups
                        .get(id)
                        .expect("every child group was planned with its parent");
                    // `walk_group` emits this group's own `GroupLeft`, because only it knows how
                    // many attempts it took and what it managed to hand over.
                    let left = walk_group(inner, inner_plan, runner, sink, report, &available);
                    if left.failed {
                        broken.insert(id.clone());
                        any_failed = true;
                    } else {
                        // **Only a section that came out clean hands anything on.** What a failed
                        // one produced is in its own record — `GroupLeft.gave` says what it had —
                        // but it is a result nobody accepted, whether its steps failed or whoever
                        // was asked declined the leave. Letting it cross would build the rest of
                        // the walk on a value the same record calls failed, and a later section
                        // reading `specification_id` has no way to tell the two apart.
                        available.extend(left.handoff);
                    }
                }
            }
        }
    }

    any_failed
}

/// Reports a node — and everything inside it, if it is a group — as skipped, and answers how many
/// steps that was.
///
/// `because` is the whole sentence rather than a name: a node that never ran because a sibling
/// broke and one that never ran because nobody allowed its section to start are two different
/// facts, and the caller is the only one that knows which.
fn skip(node: &Node, path: &str, because: &str, sink: &mut dyn FlowSink) -> usize {
    sink.emit(FlowEvent::NodeSkipped {
        path: path.to_owned(),
        because: because.to_owned(),
    });
    match node {
        Node::Step(_) => 1,
        Node::Group(group) => {
            let mut total = 0;
            for inner in &group.nodes {
                let inner_path = format!("{path}.{}", inner.id());
                total += skip(inner, &inner_path, because, sink);
            }
            total
        }
    }
}

fn count_steps(group: &Group) -> usize {
    group
        .nodes
        .iter()
        .map(|node| match node {
            Node::Step(_) => 1,
            Node::Group(inner) => count_steps(inner),
        })
        .sum()
}
