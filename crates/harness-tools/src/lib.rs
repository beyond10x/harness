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
//! * [`Verbs`] — the three tools, published to the model — or [`Flat`], the same catalogue
//!   published one tool per entry, with each entry's real schema. Both are a
//!   `harness_wire::ToolPort` over one catalogue; which one a run publishes is a deployment
//!   decision, and [`flat`](crate::Flat) carries what each costs.
//!
//! # This crate depends on `harness-wire`, serde, `regex`, `globset`, and nothing else
//!
//! The short list is deliberate: `metaharness` embeds this crate to serve the same tools to Claude
//! Code over MCP, and that workspace links no async runtime and justifies every dependency it has.
//! A catalogue that dragged in substrate or tokio could not go there.
//!
//! The two matching crates were added on 2026-08-29 with `search`'s `regex` and `glob`, and they
//! are the two this workspace would otherwise have written by hand: a glob matcher of our own would
//! be a second, subtly different answer to *does this path match `crates/**/*.rs`* on every machine
//! that reads a run, and nobody should write a regular-expression engine twice. Both are the
//! crates ripgrep itself matches with, by the same author, and they share `regex-automata` and
//! `regex-syntax` — so the second costs `bstr` and `log` rather than a tree of its own. Neither
//! links a runtime.

mod catalogue;
mod flat;
mod local;
mod operations;
mod scope;
mod toolchain;
mod verbs;

pub use catalogue::{Catalogue, Entry, entry_names, operation_of, subjects_of};
pub use flat::Flat;
pub use harness_toolchain::ResolvedProvider;
pub use harness_wire::{Refusal, Subject};
pub use local::LocalOperations;
pub use operations::{Operations, ReadWindow, Refused, SearchOptions, Split};
pub use scope::{Scope, ScopeRule, WriteScope};
pub use toolchain::entry_names as toolchain_entry_names;
pub use verbs::{DESCRIBE_VERB, INVOKE_VERB, SEARCH_VERB, Verbs};

#[cfg(test)]
mod tests;
