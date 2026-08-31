//! A build toolchain, admitted read-only into a confined workspace.
//!
//! # Where this knowledge belongs
//!
//! substrate mounts **declared host roots** (its ADR 0010) and knows nothing about what is in one:
//! a root is a directory and a mount point. That is deliberate — the moment it knew what `cargo`
//! was it would be carrying one client's vendor semantics.
//!
//! So declarative providers resolve names to directories and environment above this boundary. This
//! module only converts their neutral resolved roots into substrate declarations; adding a
//! provider does not add a branch here or change the substrate contract.
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
//! a crate or module it does not already have will fail, and that is the confinement working.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use sha2::{Digest, Sha256};
use substrate_wire::ReadOnlyRoot;

/// Where a toolchain's directories appear inside the sandbox.
///
/// Under one parent so a reader of the applied observation can tell at a glance which mounts are a
/// toolchain and which are anything else, and so a second toolchain cannot silently collide with
/// this one.
/// Where a staged driver appears inside the sandbox.
///
/// Under `/toolchain` with the toolchains, because it is the same kind of thing: a closure
/// brought in read-only for a process with no network to fetch one. A second mount point rather
/// than a second entry under `/toolchain/rustup`, so a reader of the applied observation can tell
/// the compiler from the program that drives the run.
const DRIVER_MOUNT: &str = "/toolchain/driver";

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
    programs: Vec<String>,
    providers: Vec<harness_toolchain::ResolvedProvider>,
}

impl Toolchain {
    /// Builds the confined closure from provider definitions resolved by the declarative registry.
    ///
    /// The registry has already performed every host read and path check. This conversion names
    /// only substrate's neutral read-only roots and closed execution environment.
    ///
    /// # Errors
    ///
    /// Refuses providers that require conflicting values for the same sandbox environment key.
    pub fn from_providers(
        providers: Vec<harness_toolchain::ResolvedProvider>,
    ) -> Result<Self, String> {
        let mut toolchain = Self::default();
        for provider in &providers {
            toolchain
                .roots
                .extend(provider.roots.iter().map(|root| ReadOnlyRoot {
                    host_path: root.host.display().to_string(),
                    mount: root.mount.clone(),
                }));
            toolchain.programs.extend(provider.programs.iter().cloned());
            for (name, value) in &provider.env {
                merge_env(&mut toolchain.env, name, value)?;
            }
        }
        toolchain.programs.sort();
        toolchain.programs.dedup();
        toolchain.providers = providers;
        Ok(toolchain)
    }

    /// Combines independently discovered installations into one read-only closure.
    ///
    /// # Errors
    ///
    /// Refuses when two installations require different values for the same environment variable.
    pub fn combine(mut self, other: Self) -> Result<Self, String> {
        self.roots.extend(other.roots);
        self.programs.extend(other.programs);
        self.providers.extend(other.providers);
        for (name, value) in other.env {
            merge_env(&mut self.env, &name, &value)?;
        }
        Ok(self)
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

    #[must_use]
    pub fn programs(&self) -> &[String] {
        &self.programs
    }

    /// The resolved provider definitions used to build this closure.
    #[must_use]
    pub fn providers(&self) -> &[harness_toolchain::ResolvedProvider] {
        &self.providers
    }
}

fn merge_env(env: &mut BTreeMap<String, String>, name: &str, value: &str) -> Result<(), String> {
    match env.get_mut(name) {
        None => {
            env.insert(name.to_owned(), value.to_owned());
        }
        Some(existing) if existing == value => {}
        Some(existing) if name == "PATH" => {
            for part in value.split(':') {
                if !existing.split(':').any(|current| current == part) {
                    existing.push(':');
                    existing.push_str(part);
                }
            }
        }
        Some(existing) => {
            return Err(format!(
                "the admitted toolchains disagree on `{name}` (`{existing}` versus `{value}`)"
            ));
        }
    }
    Ok(())
}

/// Lower-case hex, because a digest that reaches a record has one spelling.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}
