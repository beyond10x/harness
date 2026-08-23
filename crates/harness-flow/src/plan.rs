//! What runs, in what order, and what may run beside it.
//!
//! One plan per group, nested the way the document is. A layer is a set of siblings whose
//! dependencies are all satisfied, so everything in a layer may run at once — which is a claim
//! about *this* group only, and needs no knowledge of any other.

use std::collections::BTreeMap;

use crate::{FlowError, Group, Node, NodeId, check_edges, index};

/// A set of siblings that may run together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    /// Their names, in declaration order — so a run is reproducible rather than hash-ordered.
    pub nodes: Vec<NodeId>,
}

/// The plan for one group: its layers, and a plan for each sub-tree inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// This group's path from the root, for messages.
    pub path: String,
    /// Its layers, in the order they run.
    pub layers: Vec<Layer>,
    /// How many times this group may run. `1` unless the document says otherwise.
    pub attempts: u32,
    /// A plan per child group, by name.
    pub groups: BTreeMap<NodeId, Plan>,
}

impl Plan {
    /// How many layers deep this group is, ignoring its children.
    pub fn depth(&self) -> usize {
        self.layers.len()
    }

    /// The widest layer in this group — how much can run at once here.
    pub fn width(&self) -> usize {
        self.layers.iter().map(|layer| layer.nodes.len()).max().unwrap_or(0)
    }
}

pub(crate) fn plan(root: &Group) -> Result<Plan, FlowError> {
    plan_group(root, &root.id)
}

fn plan_group(group: &Group, path: &str) -> Result<Plan, FlowError> {
    let siblings = index(group, path)?;
    check_edges(group, path, &siblings)?;

    let attempts = match group.repeat {
        None => 1,
        Some(repeat) if repeat.max >= 1 => repeat.max,
        Some(repeat) => {
            return Err(FlowError::RepeatsNever {
                path: path.to_owned(),
                max: repeat.max,
            });
        }
    };

    let layers = layers_of(group, path, &siblings)?;

    let mut groups = BTreeMap::new();
    for node in &group.nodes {
        if let Node::Group(inner) = node {
            let inner_path = format!("{path}.{}", inner.id);
            groups.insert(inner.id.clone(), plan_group(inner, &inner_path)?);
        }
    }

    Ok(Plan {
        path: path.to_owned(),
        layers,
        attempts,
        groups,
    })
}

/// Kahn's algorithm over one group's siblings, emitting whole layers.
///
/// Layers rather than a linear order because the useful answer is *what may run together*: a caller
/// that wants one node at a time can take them one at a time, and a caller that wants concurrency
/// cannot recover it from a flattened list.
fn layers_of(
    group: &Group,
    path: &str,
    siblings: &BTreeMap<NodeId, usize>,
) -> Result<Vec<Layer>, FlowError> {
    let mut remaining: BTreeMap<&str, Vec<&str>> = group
        .nodes
        .iter()
        .map(|node| {
            (
                node.id(),
                node.needs().iter().map(String::as_str).collect::<Vec<_>>(),
            )
        })
        .collect();

    let mut layers: Vec<Layer> = Vec::new();
    let mut done: Vec<&str> = Vec::new();

    while !remaining.is_empty() {
        let mut ready: Vec<&str> = remaining
            .iter()
            .filter(|(_, needs)| needs.iter().all(|need| done.contains(need)))
            .map(|(id, _)| *id)
            .collect();

        if ready.is_empty() {
            // Everything left is waiting on something else that is left: that is the cycle.
            let mut stuck: Vec<&str> = remaining.keys().copied().collect();
            stuck.sort_by_key(|id| siblings.get(*id).copied().unwrap_or(usize::MAX));
            return Err(FlowError::Cycle {
                path: path.to_owned(),
                cycle: stuck.join(" -> "),
            });
        }

        // Declaration order, so two runs of the same document produce the same layer.
        ready.sort_by_key(|id| siblings.get(*id).copied().unwrap_or(usize::MAX));
        for id in &ready {
            remaining.remove(id);
        }
        done.extend(ready.iter().copied());
        layers.push(Layer {
            nodes: ready.into_iter().map(ToOwned::to_owned).collect(),
        });
    }

    Ok(layers)
}
