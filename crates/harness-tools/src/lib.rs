//! One catalogue of tools, published to every harness under three verbs.
//!
//! # The problem this exists to remove
//!
//! Every harness names its tools differently. Claude Code has `Bash`, `Read`, `Write`, `Edit`; the
//! b10x loop had `run`, `workspace_read`, `workspace_write`, `workspace_edit`; a Codex write travels
//! as `apply_patch` with the path inside a patch envelope. So everything downstream of a run had to
//! learn one vendor's vocabulary — and the evaluation corpus that judges four arms was written in
//! Claude Code's, which made it blind to every other harness. Widening it, one vendor name at a
//! time, puts *more* vendor names into a document that should hold none.
//!
//! The fix is upstream of the judge. One catalogue, named by metaharness's own neutral operations,
//! published under three verbs that are the same everywhere:
//!
//! ```text
//! tool_search   {query?, effect?}   -> the tools this run has
//! tool_describe {name}              -> one tool's arguments, effects, risk
//! tool_invoke   {name, arguments}   -> call it
//! ```
//!
//! # Three layers, and what each is allowed to know
//!
//! * [`Operations`] — who performs a tool. Two implementations: [`LocalOperations`] here, and a
//!   substrate-backed one in `harness-substrate`. They differ in what confines the effect and in
//!   whether *who asked* is answerable; the tools cannot tell them apart.
//! * [`Catalogue`] — what a run may do, built from what the provider admits. A provider that cannot
//!   write contributes no writing entry; a machine with no delegated cgroup contributes no `run`.
//!   The publication gate lives here, so a tool the model cannot have is one it is never told about.
//! * [`Verbs`] — the three tools, published to the model.
//!
//! # This crate depends on `harness-wire`, serde, and nothing else
//!
//! Deliberately. `metaharness` embeds it to serve the same tools to Claude Code over MCP, and that
//! workspace links no async runtime and justifies every dependency it has. A catalogue that dragged
//! in substrate or tokio could not go there.

mod catalogue;
mod local;
mod scope;
mod operations;
mod verbs;

pub use catalogue::{Catalogue, Entry, entry_names, operation_of, subjects_of};
pub use local::LocalOperations;
pub use scope::{Scope, ScopeRule, WriteScope};
pub use operations::{Operations, Split};
pub use verbs::{DESCRIBE_VERB, INVOKE_VERB, SEARCH_VERB, Verbs};
pub use harness_wire::Subject;

#[cfg(test)]
mod tests;
