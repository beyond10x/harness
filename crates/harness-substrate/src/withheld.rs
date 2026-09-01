//! What a run asked for and this machine would not admit, said out loud.
//!
//! # The silence this closes
//!
//! Publication is a gate that works by **absence** ([`crate::tools`]): a tool the machine cannot
//! confine is not published, so the model never plans around it. That is right for the model and
//! wrong for everybody else. An absence is indistinguishable from a run that never wanted the tool,
//! and on 2026-08-29 the difference cost weeks: a driven session whose only legal route was running
//! a program was published a six-entry catalogue instead of seven — no error, no warning, no fact in
//! the record — hand-wrote files instead, and the failure read as a model failure.
//!
//! What was missing is not a refusal. Refusing would put the tool back in front of the model, which
//! is the thing publication exists to avoid. What was missing is the **fact**: *execution was asked
//! for and this machine could not admit it, and here is the predicate that decided.*
//!
//! # Declared, and only declared
//!
//! A machine that was asked for nothing withholds nothing (`AGENTS.md` invariant 7: absence stays
//! absence). `programs` empty is a run that wanted no execution, and reporting a withheld `run`
//! there would be inventing a want. The record is a function of what was **declared** against what
//! the machine **states**, and of nothing else.
//!
//! # Why the reason carries a hint about cgroups
//!
//! The core confinement facts are reported by substrate's probe as a block — `exec.argv-only`,
//! `exec.cgroup-limits` and the rest are all `exec.then_some(…)` in `substrate-host`'s
//! `probe::probe` — and the term of that conjunction that fails on a developer machine is
//! `probe_cgroup`, which reads the probing process's own `/proc/self/cgroup`. A login shell sits in
//! `user.slice/user-N.slice/session-M.scope`, a **sibling** of the `user@N.service` manager scope a
//! delegated root usually lives under, so the same binary against the same machine reports exec
//! true under `systemd-run --user --scope` and absent from a shell. A reason that named only the
//! absent fact would send a reader looking at substrate's configuration for a fault that is in how
//! the harness was started.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Facts;

/// The catalogue entry execution is published as.
const RUN: &str = "run";

/// The catalogue entries a guarded workspace is published as.
const WRITING: [&str; 2] = ["file_write", "file_edit"];

/// The three limits `exec.cgroup-limits` has to state, in the order a reason lists them.
const CGROUP_LIMITS: [&str; 3] = ["cpu", "memory", "processes"];

/// Why the exec facts are usually absent when a person expected them, in one line.
///
/// Named in every execution reason a stated fact produced, and left off the *no daemon answered*
/// one — there the cgroup is not the question, because nothing probed anything.
const CGROUP_HINT: &str = "substrate states the exec facts only where its own cgroup probe passed, \
                           and that probe reads the probing process's `/proc/self/cgroup` and \
                           fails when it is outside the configured cgroup root — the embedded \
                           driver probes *this* process, and a login shell sits in \
                           `user.slice/user-N.slice/session-M.scope`, a sibling of the \
                           `user@N.service` manager scope, so the same machine answers differently \
                           under `systemd-run --user --scope`.";

/// A tool this run declared and this machine does not admit.
///
/// Carried beside the published toolset rather than instead of it. The tool is still absent from
/// what the model sees — that is the publication gate doing its job — and this is the record for
/// everyone reading the run afterwards: a person at a terminal, the `--json` stream, and
/// `b10x-harness tools`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Withheld {
    /// The catalogue entry that does not exist here — `run`, `file_write`, `file_edit`.
    ///
    /// The entry's own name and never the surface's, for the same reason an approval names the
    /// entry: a reader decides about `run`, never about `tool_invoke`.
    pub tool: String,
    /// The predicate that failed, as the machine stated it.
    ///
    /// Written in the vocabulary [`crate::Unmet`] uses — *must be X and this machine says Y*, with
    /// `nothing` for a fact the machine never stated — so a reader who has seen one refusal can
    /// read the other.
    pub reason: String,
}

/// What the machine said about one fact, or `nothing` when it said nothing.
///
/// The same word [`crate::Predicate::check`] uses, because *the machine did not say* and *the
/// machine said no* are different answers and a reader has to be able to tell them apart.
fn describe(value: Option<&Value>) -> String {
    value.map_or_else(|| "nothing".to_owned(), ToString::to_string)
}

impl Facts {
    /// Every tool this run declared that this machine will not admit, with the reason.
    ///
    /// `programs` is the declared program set — empty is a run that asked for no execution, and it
    /// withholds nothing. `writes_wanted` is whether a confined workspace was asked for at all: a
    /// read-only run was never going to get the writing entries and is not owed a sentence about
    /// them.
    ///
    /// Empty for a machine that admits everything that was asked for, which is the common case and
    /// the one that must stay silent.
    #[must_use]
    pub fn withheld(&self, programs: &[String], writes_wanted: bool) -> Vec<Withheld> {
        let mut withheld = Vec::new();
        if !programs.is_empty() && !self.confines_execution() {
            withheld.push(Withheld {
                tool: RUN.to_owned(),
                reason: self.execution_reason(),
            });
        }
        if writes_wanted && !self.holds_workspaces() {
            let reason = self.workspace_reason();
            withheld.extend(WRITING.iter().map(|tool| Withheld {
                tool: (*tool).to_owned(),
                reason: reason.clone(),
            }));
        }
        withheld
    }

    /// Which predicate stopped `run` existing here, in the machine's own terms.
    fn execution_reason(&self) -> String {
        if self.facts.is_empty() {
            return "this machine states no capability facts at all — no substrate daemon answered, \
                    or none was asked for — so `exec.argv-only` is absent and nothing that needs \
                    it is published here."
                .to_owned();
        }
        let argv_only = self.get("exec.argv-only");
        if argv_only != Some(&Value::Bool(true)) {
            return format!(
                "`exec.argv-only` must be true and this machine says {}. {CGROUP_HINT}",
                describe(argv_only)
            );
        }
        let limits = self.get("exec.cgroup-limits");
        let missing: Vec<&str> = CGROUP_LIMITS
            .iter()
            .filter(|key| {
                limits
                    .and_then(Value::as_object)
                    .and_then(|limits| limits.get(**key))
                    != Some(&Value::Bool(true))
            })
            .copied()
            .collect();
        if !missing.is_empty() {
            return format!(
                "`exec.cgroup-limits` must state `cpu`, `memory` and `processes` true and this \
                 machine says {} — no `{}`. {CGROUP_HINT}",
                describe(limits),
                missing.join("`, no `")
            );
        }
        let resource_usage = self.get("exec.resource-usage");
        format!(
            "`exec.resource-usage` must be an object because this client requests that measurement \
             on every run, and this machine says {}. substrate withholds the fact until every \
             required cgroup v2 counter, including block I/O, is available.",
            describe(resource_usage)
        )
    }

    /// Which predicate stopped the writing entries existing here.
    ///
    /// No cgroup hint: workspaces are served on a machine that cannot confine a process, so the two
    /// absences have different causes and one sentence about the second would be a guess.
    fn workspace_reason(&self) -> String {
        if self.facts.is_empty() {
            return "this machine states no capability facts at all — no substrate daemon answered, \
                    or none was asked for — so `workspace.guarded-io` is absent and nothing \
                    published here can change a file."
                .to_owned();
        }
        format!(
            "`workspace.guarded-io` must be true and this machine says {}.",
            describe(self.get("workspace.guarded-io"))
        )
    }
}
