//! Substrate's own driver, in this process.
//!
//! # What this trades away, said before anything else
//!
//! **The authenticated boundary.** A daemon derives its subject from kernel peer credentials and
//! never from request data; embedded, there is no peer and therefore no subject. Every confinement
//! substrate enforces on a *workspace* still holds — the guarded IO, the `openat2` containment, the
//! cgroup limits and namespaces around an exec are the driver's, not the daemon's, and they are
//! here. What is gone is the answer to *who asked*.
//!
//! That is the right trade for a harness the operator runs on their own machine against their own
//! tree, and the wrong one for anything multi-tenant. `harness-substrate` keeps
//! [`Client`](crate::Client) beside this for exactly that reason, and the tools cannot tell which
//! they are holding.
//!
//! **Substrate's own boundary rule.** Its README says cross-component consumers use the released
//! `substrate-daemon` artifact and the wire contract and do not import the implementation crate.
//! This imports `substrate-host`. Recorded here rather than left for a reader to notice, because a
//! rule crossed silently is a rule nobody can review.
//!
//! # Why it was worth crossing
//!
//! The socket path could not be made to work. `POST /v1/workspaces` on the daemon this machine runs
//! answered `422 request.schema-invalid` to every body derivable from the committed 0.2.0 and 0.4.0
//! contracts, and that daemon was built from a source this repository does not have. Embedding
//! removes the guess entirely: `WorkspaceCreateInput` is a Rust type, substrate serialises it, and
//! a disagreement becomes a compile error instead of a 422.
//!
//! # One runtime, current-thread, and what that costs a batch
//!
//! The driver is async and this workspace is blocking. A current-thread runtime is created once and
//! every operation is `block_on`.
//!
//! **Work does reach it concurrently**, which it did not when this was written:
//! `harness_tools::Catalogue::invoke_batch` gives each call of a batch its own thread, and all of
//! them `block_on` this one runtime. A current-thread runtime serialises them, so a batch of reads
//! costs the same wall clock here as the same reads one after another — the threads queue instead
//! of the calls.
//!
//! That is a stated cost and not a bug. Nothing races: the runtime is `Send + Sync` and each
//! `block_on` is a whole operation. What a batch buys on this backend is one model round trip
//! instead of six, which is the larger half of what batching was for; what it does not buy is
//! parallel I/O. A multi-thread runtime would buy that and cost a thread pool inside a process that
//! already has one thread per call, so it is a change to make deliberately if a run is ever bounded
//! by this rather than by the model.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use substrate_host::{DispatchOutcome, Driver as _, HostConfig, HostDriver};
use substrate_wire::{
    EmptySource, ExecEnvironment, ExecOutputQuery, FileMode, FileReadQuery, OutputStream,
    WorkspaceCreateInput, WorkspaceSource,
};

use crate::{Backend, Facts, SubstrateError};

/// One execution's identity, in the shape substrate admits.
///
/// `^ex_[A-Za-z0-9_]+$`, and unique per call. Its own function so the rule is testable without a
/// driver, a workspace or a delegated cgroup — which is why nothing caught the shape that came
/// before it.
pub(crate) fn exec_identity(process: u32, sequence: u64) -> String {
    format!("ex_{process}_{sequence}")
}

/// One workspace's identity, in the shape the driver admits.
///
/// `^ws_[A-Za-z0-9_]+$`, and unique per call. It was `ws_{lease}_{pid}` — no sequence — so two
/// creates with the same lease in one process minted the **same** id, and the second silently took
/// the first's directory instead of opening its own. Its own function for the reason
/// [`exec_identity`] is one: the rule is then testable without a driver.
pub(crate) fn workspace_identity(lease_ttl_ms: u64, process: u32, sequence: u64) -> String {
    format!("ws_{lease_ttl_ms}_{process}_{sequence}")
}

/// Substrate's driver, held in this process.
pub struct Embedded {
    driver: Arc<HostDriver>,
    runtime: tokio::runtime::Runtime,
    root: PathBuf,
    /// The toolchain this driver admits read-only, if the caller declared one.
    ///
    /// Empty by default, and empty is what keeps a run reaching nothing outside `/usr` and its own
    /// workspace. See [`crate::Toolchain`] for what declaring one costs.
    toolchain: crate::Toolchain,
    /// Makes each exec's identity distinct. substrate keys an execution's output and lifetime on
    /// it, so two calls sharing one would read each other's.
    next_exec: std::sync::atomic::AtomicU64,
    /// The same for workspaces. The lease and the pid are not enough to tell two apart: a run that
    /// opens two workspaces with one lease would otherwise be handed one directory twice.
    next_workspace: std::sync::atomic::AtomicU64,
}

impl std::fmt::Debug for Embedded {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Embedded")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl Embedded {
    /// Opens a driver whose workspaces live under `root`.
    ///
    /// Execution is served only when `cgroup_root` names a delegated cgroup subtree — substrate's
    /// own probe decides, and a host without one reports no exec facts, so no `run` tool is
    /// published. Nothing here weakens that; it is passed through and asked.
    ///
    /// # Errors
    ///
    /// Returns [`SubstrateError::Unreadable`] when the driver cannot open the root, naming what it
    /// said.
    pub fn open(
        root: impl AsRef<Path>,
        cgroup_root: Option<PathBuf>,
    ) -> Result<Self, SubstrateError> {
        Self::open_with(root, cgroup_root, crate::Toolchain::default())
    }

    /// The same, admitting a toolchain read-only inside every exec this driver starts.
    ///
    /// # Errors
    ///
    /// As [`Self::open`].
    pub fn open_with(
        root: impl AsRef<Path>,
        cgroup_root: Option<PathBuf>,
        toolchain: crate::Toolchain,
    ) -> Result<Self, SubstrateError> {
        // Canonical, because the driver compares every path it opens against this root: a root
        // reached through a symlink makes every workspace under it look like an escape. A
        // temporary directory under a symlinked `TMPDIR` is the ordinary way to meet that.
        let root = root
            .as_ref()
            .canonicalize()
            .map_err(|error| SubstrateError::Unreadable {
                reason: format!("workspace root `{}`: {error}", root.as_ref().display()),
            })?;
        let mut config = HostConfig::minimum(&root);
        config.cgroup_root = cgroup_root;
        let driver = HostDriver::open(config).map_err(|error| SubstrateError::Unreadable {
            reason: format!("the embedded driver did not open: {error:?}"),
        })?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| SubstrateError::Unreadable {
                reason: format!("no runtime for the embedded driver: {error}"),
            })?;
        Ok(Self {
            toolchain,
            driver,
            runtime,
            root,
            next_exec: std::sync::atomic::AtomicU64::new(0),
            next_workspace: std::sync::atomic::AtomicU64::new(0),
        })
    }

    /// Where the workspaces live.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Take an existing directory under the root as a confined workspace.
    ///
    /// # The gap this closes
    ///
    /// [`Backend::workspace_create`] makes a **new, empty** directory. That is right for a run that
    /// builds something from nothing and wrong for every run that works on a tree that already
    /// exists — which, for an evaluation of a coding agent, is all of them. Without this a run read
    /// one tree through the read-only tools and wrote into another through the confined ones, and
    /// was not doing the task it had been given.
    ///
    /// Adopting performs no copy. The directory *is* the workspace, so reads and writes land in the
    /// same place and substrate's containment applies to it from this moment on.
    ///
    /// # What it does not do
    ///
    /// It does not make the tree confined **retroactively**. Whatever was in that directory before
    /// is there, symlinks included, and `openat2` containment stops a path *leaving* the workspace
    /// rather than auditing what was already inside it.
    ///
    /// # Errors
    ///
    /// Returns [`SubstrateError::Refused`] when the name is not one the driver can represent — it
    /// must begin `ws_` and hold only alphanumerics and underscores — or when no such directory is
    /// there. The name rule is the driver's; see [`Backend::workspace_create`] for why meeting the
    /// stricter of its two checks is the only thing a caller can do.
    pub fn workspace_adopt(&self, name: &str) -> Result<String, SubstrateError> {
        self.driver
            .workspace_root_identity(name)
            .map_err(|error| Self::refused("workspace identity", &error))?;
        if !name.starts_with("ws_")
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(SubstrateError::Refused {
                status: 0,
                body: format!(
                    "`{name}` cannot be a workspace: a name must begin `ws_` and hold only \
                     alphanumerics and underscores. Rename the directory, or let \
                     `workspace_create` open a fresh one."
                ),
            });
        }
        let path = self.root.join(name);
        if !path.is_dir() {
            return Err(SubstrateError::Refused {
                status: 0,
                body: format!("`{}` is not a directory to adopt", path.display()),
            });
        }
        Ok(name.to_owned())
    }

    fn refused(context: &str, error: &impl std::fmt::Debug) -> SubstrateError {
        SubstrateError::Refused {
            // Not an HTTP status: there is no HTTP. `0` is the honest stand-in and the message
            // carries the driver's own typed refusal, which is the part a reader acts on.
            status: 0,
            body: format!("{context}: {error:?}"),
        }
    }
}

impl Backend for Embedded {
    fn machine(&self) -> Result<Facts, SubstrateError> {
        let snapshot = self.driver.machine();
        let value =
            serde_json::to_value(&snapshot).map_err(|error| SubstrateError::Unreadable {
                reason: error.to_string(),
            })?;
        serde_json::from_value(value).map_err(|error| SubstrateError::Unreadable {
            reason: error.to_string(),
        })
    }

    fn workspace_create(&self, lease_ttl_ms: u64) -> Result<String, SubstrateError> {
        // **`ws_`, underscores, alphanumerics — and nothing else.** The id is ours to choose here,
        // where over the wire the daemon minted one, and the driver has two checks that disagree
        // about what is legal: `HostDriver::workspace_path` admits `[A-Za-z0-9_-]`, while
        // `validate_root_name` inside the guarded filesystem requires the `ws_` prefix and refuses a
        // hyphen. A name that passes the first and fails the second reaches `mkdirat` and comes back
        // as `workspace.path-escape` — which reads as a containment failure and is a naming rule.
        // Meeting the stricter of the two is the only thing a caller can do about that.
        //
        // The sequence is what makes it *this* workspace: the lease and the pid are properties of
        // the run, not of the directory, so two creates with one lease minted one id and the second
        // caller was handed the first's tree.
        let id = workspace_identity(
            lease_ttl_ms,
            std::process::id(),
            self.next_workspace
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        );
        let root_name = self
            .driver
            .workspace_root_identity(&id)
            .map_err(|error| Self::refused("workspace identity", &error))?;
        // Every field named, because they are types now rather than a JSON body somebody guessed
        // at. This is the whole payoff of embedding: `WorkspaceSource::Empty` is a variant the
        // compiler checks, where over the wire it was five refused spellings in a row.
        let input = WorkspaceCreateInput {
            source: WorkspaceSource::Empty(EmptySource::Empty),
            labels: substrate_wire::Labels::new(),
            lease_ttl_ms: Some(lease_ttl_ms),
        };
        let outcome = self
            .runtime
            .block_on(self.driver.create_workspace(&id, &root_name, &input));
        match outcome {
            DispatchOutcome::Observed(_) => Ok(id),
            // Three refusals and not one, because they are three different facts: the driver never
            // dispatched, it dispatched and the thing is provably absent, or it dispatched and
            // nobody knows. A caller that folded them together would retry the third, which is the
            // one case where retrying can do the work twice.
            DispatchOutcome::NotDispatched(error) => {
                Err(Self::refused("workspace.create was not dispatched", &error))
            }
            DispatchOutcome::ContainedAbsent(error) => Err(Self::refused(
                "workspace.create left nothing behind",
                &error,
            )),
            DispatchOutcome::OutcomeUnknown(error) => Err(Self::refused(
                "workspace.create outcome is unknown - do not retry blindly",
                &error,
            )),
        }
    }

    fn file_write(&self, workspace: &str, path: &str, text: &str) -> Result<Value, SubstrateError> {
        let root_name = self
            .driver
            .workspace_root_identity(workspace)
            .map_err(|error| Self::refused("workspace identity", &error))?;
        // Bytes, not base64. The encoding was the *wire's*; a driver takes the file.
        let observed = self
            .runtime
            .block_on(self.driver.write_workspace_file(
                workspace,
                &root_name,
                path,
                text.as_bytes(),
            ))
            .map_err(|error| Self::refused("workspace.file-write", &error))?;
        serde_json::to_value(observed).map_err(|error| SubstrateError::Unreadable {
            reason: error.to_string(),
        })
    }

    /// One file, from byte 0 to [`substrate_wire::MAX_IO_BYTES`], as text.
    ///
    /// # The ceiling is the whole answer's shape, and this signature drops what the driver says
    /// about it
    ///
    /// `MAX_IO_BYTES` is asked for because it is exactly what `HostConfig::minimum` sets the
    /// driver's own `read_limit_bytes` to — a larger `limit_bytes` is refused by the guarded
    /// filesystem rather than clamped — and it is the figure the driver's probe reports as
    /// `workspace.read-limit-bytes`. `ConfinedOperations` reads that fact and treats a read that
    /// returned the whole ceiling as one the ceiling cut, which is what stops a 1 MiB prefix of a
    /// 3 MiB file being answered as the file.
    ///
    /// The driver answers better than that: the document here is a `substrate_wire::FileSlice` and
    /// it carries `eof`, which is exact where a length test is a guess in the safe direction.
    /// [`Backend::file_read`] hands back a `String`, so carrying it is a change to that trait and
    /// to both implementations of it, and it is worth making the next time this trait moves.
    fn file_read(&self, workspace: &str, path: &str) -> Result<String, SubstrateError> {
        let root_name = self
            .driver
            .workspace_root_identity(workspace)
            .map_err(|error| Self::refused("workspace identity", &error))?;
        // A file query carries an offset and a byte ceiling; a directory query carries a cursor
        // and an item ceiling. Mixing them, or leaving a mode's own fields empty, is refused with
        // *file query does not match its selected mode* - the shape is checked against the mode
        // rather than the fields being independently optional.
        let query = FileReadQuery {
            mode: FileMode::File,
            offset: Some(0),
            limit_bytes: Some(substrate_wire::MAX_IO_BYTES),
            cursor: None,
            limit_items: None,
        };
        let result = self
            .runtime
            .block_on(
                self.driver
                    .read_workspace_path(workspace, &root_name, path, &query),
            )
            .map_err(|error| Self::refused("workspace.file-read", &error))?;
        let value = serde_json::to_value(result).map_err(|error| SubstrateError::Unreadable {
            reason: error.to_string(),
        })?;
        // The driver answers the same document shape the wire does, so the reader is the same one.
        let data = value
            .pointer("/content/data")
            .and_then(Value::as_str)
            .ok_or_else(|| SubstrateError::Unreadable {
                reason: format!("no file content in {value}"),
            })?;
        let bytes =
            crate::base64::decode(data).map_err(|reason| SubstrateError::Unreadable { reason })?;
        String::from_utf8(bytes).map_err(|error| SubstrateError::Unreadable {
            reason: format!("the file is not text: {error}"),
        })
    }

    fn exec(
        &self,
        workspace: &str,
        argv: &[String],
        remaining: Option<Duration>,
    ) -> Result<Value, SubstrateError> {
        let root_name = self
            .driver
            .workspace_root_identity(workspace)
            .map_err(|error| Self::refused("workspace identity", &error))?;
        let snapshot = self.driver.machine();
        // Built by [`crate::confined_exec_input`], which the socket path calls too: two call sites
        // assembling this separately is how they came to ask for different things.
        let input = crate::confined_exec_input(
            workspace,
            argv,
            snapshot.snapshot.clone(),
            ExecEnvironment {
                // Nothing inherited. An exec that saw this process's environment would carry a
                // credential into a confined workspace, which is the one thing confinement is for.
                // What is set here is only what a declared toolchain needs to be findable, and it
                // names mount points inside the sandbox rather than anything on this host.
                allow: Vec::new(),
                set: self.toolchain.env().clone(),
            },
            self.toolchain.roots().to_vec(),
            remaining,
        );
        // **substrate's own shape, which this never had.** `admit` requires `^ex_[A-Za-z0-9_]+$`
        // and this was `exec-<pid>-<argv joined by dashes>`: wrong prefix, and a program path is
        // full of `/` and `.`. Every exec was refused `exec.identity-invalid` before it started —
        // for the whole life of the embedded driver, and quietly, because a failed tool call is
        // just a failed tool call to the model.
        //
        // It cost more than a feature. A live run asked to fix a suite had all three of its `run`
        // calls refused, edited the file anyway, and reported the suite passing; the file was
        // right and nothing had ever executed. A harness whose exec silently never works turns
        // every "run the tests" instruction into an invitation to claim.
        //
        // Unique per call, because substrate keys an execution's output and lifetime on it and two
        // calls sharing an id would read each other's.
        let id = exec_identity(
            std::process::id(),
            self.next_exec
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        );
        let started = self
            .runtime
            .block_on(self.driver.start_exec(&id, &root_name, &input));
        match started {
            DispatchOutcome::Observed(_) => {}
            DispatchOutcome::NotDispatched(error) => {
                return Err(Self::refused("exec.start was not dispatched", &error));
            }
            DispatchOutcome::ContainedAbsent(error) => {
                return Err(Self::refused("exec.start left nothing behind", &error));
            }
            DispatchOutcome::OutcomeUnknown(error) => {
                return Err(Self::refused(
                    "exec.start outcome is unknown - a retry may run it twice",
                    &error,
                ));
            }
        }
        let output = self
            .runtime
            .block_on(self.driver.output(
                &id,
                &ExecOutputQuery {
                    stream: OutputStream::Stdout,
                    offset: 0,
                    limit_bytes: 1_048_576,
                },
            ))
            .map_err(|error| Self::refused("exec.output", &error))?;
        let observed = self
            .runtime
            .block_on(self.driver.observe_exec(&id))
            .map_err(|error| Self::refused("exec.observe", &error))?;
        // `ExecObservation` is the driver's own type and carries raw bytes, so it is projected
        // here rather than serialised: what a model needs back is what the program said and how it
        // ended, and `stdout_truncated` is part of that - a partial answer that looked whole would
        // be read as the whole answer.
        Ok(json!({
            "stdout": String::from_utf8_lossy(&observed.stdout),
            "stderr": String::from_utf8_lossy(&observed.stderr),
            "stdout_truncated": observed.stdout_truncated,
            "stderr_truncated": observed.stderr_truncated,
            "output_complete": observed.output_complete,
            "exit": serde_json::to_value(&observed.resource).unwrap_or(Value::Null),
            "slice": serde_json::to_value(output).unwrap_or(Value::Null),
        }))
    }
}
