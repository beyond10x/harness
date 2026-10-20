//! One catalogue of tools, published to every harness under three verbs, performed by the `tools`
//! repository's crates.
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
//! # Where the implementations come from (atlas ADR 0097, ADR 0095 phase T2)
//!
//! The default reading and writing of a tree are the `tools` repository's, pinned by tag: the `fs`
//! toolset ([`tool_fs`]) answers the reader's `file_read`, `dir_list` and `find`, and the
//! `code-editor` toolset ([`tool_code_editor`]) answers `file_write` and `file_edit`. The adapter
//! ([`LocalOperations`]) maps each harness operation onto one `tools` operation — the mapping is
//! [`tools_operation`], tested both ways — and keeps `search` and `run` here, because `tools`
//! `0.1.0` has no search or process toolset. The whole-file `sha256` a `tools` read now carries is
//! stripped by the adapter: it is a `tool-contract/1` property harness does not publish yet.
//!
//! # Three layers, and what each is allowed to know
//!
//! * [`Operations`] — who performs a tool. Two implementations: [`LocalOperations`] here, and a
//!   substrate-backed one in `harness-substrate`. They differ in what confines the effect and in
//!   whether *who asked* is answerable; the tools cannot tell them apart.
//! * [`Catalogue`] — what a run may do, built from what the provider admits. A provider that cannot
//!   write contributes no writing entry; a machine with no delegated cgroup contributes no `run`.
//!   The publication gate lives here, so a tool the model cannot have is one it is never told about.
//! * [`Verbs`] — the three tools, published to the model — or [`Flat`], the same catalogue
//!   published one tool per entry, with each entry's real schema. Both are a
//!   `harness_wire::ToolPort` over one catalogue; which one a run publishes is a deployment
//!   decision, and [`flat`](crate::Flat) carries what each costs.
//!
//! # The toolchain types come from `tool-toolchain`
//!
//! [`ResolvedProvider`] and the declarative toolchain registry are the `tools` crate's, moved there
//! with `harness-toolchain` (ADR 0097 decision 3). This crate re-exports the one type the catalogue
//! names and keeps no copy of its own.

mod catalogue;
mod flat;
mod local;
mod operations;
mod scope;
mod toolchain;
mod verbs;

pub use catalogue::{Catalogue, Entry, entry_names, operation_of, subjects_of};
pub use flat::Flat;
pub use harness_wire::{Refusal, Subject};
pub use local::{LocalOperations, harness_operation, tools_operation};
pub use operations::{Operations, ReadWindow, Refused, SearchOptions, Split};
pub use scope::{Scope, ScopeRule, WriteScope};
pub use tool_toolchain::ResolvedProvider;
pub use toolchain::entry_names as toolchain_entry_names;
pub use verbs::{DESCRIBE_VERB, INVOKE_VERB, SEARCH_VERB, Verbs};

#[cfg(test)]
mod tests;
