//! The catalogue's entries, published as themselves.
//!
//! # What the three verbs cost, measured rather than argued
//!
//! [`Verbs`](crate::Verbs) publishes `tool_search`, `tool_describe` and `tool_invoke` over whatever
//! the catalogue holds, so the names a run works in are ours on every harness. That was worth
//! having and it was never free. Across three live runs, **33% to 44% of every tool call was
//! `tool_search` or `tool_describe`** — four calls in ten spent finding out what exists, each one a
//! billed round trip that is replayed in every later turn and adds nothing to the tree. The
//! catalogue's own [`brief`](crate::Catalogue::brief) removed the *need* to ask by putting the
//! whole list in the standing instruction; it could not remove the second cost.
//!
//! The second cost is the schema. `tool_invoke.arguments` is an untyped `object`: the provider
//! cannot check that `file_read` was given a `path`, cannot fill in `offset`, and cannot refuse a
//! misspelled field before the call is billed. Every one of those becomes a failed tool call and
//! another turn.
//!
//! # And why publishing flat loses nothing the verbs bought
//!
//! The reason for the indirection was **neutral names across harnesses** — that a consumer can ask
//! *did this run write a file* without learning whether the harness spells it `Write`,
//! `workspace_write` or `apply_patch`. The entry names *are* that vocabulary: `file_read`,
//! `file_write`, `file_edit`, `dir_list`, `search`, `find`, `run`, each mapping to one neutral
//! operation through [`operation_of`](crate::operation_of), which a reader of a finished run can
//! apply without a catalogue. Publishing them directly keeps the vocabulary and hands the provider
//! the real per-tool schema.
//!
//! So both surfaces exist over one catalogue, and which one a run publishes is a deployment
//! decision. `Verbs` stays fully supported: metaharness serves it over MCP, where the three-verb
//! shape is the point.

use std::time::Duration;

use harness_wire::{Subject, ToolCall, ToolOutcome, ToolPort, ToolSpec};
use serde_json::Value;

use crate::Catalogue;

/// Every catalogue entry, as its own tool.
pub struct Flat {
    catalogue: Catalogue,
    specs: Vec<ToolSpec>,
}

impl std::fmt::Debug for Flat {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Flat")
            .field("catalogue", &self.catalogue)
            .finish_non_exhaustive()
    }
}

impl Flat {
    /// This catalogue's entries, published one tool each.
    ///
    /// The specs are taken once, at construction, because the catalogue is fixed for the life of a
    /// run: what a machine admits is decided before the first turn, and a toolset that could grow
    /// mid-run is a boundary a run could widen.
    pub fn new(catalogue: Catalogue) -> Self {
        let specs = catalogue.entries().iter().map(crate::Entry::spec).collect();
        Self { catalogue, specs }
    }

    /// What the catalogue holds, for a caller that wants to look without going through the model.
    pub fn catalogue(&self) -> &Catalogue {
        &self.catalogue
    }
}

impl ToolPort for Flat {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    fn operations(&self) -> Vec<&'static str> {
        self.catalogue.operations()
    }

    /// The subjects of the entry named, from its own arguments.
    ///
    /// No unwrapping here, unlike [`Verbs`](crate::Verbs): the call *is* the entry, so the
    /// arguments a gate reads are the ones the model sent. An unknown name touches nothing — the
    /// catalogue refuses it before anything runs.
    fn subjects(&self, call: &ToolCall) -> Vec<Subject> {
        self.catalogue
            .get(call.name.as_str())
            .map(|entry| entry.subjects(&call.arguments))
            .unwrap_or_default()
    }

    fn call(&mut self, call: &ToolCall) -> ToolOutcome {
        self.call_within(call, None)
    }

    fn call_within(&mut self, call: &ToolCall, remaining: Option<Duration>) -> ToolOutcome {
        match self
            .catalogue
            .invoke_within(call.name.as_str(), &call.arguments, remaining)
        {
            Ok(output) => ToolOutcome::ok(output),
            // Including the unknown name: `invoke_within` refuses it by name, listing every tool
            // this run has, which is the answer the model's next move needs.
            Err(message) => ToolOutcome::failed(message),
        }
    }

    /// Runs the batch side by side, one thread per call.
    ///
    /// The loop hands over only calls whose invoked envelope does not mutate; what makes them safe
    /// to run at once is that decision, not this function, and
    /// [`Catalogue::invoke_batch`] says so where it happens.
    fn call_batch(&mut self, calls: &[ToolCall], remaining: Option<Duration>) -> Vec<ToolOutcome> {
        let named: Vec<(&str, &Value)> = calls
            .iter()
            .map(|call| (call.name.as_str(), &call.arguments))
            .collect();
        self.catalogue
            .invoke_batch(&named, remaining)
            .into_iter()
            .map(|result| match result {
                Ok(output) => ToolOutcome::ok(output),
                Err(message) => ToolOutcome::failed(message),
            })
            .collect()
    }
}
