//! Walking a plan.
//!
//! The walk knows about order and failure and nothing else. What a step *is* — a turn of the loop,
//! a tool call, a command — is the caller's, behind [`StepRunner`], which is what lets the whole
//! scheduler be tested without a provider, a credential or a network.

use std::collections::BTreeSet;

use crate::{FlowEvent, FlowSink, Group, Node, NodeId, Plan, Step};

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
    fn run(&mut self, path: &str, step: &Step) -> StepOutcome;
}

/// What a walk did, in totals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Report {
    /// Steps that ran.
    pub ran: usize,
    /// Of those, how many failed.
    pub failed: usize,
    /// Steps that never ran, because something they needed failed.
    pub skipped: usize,
}

impl Report {
    /// `true` when every step that ran passed and none was skipped.
    pub fn clean(&self) -> bool {
        self.failed == 0 && self.skipped == 0
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
    let failed = walk_group(root, plan, runner, sink, &mut report);
    sink.emit(FlowEvent::FlowFinished {
        flow: root.id.clone(),
        ran: report.ran,
        failed: report.failed,
        skipped: report.skipped,
    });
    let _ = failed;
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
fn walk_group(
    group: &Group,
    plan: &Plan,
    runner: &mut dyn StepRunner,
    sink: &mut dyn FlowSink,
    report: &mut Report,
) -> bool {
    let mut attempt = 1;
    loop {
        let failed = attempt_group(group, plan, runner, sink, report, attempt);
        if !failed || attempt >= plan.attempts {
            sink.emit(FlowEvent::GroupLeft {
                path: plan.path.clone(),
                failed,
                attempts: attempt,
                exhausted: failed && attempt >= plan.attempts && plan.attempts > 1,
            });
            return failed;
        }
        sink.emit(FlowEvent::GroupRepeating {
            path: plan.path.clone(),
            attempt,
            of: plan.attempts,
        });
        attempt += 1;
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
) -> bool {
    sink.emit(FlowEvent::GroupEntered {
        path: plan.path.clone(),
        layers: plan.layers.len(),
        attempt,
        of: plan.attempts,
    });

    let mut broken: BTreeSet<NodeId> = BTreeSet::new();
    let mut any_failed = false;

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
                    let outcome = runner.run(&path, step);
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
                    // many attempts it took.
                    if walk_group(inner, inner_plan, runner, sink, report) {
                        broken.insert(id.clone());
                        any_failed = true;
                    }
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
