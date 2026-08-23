//! A workflow notation the loop runs natively: **a DAG of sub-trees**.
//!
//! # Why sub-trees and not one flat graph
//!
//! A flat node-and-edge list can express everything this can, and that is the problem. In a flat
//! graph any node may point at any other, so nothing is local: to know whether a change to one
//! part of a workflow can affect another you have to read the whole edge list. Every property this
//! crate wants — *is it acyclic*, *what may run in parallel*, *what does this part need* — becomes
//! a question about the entire document.
//!
//! Here a node is either a **step** (one thing that runs) or a **group** (a sub-tree that holds its
//! own nodes and its own edges). **An edge may only join siblings.** That single restriction buys
//! the rest:
//!
//! * **Acyclicity is checked once per group, over a handful of nodes.** Cross-level cycles cannot
//!   be written down, because there is no syntax for an edge that leaves a group.
//! * **A group is a unit of reuse.** It depends on what its own `needs` names and nothing else, so
//!   the same sub-tree runs unchanged inside another workflow. A flat graph has no such boundary —
//!   copying part of it means copying the edges that reach into it.
//! * **A group is a unit of scope.** Anything a run wants to bound per section — a toolset, a
//!   budget, an approval policy — has an obvious place to hang, because *this section* is a node.
//!   In a flat graph "this section" is a set of node ids somebody has to keep correct by hand.
//! * **A group is a unit of reporting.** The event stream nests, so a reader sees *specify finished,
//!   verify started* rather than nineteen step ids they have to reassemble.
//!
//! What it costs: a diamond that spans two groups cannot be written as one edge. That is the
//! intended trade. Where two sections genuinely interleave, they are one section — and having to
//! say so is the point, because a workflow whose parts reach into each other's middles is one
//! nobody can reason about in pieces either.
//!
//! # The shape
//!
//! ```yaml
//! id: development
//! root:
//!   id: root
//!   nodes:
//!     - step: receive
//!     - group: shape
//!       needs: [receive]
//!       nodes:
//!         - step: specify
//!         - step: decompose
//!           needs: [specify]
//!     - step: implement
//!       needs: [shape]
//! ```
//!
//! `implement` needs the whole `shape` sub-tree, not one of its steps: a group is opaque to its
//! siblings, which is what makes it substitutable.
//!
//! # What this crate does not do
//!
//! It does not run a model, hold a credential or know what a tool is. [`Flow::plan`] answers *what
//! runs, in what order, and what may run beside it*; [`Flow::run`] walks that plan against a
//! [`StepRunner`] the caller supplies. Binding a step to an actual turn of the loop is the caller's
//! business, which is what lets the whole scheduler be tested without a provider.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

mod event;
mod plan;
mod run;

pub use event::{FlowEvent, FlowSink, VecFlowSink};
pub use plan::{Layer, Plan};
pub use run::{StepOutcome, StepRunner};

/// Names one node. Unique among its siblings, and a path from the root names it globally.
pub type NodeId = String;

/// A validated workflow: a root group, and nothing outside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Flow {
    /// What this workflow is called.
    pub id: String,
    /// The root sub-tree. A flow is a group, which is why a flow can be a node of another flow.
    pub root: Group,
}

/// A sub-tree: its own nodes, and edges that may only join them to each other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    /// Names this group among its siblings.
    pub id: NodeId,
    /// Siblings that must finish before this group starts.
    #[serde(default)]
    pub needs: Vec<NodeId>,
    /// The nodes inside. An empty group is refused: a section that runs nothing is a mistake
    /// somebody made, not a section.
    pub nodes: Vec<Node>,
}

/// One thing that runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// Names this step among its siblings.
    pub id: NodeId,
    /// Siblings that must finish before this step starts.
    #[serde(default)]
    pub needs: Vec<NodeId>,
    /// What the caller is meant to do here, passed through untouched.
    ///
    /// This crate never reads it. Keeping the payload opaque is what stops the scheduler growing
    /// opinions about prompts, tools and models, which is the whole reason the loop is not in here.
    #[serde(default)]
    pub run: serde_json::Value,
}

/// A node of a sub-tree: something that runs, or a sub-tree of things that run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Node {
    /// A sub-tree. Listed first so a document carrying `nodes:` parses as one.
    Group(Group),
    /// A leaf.
    Step(Step),
}

impl Node {
    /// The node's own name.
    pub fn id(&self) -> &str {
        match self {
            Self::Group(group) => &group.id,
            Self::Step(step) => &step.id,
        }
    }

    /// The siblings this node waits for.
    pub fn needs(&self) -> &[NodeId] {
        match self {
            Self::Group(group) => &group.needs,
            Self::Step(step) => &step.needs,
        }
    }

    /// `true` for a sub-tree.
    pub fn is_group(&self) -> bool {
        matches!(self, Self::Group(_))
    }
}

/// Every way a document can fail to be a workflow.
///
/// Each names a path — `root.shape.specify` — because a message that says only *cycle detected* in
/// a nested document sends the reader to search for it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FlowError {
    #[error("`{path}` holds no nodes: a section that runs nothing is not a section")]
    EmptyGroup { path: String },
    #[error("`{path}` declares `{id}` twice: a name must be unique among its siblings")]
    DuplicateId { path: String, id: NodeId },
    #[error(
        "`{path}` needs `{missing}`, which is not one of its siblings. An edge may only join \
         siblings — to depend on something inside another group, depend on the group"
    )]
    UnknownNeed { path: String, missing: NodeId },
    #[error("`{path}` needs itself")]
    SelfNeed { path: String },
    #[error("`{path}` holds a cycle through: {cycle}")]
    Cycle { path: String, cycle: String },
    #[error("`{path}` is empty: a flow with no id names nothing")]
    NoId { path: String },
}

impl Flow {
    /// Validates the document and answers what runs when.
    ///
    /// # Errors
    ///
    /// Returns the first [`FlowError`] found, naming the path it was found at. Validation is
    /// depth-first in declaration order, so the error a reader gets is the earliest one in the
    /// document they are looking at.
    pub fn plan(&self) -> Result<Plan, FlowError> {
        if self.id.trim().is_empty() {
            return Err(FlowError::NoId {
                path: "<flow>".to_owned(),
            });
        }
        plan::plan(&self.root)
    }

    /// Runs the plan against a caller's runner, reporting through a sink.
    ///
    /// # Errors
    ///
    /// Returns a [`FlowError`] when the document does not validate. A step that *fails* is not an
    /// error here — it is a [`StepOutcome`] the walk acts on, because a workflow that could not
    /// represent a failed step would need one anyway.
    pub fn run(
        &self,
        runner: &mut dyn StepRunner,
        sink: &mut dyn FlowSink,
    ) -> Result<run::Report, FlowError> {
        let plan = self.plan()?;
        Ok(run::walk(&self.root, &plan, runner, sink))
    }

    /// Every step in the flow, by path, in declaration order.
    pub fn steps(&self) -> Vec<String> {
        let mut found = Vec::new();
        collect_steps(&self.root, &self.root.id, &mut found);
        found
    }
}

fn collect_steps(group: &Group, prefix: &str, found: &mut Vec<String>) {
    for node in &group.nodes {
        let path = format!("{prefix}.{}", node.id());
        match node {
            Node::Step(_) => found.push(path),
            Node::Group(inner) => collect_steps(inner, &path, found),
        }
    }
}

/// The siblings of one group, indexed by name, with the order they were declared in.
pub(crate) fn index(group: &Group, path: &str) -> Result<BTreeMap<NodeId, usize>, FlowError> {
    if group.nodes.is_empty() {
        return Err(FlowError::EmptyGroup {
            path: path.to_owned(),
        });
    }
    let mut seen: BTreeMap<NodeId, usize> = BTreeMap::new();
    for (position, node) in group.nodes.iter().enumerate() {
        if seen.insert(node.id().to_owned(), position).is_some() {
            return Err(FlowError::DuplicateId {
                path: path.to_owned(),
                id: node.id().to_owned(),
            });
        }
    }
    Ok(seen)
}

/// Checks that every edge in this group joins two of its own nodes.
pub(crate) fn check_edges(
    group: &Group,
    path: &str,
    siblings: &BTreeMap<NodeId, usize>,
) -> Result<(), FlowError> {
    for node in &group.nodes {
        let here = format!("{path}.{}", node.id());
        let mut named: BTreeSet<&str> = BTreeSet::new();
        for need in node.needs() {
            if need == node.id() {
                return Err(FlowError::SelfNeed { path: here });
            }
            if !siblings.contains_key(need) {
                return Err(FlowError::UnknownNeed {
                    path: here,
                    missing: need.clone(),
                });
            }
            named.insert(need.as_str());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
