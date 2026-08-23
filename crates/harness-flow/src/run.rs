//! Walking a plan.
//!
//! The walk knows about order and failure and nothing else. What a step *is* — a turn of the loop,
//! a tool call, a command — is the caller's, behind [`StepRunner`], which is what lets the whole
//! scheduler be tested without a provider, a credential or a network.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::{FlowEvent, FlowSink, Group, Node, NodeId, Plan, Step};

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
    /// A handoff missing a promised name fails the group. `gives` is a contract the document wrote
    /// down; a group that promised `specification_id` and produced nothing with that name has not
    /// finished, and letting its siblings run on would hand them a hole they cannot see.
    fn handoff(&mut self, _scope: &str, _gives: &[NodeId]) -> Handoff {
        Handoff::new()
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

/// Walks one group, re-entering it while it does not come out clean and the document still allows
/// an attempt.
///
/// **This is the retreat.** The bound lives in the document rather than in the guard, and every
/// attempt re-runs the *whole* scope: a run that went back and then did not re-verify would have
/// skipped a check rather than retreated. See [`crate::Repeat`].
///
/// Answers whether the group failed on its last attempt, which is what makes it opaque to its
/// siblings: they wait on *the group*, and one that did not come out clean stops whatever needed it.
/// What a group leaves behind: whether it failed, and what it handed over.
struct Left {
    failed: bool,
    handoff: Handoff,
}

fn walk_group(
    group: &Group,
    plan: &Plan,
    runner: &mut dyn StepRunner,
    sink: &mut dyn FlowSink,
    report: &mut Report,
    inherited: &Handoff,
) -> Left {
    let mut attempt = 1;
    let failed = loop {
        let failed = attempt_group(group, plan, runner, sink, report, attempt, inherited);
        if !failed || attempt >= plan.attempts {
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

    // The handoff is asked for once, after the last attempt: a group that retreated three times
    // hands over what it ended up with, not three drafts of it.
    let mut handoff = Handoff::new();
    let mut broke_its_promise = false;
    if !group.gives.is_empty() {
        handoff = runner.handoff(&plan.path, &group.gives);
        let missing: Vec<String> = group
            .gives
            .iter()
            .filter(|name| !handoff.contains_key(*name))
            .cloned()
            .collect();
        if !missing.is_empty() {
            broke_its_promise = true;
            sink.emit(FlowEvent::HandoffIncomplete {
                path: plan.path.clone(),
                missing,
            });
        }
    }

    let failed = failed || broke_its_promise;
    sink.emit(FlowEvent::GroupLeft {
        path: plan.path.clone(),
        failed,
        attempts: attempt,
        exhausted: failed && attempt >= plan.attempts && plan.attempts > 1 && !broke_its_promise,
        gave: handoff.keys().cloned().collect(),
    });
    Left { failed, handoff }
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
                let skipped = skip(node, &path, blocker, sink);
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
                    }
                    available.extend(left.handoff);
                }
            }
        }
    }

    any_failed
}

/// Reports a node — and everything inside it, if it is a group — as skipped, and answers how many
/// steps that was.
fn skip(node: &Node, path: &str, because: &str, sink: &mut dyn FlowSink) -> usize {
    sink.emit(FlowEvent::NodeSkipped {
        path: path.to_owned(),
        because: format!("`{because}` did not finish clean"),
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
