//! What this machine can confine, and which tools may therefore exist.
//!
//! # The one property this crate is built on
//!
//! Substrate **refuses rather than degrading**. Its own README:
//!
//! > *"Without a delegated cgroup root, workspace operations remain served and exec confinement
//! > facts are absent, so exec admission answers `exec.sandbox-unavailable`."*
//!
//! That turns the standing principle — *by default something insecure and open-ended like bash is
//! not allowed; provide tools instead* — from a policy into a mechanism:
//!
//! **A tool whose effects this machine cannot confine is not published at all.**
//!
//! Not disabled, not gated, not refused when it is called: absent from the toolset, so the model
//! never sees it, never plans around it and never spends a turn being told no. The toolset is a
//! function of the machine, computed once from substrate's own probe.
//!
//! **The absence is silent to the model and stated to everyone else.** A tool that was *declared*
//! and could not be admitted leaves a [`Withheld`] record naming it and the predicate that decided
//! — see [`Facts::withheld`] and the module that computes it. Without that record a run whose only
//! legal route was running a program looked exactly like a run that never wanted one.
//!
//! # Three tiers, and this crate owns the first
//!
//! | tier | question | decided by |
//! |---|---|---|
//! | **publication** | may this tool exist here at all? | [`Facts`], at startup — here |
//! | authorization | may this call happen? | subjects × policy |
//! | approval | does a person say yes? | [`harness_wire::Envelope::needs_approval`] |
//!
//! # The predicates are substrate's, not ours
//!
//! Every operation in the wire contract carries `capability_predicates` — `exec.argv-only == true`,
//! `workspace.guarded-io == true`, `exec.output-limit-bytes >= <what you asked for>`. This crate
//! evaluates *those*, against the facts `GET /v1/machine` returns. It invents no policy of its own,
//! which is the whole reason to read a contract instead of writing a second one.
//!
//! Worth noticing in passing: `exec.start` requires **`exec.argv-only`**. Substrate will not run a
//! shell either. The design this component is building towards — a `run` tool over a declared
//! command set rather than a `bash` — is the same position, arrived at independently.
//!
//! # No HTTP crate, and the refusal is recorded here
//!
//! The transport is HTTP/1.1 over an owner-permissioned Unix socket, and this crate speaks it by
//! hand. `reqwest` does not carry a Unix-socket transport in the feature set this workspace already
//! builds, so taking one would mean adding `hyper`, `hyperlocal` and their trees to reach four
//! routes with no body streaming, no redirects, no compression and no TLS. The workspace rule is
//! *prefer no dependency, and record the refusal* — the same rule that kept `ratatui` out of
//! `aep-render`'s terminal frame.
//!
//! What that costs is stated rather than hidden: [`Client`] handles one request per connection,
//! reads a whole body into memory, and understands `Content-Length` and nothing else. Every route
//! it is pointed at answers a bounded JSON document, so none of that is a limitation today, and a
//! route that streams would need a different client rather than a flag on this one.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use substrate_wire::{
    ConfinementRequest, ExecEnvironment, ExecLimits, ExecMeasurement, ExecStartInput, NetworkMode,
    ReadOnlyRoot, SandboxProfile, WorkspaceAccess,
};

mod backend;
mod base64;
mod client;
mod embedded;
mod predicate;
mod toolchain;
mod tools;
mod withheld;

pub use backend::Backend;
pub use client::{Client, Transport, UnixTransport};
pub use embedded::Embedded;
pub use predicate::{Predicate, PredicateOp, Unmet, When};
pub use substrate_wire::WorkspaceAccess as ProcessWorkspaceAccess;
pub use toolchain::Toolchain;
pub use tools::ConfinedOperations;
pub use withheld::Withheld;

/// Build and validate the exact workspace write surface for a confined process.
///
/// An empty declaration is read-only. Paths are workspace-relative directories and are sorted so
/// the request and its evidence are stable. Validation happens before the model runs rather than
/// becoming a failed tool call after a paid turn.
///
/// # Errors
///
/// Names a path set the substrate wire refuses.
pub fn process_workspace_access(paths: &[String]) -> Result<ProcessWorkspaceAccess, String> {
    if paths.is_empty() {
        return Ok(ProcessWorkspaceAccess::ReadOnly);
    }
    let mut writable_subtrees = paths.to_vec();
    writable_subtrees.sort();
    writable_subtrees.dedup();
    let access = ProcessWorkspaceAccess::Scoped { writable_subtrees };
    substrate_wire::validate_workspace_access(&access)
        .map_err(|error| format!("process workspace write scope was refused: {error}"))?;
    Ok(access)
}

/// What substrate says this machine can do.
///
/// The `capability` document of the wire contract, kept as the facts it carries rather than as a
/// struct per version: a fact this build has never heard of must survive being read, or a newer
/// daemon becomes unreadable to an older client for having *more* to say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Facts {
    /// Which driver answered.
    #[serde(default)]
    pub driver: Option<String>,
    /// Its version.
    #[serde(default)]
    pub driver_version: Option<String>,
    /// Every fact, by name.
    #[serde(default)]
    pub facts: BTreeMap<String, Value>,
    /// The capability snapshot this document was probed as.
    ///
    /// Carried beside the facts because an exec has to **name** it: substrate refuses a start whose
    /// admitted snapshot is stale, so a run that cannot say which probe it is acting on cannot be
    /// admitted confined. Kept as a [`Value`] for the same reason the facts are a map — a daemon
    /// that changes the shape of its own identifier must not become unreadable to this build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Value>,
}

impl Facts {
    /// A machine that admits nothing.
    ///
    /// What a caller gets when there is no daemon to ask. **Not an error**: a harness with no
    /// substrate is a harness whose confined tools do not exist, which is a legitimate way to run
    /// and exactly how this component has always run. Turning it into a failure would make the
    /// read-only harness unlaunchable on a machine that never wanted the other tools.
    pub fn none() -> Self {
        Self {
            driver: None,
            driver_version: None,
            facts: BTreeMap::new(),
            snapshot: None,
        }
    }

    /// The value of one fact, where the machine states it.
    pub fn get(&self, fact: &str) -> Option<&Value> {
        self.facts.get(fact)
    }

    /// Whether every predicate holds, or the first that does not.
    ///
    /// # Errors
    ///
    /// Returns the [`Unmet`] predicate, naming the fact, what was wanted and what the machine
    /// actually said — because the caller's next move is to read that sentence, not to retry.
    pub fn admits(&self, predicates: &[Predicate], input: &Value) -> Result<(), Unmet> {
        for predicate in predicates {
            predicate.check(self, input)?;
        }
        Ok(())
    }

    /// `true` when this machine can run a confined process.
    ///
    /// The single question the `run` tool's existence turns on, asked in one place so three callers
    /// cannot disagree about which facts count.
    ///
    /// `false` takes the tool away silently, which is right for the model and wrong for a reader:
    /// [`Facts::withheld`] is the other half, and says which required fact decided.
    pub fn confines_execution(&self) -> bool {
        self.get("exec.argv-only") == Some(&Value::Bool(true))
            && self
                .get("exec.cgroup-limits")
                .and_then(Value::as_object)
                .is_some_and(|limits| {
                    ["cpu", "memory", "processes"]
                        .iter()
                        .all(|key| limits.get(*key) == Some(&Value::Bool(true)))
                })
            // Every request this client builds asks for `resource-usage`. Publishing `run` when
            // the daemon withheld this independent fact makes every call deterministically earn
            // `exec.metrics-unserved`, which is absence disguised as a usable tool.
            && self
                .get("exec.resource-usage")
                .and_then(Value::as_object)
                .is_some()
    }

    /// `true` when this machine can hold a guarded workspace.
    ///
    /// The question `workspace_write` turns on. Separate from execution on purpose: substrate
    /// serves workspaces on a machine that cannot confine a process, so a harness there gets the
    /// write tools and not the `run` tool — a real configuration, not a degenerate one.
    pub fn holds_workspaces(&self) -> bool {
        self.get("workspace.guarded-io") == Some(&Value::Bool(true))
    }
}

/// How long one confined exec may run when the run itself sets no shorter bound.
///
/// Fifteen minutes: sized for a build, argued at the limits below.
const EXEC_TIMEOUT_MS: u64 = 900_000;

/// The one exec a confined backend starts, whichever backend is starting it.
///
/// **Both paths build it here, and that is the point.** The embedded driver asked for confinement
/// by name from its first commit; the socket path posted `{workspace_id, argv}` and nothing else,
/// so whether that ran unconfined or was refused was the daemon's choice rather than this harness's.
/// Two call sites building the same request separately is how they came to differ without anybody
/// deciding to, so there is now one and a divergence has to be written down here to happen.
///
/// The wire crate's own types decide the field names. Nothing hand-writes this JSON — that is the
/// whole thing embedding bought, and the socket path never had it.
pub(crate) fn confined_exec_input(
    workspace: &str,
    argv: &[String],
    snapshot: String,
    env: ExecEnvironment,
    read_only_roots: Vec<ReadOnlyRoot>,
    workspace_access: WorkspaceAccess,
    remaining: Option<Duration>,
) -> ExecStartInput {
    // The smaller of the ceiling below and what the run has left on its clock. The loop's
    // deadline check between calls cannot reach into an exec the daemon is holding open, so the
    // bound the daemon enforces has to be the run's as well as the build's.
    let timeout_ms = remaining
        .and_then(|left| u64::try_from(left.as_millis()).ok())
        .map_or(EXEC_TIMEOUT_MS, |left| left.min(EXEC_TIMEOUT_MS));
    ExecStartInput {
        workspace: workspace.to_owned(),
        argv: argv.to_vec(),
        env,
        // substrate mounts these read-only and reports them in the observation (its ADR 0010).
        // Empty unless the caller declared a toolchain, which is every existing consumer.
        read_only_roots,
        sandbox: ConfinementRequest {
            // The field the wire path never found: substrate refuses an exec whose admitted
            // snapshot is stale, so the run has to name the one it probed.
            capability_snapshot: snapshot,
            network: NetworkMode::None,
            aperture: None,
            profile: SandboxProfile::Workspace,
            required: true,
        },
        // **Sized for a build, not for an interpreter.** Two minutes of wall clock and two minutes
        // of CPU were right when the only thing a confined run could execute was something under
        // `/usr`; a declared toolchain makes a compiler reachable, and a compiler blows through
        // both without finishing anything. A bound that makes a capability unusable is the same as
        // not having it.
        //
        // CPU is the larger of the two because a build is parallel: `cargo` will happily use every
        // core, so the CPU a wall-clock minute can consume is a multiple of it. Both are still
        // bounds — a run that loops is stopped, which is the whole point of having them.
        limits: ExecLimits {
            timeout_ms,
            output_bytes: 1_048_576,
            // A parallel build is hundreds of processes and threads, not dozens: `cargo` fans out
            // across every core and each `rustc` spawns its own codegen threads. At 64 the run did
            // not fail cleanly — it died inside the standard library with `failed to spawn thread:
            // Resource temporarily unavailable`, which reads as a machine under load rather than
            // as a bound somebody set.
            processes: 2_048,
            memory_bytes: 8_589_934_592,
            cpu_millis: 3_600_000,
        },
        wait: true,
        workspace_access,
        scratch: None,
        measurements: [ExecMeasurement::ResourceUsage].into_iter().collect(),
        secret_slots: Vec::new(),
        capsule: None,
        lease_ttl_ms: None,
    }
}

/// Every way this crate fails.
#[derive(Debug, thiserror::Error)]
pub enum SubstrateError {
    #[error("no substrate daemon at {path}: {source}")]
    Unreachable {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("the substrate daemon answered {status}: {body}")]
    Refused { status: u16, body: String },
    #[error("the substrate daemon's answer is not the document this build reads: {reason}")]
    Unreadable { reason: String },
}

#[cfg(test)]
mod tests;
