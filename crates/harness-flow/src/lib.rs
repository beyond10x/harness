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
//! # Retreats, which a DAG cannot hold
//!
//! Real workflows go backwards: `adp/default/2` has three edges that do. A DAG has no back-edge and
//! this notation does not grow one — a retreat is **re-entering a scope**, and a scope is what a
//! group already is. [`Repeat`] is the whole feature: a group runs again while it did not come out
//! clean, up to a bound written in the document. Every level stays acyclic and the locality
//! argument above survives unchanged.
//!
//! # The shape
//!
//! ```yaml
//! id: development
//! root:
//!   id: root
//!   nodes:
//!     - id: receive
//!     - id: shape
//!       needs: [receive]
//!       nodes:
//!         - id: specify
//!         - id: decompose
//!           needs: [specify]
//!     - id: implement
//!       needs: [shape]
//! ```
//!
//! **A node that carries `nodes:` is a group; one that does not is a step.** The document's shape
//! says which, so nothing has to declare it twice and no keyword can disagree with the structure
//! underneath it.
//!
//! `implement` needs the whole `shape` sub-tree, not one of its steps: a group is opaque to its
//! siblings, which is what makes it substitutable.
//!
//! # A boundary is where a run can be told no
//!
//! A section is the only place a workflow can be governed without the scheduler growing opinions:
//! [`StepRunner::entering`] is asked before a group runs anything and [`StepRunner::leaving`] after
//! it has said what it hands over. Both may answer [`Gate::Refused`], and the walk turns that into
//! the two things it already knows how to do — skip a section as failed, or re-enter it. **The
//! reason is the caller's and this crate evaluates none of it**: what a governor is, and whether
//! there is one at all, stays outside.
//!
//! # What this crate does not do
//!
//! It does not run a model, hold a credential or know what a tool is. [`Flow::from_yaml`] and
//! [`Flow::from_json`] read a document without validating it; [`Flow::plan`] answers *what runs, in
//! what order, and what may run beside it*; [`Flow::run`] walks that plan against a [`StepRunner`]
//! the caller supplies. Binding a step to an actual turn of the loop is the caller's business,
//! which is what lets the whole scheduler be tested without a provider.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

mod event;
mod plan;
mod run;

pub use event::{FlowEvent, FlowSink, Moment, VecFlowSink};
pub use plan::{Layer, Plan};
pub use run::{Gate, Handoff, Report, StepContext, StepOutcome, StepRunner};

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
    /// Run this group again while it does not come out clean.
    ///
    /// Absent means once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<Repeat>,
    /// What this group promises to hand its siblings when it leaves.
    ///
    /// # This is the context boundary, written down
    ///
    /// A sibling already cannot depend on a step *inside* a group — an edge may only join
    /// siblings. The symmetric statement is the context rule: **if a sibling cannot depend on a
    /// step inside a group, it must not see that step's transcript either.** So a group is a
    /// context scope. Its steps share one conversation and stay warm; what crosses the boundary is
    /// this list and nothing else.
    ///
    /// It is declared rather than inferred because the alternative is *whatever the model happened
    /// to say last*, which is not a contract. A group that names `specification_id` and hands over
    /// something without one has broken a promise the document made, and the walk says so.
    ///
    /// The cost this exists to remove is measurable. The first driven evaluation ran one cold
    /// session per workflow state and spent 14.0M tokens where a single-session arm spent 4.6M for
    /// the same deliverable — 3.4x, and the difference is six cold starts rather than six units of
    /// work. Under scopes only the boundary pays.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gives: Vec<NodeId>,
    /// The nodes inside. An empty group is refused: a section that runs nothing is a mistake
    /// somebody made, not a section.
    pub nodes: Vec<Node>,
}

/// How many times a group may be re-entered when it does not come out clean.
///
/// # This is how a retreat is written down
///
/// A real workflow has them. `adp/default/2` has three — `verify -> implement`,
/// `adversarial_verify -> implement`, `review -> implement` — and its own header argues why: a
/// workflow that can only go forwards is a lie about how engineering works, and without a route
/// back the only ways out are to weaken the check or to declare victory anyway.
///
/// A DAG cannot hold a back-edge, so the notation does not grow one. **A retreat is re-entering a
/// scope, and a scope is what a group already is**: `implement -> verify -> adversarial_verify`
/// becomes one group that repeats until it comes out clean. Every level stays acyclic, the whole
/// locality argument in this module's documentation survives intact, and the thing that stops an
/// infinite retreat is a number in the document rather than an accident of the guard.
///
/// A back-edge would have bought one thing this does not: retreating to a *point* rather than to
/// the start of a scope. That is deliberate. A run that goes back to `implement` and then does not
/// re-verify has not retreated, it has skipped a check — which is exactly the failure the
/// workflow's own comment names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repeat {
    /// Total attempts, including the first. `1` is the same as no `repeat` at all.
    pub max: u32,
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

/// A document that could not be read at all.
///
/// # Reading and validating are two different refusals
///
/// This one is *these bytes are not a document*: a tab where YAML wanted a space, a trailing comma
/// in JSON, `nodes` given as a string. [`FlowError`] is *this document is not a workflow*: a cycle,
/// an edge that reaches into a group, a section that runs nothing. Parsing does not validate —
/// [`Flow::plan`] does — because a caller that wants to hold a document before deciding to run it
/// (a `plan` verb, a linter, an editor) should not have to catch the second error to discover the
/// first.
///
/// It names the format because the same bytes are refused differently by each reader, and carries
/// the parser's own message because that message knows the line and column and this crate does not.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("this is not a workflow document in {format}: {message}")]
pub struct ParseError {
    /// What it was read as — `YAML` or `JSON`.
    pub format: &'static str,
    /// The reader's own words, unedited.
    pub message: String,
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
    #[error(
        "`{path}` may repeat at most {max} times: a group that may run zero times is a group that \
         is not there. Delete it, or say `max: 1`"
    )]
    RepeatsNever { path: String, max: u32 },
    #[error(
        "`{path}` is the root and promises `{gives}` to nobody: a handoff crosses a boundary, and \
         the root has no sibling on the other side of it"
    )]
    RootGives { path: String, gives: String },
}

impl Flow {
    /// Reads a document written in YAML. **Does not validate it** — [`Flow::plan`] does.
    ///
    /// This lives here rather than in whatever runs the flow, so that every caller reads the same
    /// notation the same way. A CLI that parsed a document itself would be a second description of
    /// the format, and the two would drift the first time a field was added.
    ///
    /// # Errors
    ///
    /// [`ParseError`] naming YAML and carrying the reader's own message, line and column.
    pub fn from_yaml(text: &str) -> Result<Self, ParseError> {
        serde_yaml_ng::from_str(text).map_err(|error| ParseError {
            format: "YAML",
            message: error.to_string(),
        })
    }

    /// Reads a document written in JSON. **Does not validate it** — [`Flow::plan`] does.
    ///
    /// The same notation, read by the other reader: YAML is what a person writes and JSON is what
    /// another program emits, and a workflow that could only arrive one way would push whoever
    /// generates one into writing a YAML serialiser.
    ///
    /// # Errors
    ///
    /// [`ParseError`] naming JSON and carrying the reader's own message, line and column.
    pub fn from_json(text: &str) -> Result<Self, ParseError> {
        serde_json::from_str(text).map_err(|error| ParseError {
            format: "JSON",
            message: error.to_string(),
        })
    }

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
    ) -> Result<Report, FlowError> {
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
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
