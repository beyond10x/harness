//! A build toolchain, admitted read-only into a confined workspace.
//!
//! # Where this knowledge belongs
//!
//! substrate mounts **declared host roots** (its ADR 0010) and knows nothing about what is in one:
//! a root is a directory and a mount point. That is deliberate — the moment it knew what `cargo`
//! was it would be carrying one client's vendor semantics.
//!
//! So the mapping from *a Rust build* to *these two directories and these four variables* lives
//! here, in the client that wants it. A second toolchain is a second constructor, not a change to
//! the substrate contract.
//!
//! # What it costs, stated plainly
//!
//! A declared root is trusted by whoever declared it and unverified by substrate: there is no
//! manifest and no digest, because hashing a package registry on every exec is not a thing anybody
//! would run twice. What is guaranteed is that the directory is mounted **read-only** at the point
//! named and reported in the run's observation — so a process cannot alter the toolchain it was
//! given, and a reader can see exactly what was admitted.
//!
//! Nothing here opens a network. A toolchain is a closure brought *in*; a build that needs to fetch
//! a crate it does not already have will fail, and that is the confinement working.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use substrate_wire::ReadOnlyRoot;

/// Where a toolchain's directories appear inside the sandbox.
///
/// Under one parent so a reader of the applied observation can tell at a glance which mounts are a
/// toolchain and which are anything else, and so a second toolchain cannot silently collide with
/// this one.
const MOUNT_PREFIX: &str = "/toolchain";

/// A toolchain the run may read, and the environment that points at it.
#[derive(Debug, Clone, Default)]
pub struct Toolchain {
    roots: Vec<ReadOnlyRoot>,
    env: BTreeMap<String, String>,
}

impl Toolchain {
    /// The Rust toolchain, from the directories `cargo` and `rustup` actually keep it in.
    ///
    /// Read from `CARGO_HOME` and `RUSTUP_HOME` where the operator set them and from `$HOME`'s
    /// conventional locations otherwise — the same two places the tools themselves look, so a run
    /// gets the toolchain the operator has rather than one this file guessed at.
    ///
    /// # Errors
    ///
    /// Names the directory that is missing. A toolchain half-declared would produce a build that
    /// fails deep inside cargo with a message about a registry, which is a much worse way to find
    /// out that a path was wrong.
    pub fn rust(home: Option<&Path>) -> Result<Self, String> {
        let located = |variable: &str, fallback: &str| -> Result<PathBuf, String> {
            if let Some(value) = std::env::var_os(variable) {
                return Ok(PathBuf::from(value));
            }
            home.map(|home| home.join(fallback)).ok_or_else(|| {
                format!("neither `{variable}` nor a home directory says where to find `{fallback}`")
            })
        };
        let cargo = located("CARGO_HOME", ".cargo")?;
        let rustup = located("RUSTUP_HOME", ".rustup")?;

        let mut toolchain = Self::default();
        for (path, name) in [(cargo, "cargo"), (rustup, "rustup")] {
            let path = path.canonicalize().map_err(|error| {
                format!("the Rust toolchain's `{name}` directory ({}): {error}", path.display())
            })?;
            if !path.is_dir() {
                return Err(format!(
                    "the Rust toolchain's `{name}` directory ({}) is not a directory",
                    path.display()
                ));
            }
            toolchain.roots.push(ReadOnlyRoot {
                host_path: path.display().to_string(),
                mount: format!("{MOUNT_PREFIX}/{name}"),
            });
        }

        // `--clearenv` leaves the process with nothing, so everything cargo reads has to be said
        // here. `PATH` carries rustup's shims, which is how `cargo` finds `rustc` at all.
        toolchain.env.insert(
            "CARGO_HOME".to_owned(),
            format!("{MOUNT_PREFIX}/cargo"),
        );
        toolchain.env.insert(
            "RUSTUP_HOME".to_owned(),
            format!("{MOUNT_PREFIX}/rustup"),
        );
        toolchain.env.insert(
            "PATH".to_owned(),
            format!("{MOUNT_PREFIX}/cargo/bin:/usr/local/bin:/usr/bin:/bin"),
        );
        // The workspace, not the operator's home — which is not mounted and must not be implied.
        // cargo reads `HOME` for its own fallbacks and a run without one behaves unpredictably.
        toolchain.env.insert("HOME".to_owned(), "/workspace".to_owned());
        // A build tree the run may actually write to. `CARGO_HOME` is read-only by construction,
        // so without this cargo would try to write its output into a directory it cannot.
        toolchain
            .env
            .insert("CARGO_TARGET_DIR".to_owned(), "/workspace/target".to_owned());
        Ok(toolchain)
    }

    /// The roots to declare on a start.
    #[must_use]
    pub fn roots(&self) -> &[ReadOnlyRoot] {
        &self.roots
    }

    /// The variables the exec must be given.
    #[must_use]
    pub fn env(&self) -> &BTreeMap<String, String> {
        &self.env
    }
}
