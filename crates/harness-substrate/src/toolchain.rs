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

/// Where substrate binds the workspace.
const WORKSPACE: &str = "/workspace";

/// The toolchain directory `rustup` keeps a stable install under.
///
/// Named rather than discovered because the shim directory `rustup` puts on `PATH` lives in
/// `CARGO_HOME`, and `CARGO_HOME` is now inside the workspace where no shim exists. Pointing
/// `PATH` straight at the toolchain's own `bin` skips the shim entirely, which is what a confined
/// run wants anyway: one fixed compiler, chosen before the run started.
const TOOLCHAIN: &str = "stable-x86_64-unknown-linux-gnu";

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
        let rustup = match std::env::var_os("RUSTUP_HOME") {
            Some(value) => PathBuf::from(value),
            None => home
                .map(|home| home.join(".rustup"))
                .ok_or_else(|| "neither `RUSTUP_HOME` nor a home directory says where the Rust \
                                 toolchain is".to_owned())?,
        };
        let rustup = rustup.canonicalize().map_err(|error| {
            format!("the Rust toolchain's `rustup` directory ({}): {error}", rustup.display())
        })?;
        if !rustup.is_dir() {
            return Err(format!(
                "the Rust toolchain's `rustup` directory ({}) is not a directory",
                rustup.display()
            ));
        }

        let mut toolchain = Self::default();
        toolchain.roots.push(ReadOnlyRoot {
            host_path: rustup.display().to_string(),
            mount: format!("{MOUNT_PREFIX}/rustup"),
        });

        // `--clearenv` leaves the process with nothing, so everything the toolchain reads is said
        // here. `PATH` carries rustup's shims, which is how `cargo` finds `rustc` at all.
        toolchain
            .env
            .insert("RUSTUP_HOME".to_owned(), format!("{MOUNT_PREFIX}/rustup"));
        toolchain.env.insert(
            "PATH".to_owned(),
            format!("{MOUNT_PREFIX}/rustup/toolchains/{TOOLCHAIN}/bin:/usr/local/bin:/usr/bin:/bin"),
        );
        // **`CARGO_HOME` is inside the workspace, and that is not a convenience.**
        //
        // Two reasons, and the first is the serious one. `~/.cargo` holds `credentials.toml` — a
        // registry token — beside the package cache, so mounting it whole would hand every
        // confined run the operator's publishing credential. It was mounted whole for one commit
        // and that was wrong; nothing about a build needs the token, and a confinement that leaks
        // one is not a confinement.
        //
        // The second is that cargo needs `CARGO_HOME` **writable** even offline: it takes a
        // `.package-cache` lock there before it does anything, and against a read-only mount it
        // blocks forever with no output — which is exactly how this was found.
        //
        // So the caller seeds `<workspace>/.cargo` with the package cache the run needs. That is a
        // copy rather than a mount, and it is the caller's to make, because only the caller knows
        // which crates a task will want.
        toolchain
            .env
            .insert("CARGO_HOME".to_owned(), format!("{WORKSPACE}/.cargo"));
        // The workspace, never the operator's home — which is not mounted and must not be implied.
        toolchain.env.insert("HOME".to_owned(), WORKSPACE.to_owned());
        // A build tree the run may write to. Without it cargo writes beside the sources, which is
        // fine, but naming it keeps the output somewhere a caller can find and clear.
        toolchain
            .env
            .insert("CARGO_TARGET_DIR".to_owned(), format!("{WORKSPACE}/target"));
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
