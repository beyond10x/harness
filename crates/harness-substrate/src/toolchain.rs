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
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use substrate_wire::ReadOnlyRoot;

/// Where a toolchain's directories appear inside the sandbox.
///
/// Under one parent so a reader of the applied observation can tell at a glance which mounts are a
/// toolchain and which are anything else, and so a second toolchain cannot silently collide with
/// this one.
const MOUNT_PREFIX: &str = "/toolchain";

/// Where substrate binds the workspace.
const WORKSPACE: &str = "/workspace";

/// Where a staged driver appears inside the sandbox.
///
/// Under [`MOUNT_PREFIX`] with the toolchains, because it is the same kind of thing: a closure
/// brought in read-only for a process with no network to fetch one. A second mount point rather
/// than a second entry under `/toolchain/rustup`, so a reader of the applied observation can tell
/// the compiler from the program that drives the run.
const DRIVER_MOUNT: &str = "/toolchain/driver";

/// The toolchain directory `rustup` keeps a stable install under.
///
/// Named rather than discovered because the shim directory `rustup` puts on `PATH` lives in
/// `CARGO_HOME`, and `CARGO_HOME` is now inside the workspace where no shim exists. Pointing
/// `PATH` straight at the toolchain's own `bin` skips the shim entirely, which is what a confined
/// run wants anyway: one fixed compiler, chosen before the run started.
const TOOLCHAIN: &str = "stable-x86_64-unknown-linux-gnu";

/// One host program, staged so a confined run can execute it.
///
/// **Its digest is here because substrate will not compute one.** A declared root is mounted
/// read-only and *reported*, and that is the whole guarantee (`substrate-wire`'s own note: "what
/// cannot be verified must at least be visible"). So the claim that a run pins the build its
/// evidence is recorded against is only true if somebody writes the digest down, and this is the
/// value they write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedDriver {
    program: String,
    sha256: String,
}

impl StagedDriver {
    /// Where the program is, **inside the sandbox** — the path an argv must name.
    #[must_use]
    pub fn program(&self) -> &str {
        &self.program
    }

    /// The SHA-256 of the bytes that were staged, hex, lower case.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

/// A toolchain the run may read, and the environment that points at it.
#[derive(Debug, Clone, Default)]
pub struct Toolchain {
    roots: Vec<ReadOnlyRoot>,
    env: BTreeMap<String, String>,
    driver: Option<StagedDriver>,
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
            None => home.map(|home| home.join(".rustup")).ok_or_else(|| {
                "neither `RUSTUP_HOME` nor a home directory says where the Rust \
                                 toolchain is"
                    .to_owned()
            })?,
        };
        let rustup = rustup.canonicalize().map_err(|error| {
            format!(
                "the Rust toolchain's `rustup` directory ({}): {error}",
                rustup.display()
            )
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
            format!(
                "{MOUNT_PREFIX}/rustup/toolchains/{TOOLCHAIN}/bin:/usr/local/bin:/usr/bin:/bin"
            ),
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
        toolchain
            .env
            .insert("HOME".to_owned(), WORKSPACE.to_owned());
        // A build tree the run may write to. Without it cargo writes beside the sources, which is
        // fine, but naming it keeps the output somewhere a caller can find and clear.
        toolchain
            .env
            .insert("CARGO_TARGET_DIR".to_owned(), format!("{WORKSPACE}/target"));
        Ok(toolchain)
    }

    /// The same, with one host program staged and admitted read-only.
    ///
    /// # Why a program needs this at all
    ///
    /// A confined run reaches `/usr`, `/bin`, `/lib` and `/lib64` and its own workspace, and
    /// nothing else. Allow-listing a program by absolute host path admits the **name**; the
    /// sandbox still has no such **file**, so the exec dies at `ENOENT` and a model reads that as
    /// *the command is wrong* rather than *the program is not here*. A driven run whose only legal
    /// route is its own CLI could not take it, and wrote the store's files directly instead.
    ///
    /// # Why a private stage and not the program's own directory
    ///
    /// A root must be a directory (substrate refuses `exec.read-only-root-not-a-directory`), and
    /// the directory a build puts its binary in holds every other binary, every dependency and
    /// every build script. Mounting it would admit all of them to answer for one. So exactly one
    /// file is linked into a directory of its own, and that is what is mounted.
    ///
    /// The stage is **named by the digest of what is in it**, which makes this idempotent across
    /// runs and makes a rebuilt program a different stage rather than a silently reused one. The
    /// link is hard where the filesystem allows it: `cargo` replaces a binary by rename, so a hard
    /// link keeps the exact bytes this run was launched against even if a build lands mid-run.
    ///
    /// # Errors
    ///
    /// Names the program that is not a readable file, or the stage that cannot be written. A
    /// driver half-declared would produce a run that fails at its first `run` call with `ENOENT`,
    /// which is the failure this exists to remove.
    pub fn with_driver(mut self, program: &Path, stage_root: &Path) -> Result<Self, String> {
        let program = program
            .canonicalize()
            .map_err(|error| format!("the driver program ({}): {error}", program.display()))?;
        if !program.is_file() {
            return Err(format!(
                "the driver program ({}) is not a file",
                program.display()
            ));
        }
        let name = program
            .file_name()
            .ok_or_else(|| format!("the driver program ({}) has no name", program.display()))?
            .to_string_lossy()
            .into_owned();

        let bytes = std::fs::read(&program)
            .map_err(|error| format!("the driver program ({}): {error}", program.display()))?;
        let sha256 = hex(&Sha256::digest(&bytes));

        let stage = stage_root.join(format!("harness-driver-{}", &sha256[..16]));
        std::fs::create_dir_all(&stage)
            .map_err(|error| format!("the driver stage ({}): {error}", stage.display()))?;
        // The stage is the operator's, not the run's: the mount is read-only, but a stage anyone
        // could write is a stage anyone could swap before the mount happens.
        std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("the driver stage ({}): {error}", stage.display()))?;

        let staged = stage.join(&name);
        if !staged.exists() {
            // Hard link first, and fall back only across a filesystem boundary. The digest in the
            // stage's name means an existing file is already these bytes, so this runs once per
            // build rather than once per run.
            if std::fs::hard_link(&program, &staged).is_err() {
                std::fs::copy(&program, &staged).map_err(|error| {
                    format!("staging the driver at {}: {error}", staged.display())
                })?;
                std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o500)).map_err(
                    |error| format!("staging the driver at {}: {error}", staged.display()),
                )?;
            }
        }

        self.roots.push(ReadOnlyRoot {
            host_path: stage.display().to_string(),
            mount: DRIVER_MOUNT.to_owned(),
        });
        self.driver = Some(StagedDriver {
            program: format!("{DRIVER_MOUNT}/{name}"),
            sha256,
        });
        Ok(self)
    }

    /// The staged driver, where one was declared.
    #[must_use]
    pub fn driver(&self) -> Option<&StagedDriver> {
        self.driver.as_ref()
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

/// Lower-case hex, because a digest that reaches a record has one spelling.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}
