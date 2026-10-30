//! Which repositories the operator has said may supply a profile.
//!
//! A repository profile is a file that arrives with the code, so it is read only for a path the
//! operator named in a record of their own. The record is a file this binary writes, beside the
//! operator's `harness.toml` and never inside it: the operator's configuration is theirs, and a
//! tool that edits it is one whose every rule has to be re-read after each command.
//!
//! # Keyed by the canonical path
//!
//! `trust` and `untrust` canonicalise what they are given, and a run compares its canonical
//! workspace. So a symlink and its target are one repository, and a record for `/src/a` says
//! nothing about `/src/a/inner` or `/src/b`: trust is per path, never per subtree.
//!
//! # What trust is not
//!
//! It is a statement about a **path**, not about the file's contents: a trusted repository's file
//! is read as it is at each run. That is bearable only because a repository profile may carry
//! nothing that decides what a run may do (`profile::REPOSITORY_KEYS`).
//!
//! # Whose record it is
//!
//! A record another user could write would let them make a repository's profile apply to this
//! user's runs, so one that is not owned by the user running this, or that its group or anyone
//! else may write, is refused by name rather than read (`judge`). Changes are made under an
//! exclusive lock on [`LOCK_FILE`], so two `trust` or `untrust` commands at the same moment cannot
//! each read the record and write back their own copy over the other's.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

/// The trust record's name, in the same directory as the operator's `harness.toml`.
pub const TRUST_FILE: &str = "harness-trust.toml";

/// The file `trust` and `untrust` hold an exclusive lock on while they read, change and replace
/// the record. Beside the record, created `0600`, and never removed: removing it would let a
/// second command lock a new file while the first still holds the old one.
pub const LOCK_FILE: &str = "harness-trust.toml.lock";

/// Whether a record with this owner and mode may say what the user `me` trusts.
///
/// Refused when another user owns it, or when its group or anyone else may write it: whoever can
/// write the record decides which repositories' profiles apply. Readable by others is allowed;
/// the record holds paths, not secrets.
///
/// # Errors
///
/// Names the record, the owner or the mode, and the command that fixes it.
fn judge(record: &Path, owner: u32, mode: u32, me: u32) -> Result<(), String> {
    if owner != me {
        return Err(format!(
            "the trust record `{}` is owned by uid {owner}, and this runs as uid {me}. A record \
             another user owns cannot say which repositories this user trusts, so it was not \
             read. Remove it and trust again with `b10x-harness profiles trust <PATH>`.",
            record.display()
        ));
    }
    if mode & 0o022 != 0 {
        return Err(format!(
            "the trust record `{}` is writable by its group or by others (mode {:o}), so anyone \
             who can write it decides which repositories' profiles apply. It was not read. \
             `chmod 600` it.",
            record.display(),
            mode & 0o7777
        ));
    }
    Ok(())
}

/// The effective user id of this process, from `/proc/self/status`.
///
/// Read there rather than through `libc`, which this crate does not depend on. A process that
/// cannot read it refuses the record rather than trusting one it could not judge.
#[cfg(target_os = "linux")]
fn effective_uid() -> Result<u32, String> {
    let status = std::fs::read_to_string("/proc/self/status")
        .map_err(|error| format!("reading `/proc/self/status` for the effective uid: {error}"))?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|ids| ids.split_whitespace().nth(1))
        .and_then(|effective| effective.parse().ok())
        .ok_or_else(|| "`/proc/self/status` names no effective uid".to_owned())
}

/// [`judge`] on the record as it was opened.
#[cfg(unix)]
fn judge_opened(record: &Path, opened: &std::fs::File) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt as _;

    let held = opened
        .metadata()
        .map_err(|error| format!("reading the trust record `{}`: {error}", record.display()))?;
    #[cfg(target_os = "linux")]
    let me = effective_uid()?;
    // Elsewhere there is no effective uid without `libc`; the mode is still judged.
    #[cfg(not(target_os = "linux"))]
    let me = held.uid();
    judge(record, held.uid(), held.mode(), me)
}

#[cfg(not(unix))]
fn judge_opened(_record: &Path, _opened: &std::fs::File) -> Result<(), String> {
    Ok(())
}

/// The repositories whose own profile a run may read, by canonical path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trust {
    #[serde(default)]
    repositories: BTreeSet<String>,
}

impl Trust {
    /// Whether this canonical workspace path was trusted.
    ///
    /// A path that is not UTF-8 is never trusted: the record is TOML, which holds only UTF-8, so
    /// `trust` refuses one and no record can name it.
    #[must_use]
    pub fn admits(&self, canonical: &Path) -> bool {
        canonical
            .to_str()
            .is_some_and(|path| self.repositories.contains(path))
    }

    /// The record as TOML: one array, one path per line, sorted.
    ///
    /// Written here rather than by a serialiser, so the only thing between this and the file is a
    /// string escape that [`save`] checks by parsing the result back before it replaces anything.
    fn render(&self) -> String {
        if self.repositories.is_empty() {
            return "repositories = []\n".to_owned();
        }
        let mut text = String::from("repositories = [\n");
        for path in &self.repositories {
            let _ = writeln!(text, "    {},", basic_string(path));
        }
        text.push_str("]\n");
        text
    }
}

/// A TOML basic string holding exactly `text`.
fn basic_string(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for character in text.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            control if control.is_control() => {
                let _ = write!(quoted, "\\u{:04X}", u32::from(control));
            }
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

/// What `trust` or `untrust` did, for the line it prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    /// The path the record is keyed by.
    pub repository: PathBuf,
    /// Whether the record changed.
    pub changed: bool,
    /// Further entries `untrust` removed: the other spellings the same path named.
    pub also: Vec<PathBuf>,
}

/// Reads the trust record, or an empty one where there is none.
///
/// A record that is there and does not parse is an error, not an empty record: treating it as
/// empty would quietly stop applying every trusted repository's profile, and a run would differ
/// from the last one for a reason nobody could see.
///
/// # Errors
///
/// Names the record when it is there and cannot be read or parsed, and when another user owns it
/// or may write it (`judge`). A parse failure is named by line and column and never quotes the
/// record's text.
pub fn load(record: &Path) -> Result<Trust, String> {
    use std::io::Read as _;

    let unreadable =
        |error: std::io::Error| format!("reading the trust record `{}`: {error}", record.display());
    let mut opened = match std::fs::File::open(record) {
        Ok(opened) => opened,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Trust::default());
        }
        Err(error) => return Err(unreadable(error)),
    };
    judge_opened(record, &opened)?;
    let mut text = String::new();
    opened.read_to_string(&mut text).map_err(unreadable)?;
    toml::from_str(&text).map_err(|error| {
        format!(
            "the trust record `{}` does not parse, at {}. It is written by `b10x-harness profiles \
             trust` and `profiles untrust`; correct it or remove it.",
            record.display(),
            crate::profile::parse_failure(&text, &error)
        )
    })
}

/// Holds the exclusive lock on [`LOCK_FILE`] beside `record` until it is dropped.
///
/// The directory is created first (`0700`, when this creates it), and the lock file `0600`.
///
/// # Errors
///
/// Names the directory or lock file it could not create, open or lock.
fn lock(record: &Path) -> Result<std::fs::File, String> {
    let directory = record
        .parent()
        .ok_or_else(|| format!("`{}` has no directory to live in", record.display()))?;
    create_private_directory(directory)?;
    let path = directory.join(LOCK_FILE);
    let file = open_lock(&path)?;
    file.lock()
        .map_err(|error| format!("locking `{}`: {error}", path.display()))?;
    Ok(file)
}

#[cfg(unix)]
fn open_lock(path: &Path) -> Result<std::fs::File, String> {
    use std::os::unix::fs::OpenOptionsExt as _;

    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
        .map_err(|error| format!("opening `{}`: {error}", path.display()))
}

#[cfg(not(unix))]
fn open_lock(path: &Path) -> Result<std::fs::File, String> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|error| format!("opening `{}`: {error}", path.display()))
}

/// Writes the record whole, atomically, readable by its owner alone.
///
/// The text is parsed back and compared before anything is replaced; then it goes to a new file
/// in the same directory, created `0600`, and is renamed over the record, so a crash in the middle
/// leaves the old record intact. A directory this has to create is created `0700`.
///
/// # Errors
///
/// Names the file or directory it could not write.
fn save(record: &Path, trust: &Trust) -> Result<(), String> {
    let text = trust.render();
    let reread: Trust = toml::from_str(&text)
        .map_err(|error| format!("the trust record this build rendered does not parse: {error}"))?;
    if reread != *trust {
        return Err(
            "the trust record this build rendered does not read back as written".to_owned(),
        );
    }
    let directory = record
        .parent()
        .ok_or_else(|| format!("`{}` has no directory to live in", record.display()))?;
    create_private_directory(directory)?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());
    let temporary = directory.join(format!(".{TRUST_FILE}.{}.{nanos}.tmp", std::process::id()));
    if let Err(error) = write_private(&temporary, text.as_bytes()) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    std::fs::rename(&temporary, record).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        format!("replacing the trust record `{}`: {error}", record.display())
    })
}

/// Adds a repository to the record.
///
/// # Errors
///
/// Names a path that does not exist, is not a directory or is not UTF-8, and a record that cannot
/// be read or written.
pub fn trust(path: &Path, record: &Path) -> Result<Changed, String> {
    let repository = path.canonicalize().map_err(|error| {
        format!(
            "`{}` cannot be trusted: {error}. Trust names a repository directory that exists.",
            path.display()
        )
    })?;
    if !repository.is_dir() {
        return Err(format!(
            "`{}` is not a directory. A repository profile lives at \
             `<workspace>/.b10x/harness.toml`, so what is trusted is the workspace.",
            repository.display()
        ));
    }
    let key = repository
        .to_str()
        .ok_or_else(|| {
            format!(
                "`{}` is not UTF-8, and the trust record is TOML, which holds only UTF-8 text.",
                repository.display()
            )
        })?
        .to_owned();
    let _held = lock(record)?;
    let mut trust = load(record)?;
    let changed = trust.repositories.insert(key);
    if changed {
        save(record, &trust)?;
    }
    Ok(Changed {
        repository,
        changed,
        also: Vec::new(),
    })
}

/// Removes a repository from the record.
///
/// Every entry the path can name is removed, so a repository that moved or was deleted after it
/// was trusted can still be untrusted by the spelling it was trusted by:
///
/// - the path resolved the way `trust` resolves it: canonicalised when it exists; otherwise
///   resolved component by component as the kernel would, `..` landing where the kernel's `..`
///   lands and a dangling symbolic link read against the directory it really sits in, with only
///   the part that does not exist taken as spelled (`resolved`);
/// - the location the path names, its parent resolved that way and its last component kept, which
///   is the entry for a trusted directory whose path has since become a symbolic link.
///
/// # Errors
///
/// Names a path that cannot be made absolute, and a record that cannot be read or written.
pub fn untrust(path: &Path, record: &Path) -> Result<Changed, String> {
    let unresolved =
        |error: std::io::Error| format!("`{}` cannot be resolved: {error}", path.display());
    let spelled = std::path::absolute(path).map_err(unresolved)?;
    let repository = resolved(&spelled);
    let mut names = vec![repository.clone()];
    if let (Some(parent), Some(last)) = (spelled.parent(), spelled.file_name()) {
        names.push(resolved(parent).join(last));
    }
    let _held = lock(record)?;
    let mut trust = load(record)?;
    let mut removed = Vec::new();
    for name in names {
        if name
            .to_str()
            .is_some_and(|key| trust.repositories.remove(key))
        {
            removed.push(name);
        }
    }
    let changed = !removed.is_empty();
    if changed {
        save(record, &trust)?;
    }
    let mut removed = removed.into_iter();
    Ok(Changed {
        repository: removed.next().unwrap_or(repository),
        changed,
        also: removed.collect(),
    })
}

/// How many symbolic links [`resolve`] follows before it takes the rest as spelled, as the kernel
/// gives up.
const LINK_LIMIT: usize = 40;

/// An absolute `path` canonicalised when it exists, and otherwise [`resolve`]d.
fn resolved(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| resolve(path, 0))
}

/// An absolute `path` resolved the way the kernel resolves it, for as long as it exists.
///
/// Component by component, onto a directory that holds no symbolic link: `.` is skipped; `..`
/// leaves that directory, so it lands where the kernel's `..` lands; a component that is a link is
/// replaced by its target, read against the directory the link really sits in, and the result is
/// resolved again, up to [`LINK_LIMIT`] links. From the first component that does not exist the
/// rest is taken lexically ([`spelled`]): that is the one part no filesystem can resolve.
fn resolve(path: &Path, links: usize) -> PathBuf {
    let mut resolved = PathBuf::new();
    let mut components = path.components();
    while let Some(component) = components.next() {
        match component {
            Component::Prefix(_) | Component::RootDir => resolved.push(component),
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(name) => {
                let candidate = resolved.join(name);
                let Ok(found) = std::fs::symlink_metadata(&candidate) else {
                    return spelled(candidate, components);
                };
                if found.file_type().is_symlink() && links < LINK_LIMIT {
                    let Ok(target) = std::fs::read_link(&candidate) else {
                        return spelled(candidate, components);
                    };
                    // An absolute target replaces `resolved`; a relative one is read against it.
                    let mut next = resolved.join(target);
                    next.extend(components);
                    return resolve(&next, links + 1);
                }
                resolved = candidate;
            }
        }
    }
    resolved
}

/// `base` with `rest` appended as spelled: `.` skipped and `..` taken against the component before
/// it, since nothing here exists for the kernel to resolve.
fn spelled<'a>(mut base: PathBuf, rest: impl Iterator<Item = Component<'a>>) -> PathBuf {
    for component in rest {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                base.pop();
            }
            other => base.push(other),
        }
    }
    base
}

#[cfg(unix)]
fn create_private_directory(directory: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt as _;

    if directory.is_dir() {
        return Ok(());
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)
        .map_err(|error| format!("creating `{}`: {error}", directory.display()))
}

#[cfg(not(unix))]
fn create_private_directory(directory: &Path) -> Result<(), String> {
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("creating `{}`: {error}", directory.display()))
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| format!("creating `{}`: {error}", path.display()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("writing `{}`: {error}", path.display()))
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|error| format!("writing `{}`: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory standing in for `$XDG_CONFIG_HOME/b10x`, and a repository beside it.
    struct Machine {
        root: tempfile::TempDir,
    }

    impl Machine {
        fn new() -> Self {
            Self {
                root: tempfile::tempdir().expect("a temporary directory"),
            }
        }

        fn record(&self) -> PathBuf {
            self.root
                .path()
                .join("config")
                .join("b10x")
                .join(TRUST_FILE)
        }

        fn repository(&self, name: &str) -> PathBuf {
            let path = self.root.path().join(name);
            std::fs::create_dir_all(&path).expect("a repository directory");
            path
        }
    }

    #[test]
    fn trust_then_untrust_round_trips_through_the_record() {
        let machine = Machine::new();
        let repository = machine.repository("repo");
        let canonical = repository.canonicalize().expect("canonical");

        let trusted = trust(&repository, &machine.record()).expect("trusted");
        assert_eq!(trusted.repository, canonical);
        assert!(trusted.changed);
        assert!(load(&machine.record()).expect("reads").admits(&canonical));

        let again = trust(&repository, &machine.record()).expect("trusted twice");
        assert!(!again.changed, "trusting twice changes nothing");

        let removed = untrust(&repository, &machine.record()).expect("untrusted");
        assert_eq!(removed.repository, canonical);
        assert!(removed.changed);
        assert!(!load(&machine.record()).expect("reads").admits(&canonical));

        let again = untrust(&repository, &machine.record()).expect("untrusted twice");
        assert!(
            !again.changed,
            "untrusting what is not trusted changes nothing"
        );
    }

    #[test]
    fn untrust_removes_a_deleted_repository_named_through_dot_dot_or_a_trailing_slash() {
        let machine = Machine::new();
        let here = machine.repository("here");
        for spelling in ["gone/", "here/../gone", "./here/./../gone//"] {
            let gone = machine.repository("gone");
            let canonical = gone.canonicalize().expect("canonical");
            trust(&gone, &machine.record()).expect("trusted");
            std::fs::remove_dir_all(&gone).expect("deleted");

            let removed =
                untrust(&machine.root.path().join(spelling), &machine.record()).expect("untrust");
            assert!(removed.changed, "{spelling}: {removed:?}");
            assert_eq!(removed.repository, canonical, "{spelling}");
            assert!(!load(&machine.record()).expect("reads").admits(&canonical));
        }
        assert!(here.exists(), "nothing but the record was touched");
    }

    #[cfg(unix)]
    #[test]
    fn untrust_follows_a_dangling_link_and_a_symlinked_parent_to_the_entry() {
        let machine = Machine::new();
        let real = machine.repository("real");
        let canonical = real.canonicalize().expect("canonical");
        let link = machine.root.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("a symlink");
        let nested = machine.repository("real/work");
        let nested_canonical = nested.canonicalize().expect("canonical");
        trust(&link, &machine.record()).expect("trusted through the link");
        trust(&link.join("work"), &machine.record()).expect("trusted through the parent link");
        std::fs::remove_dir_all(&nested).expect("deleted");

        let removed = untrust(&link.join("work"), &machine.record()).expect("untrust");
        assert_eq!(removed.repository, nested_canonical);
        assert!(removed.changed);

        std::fs::remove_dir_all(&real).expect("deleted, leaving the link dangling");
        let removed = untrust(&link, &machine.record()).expect("untrust");
        assert_eq!(removed.repository, canonical);
        assert!(removed.changed);
        assert_eq!(load(&machine.record()).expect("reads"), Trust::default());
    }

    #[cfg(unix)]
    #[test]
    fn untrust_takes_dot_dot_after_a_link_the_way_the_kernel_does_and_spares_the_link_sibling() {
        let machine = Machine::new();
        let project = machine.repository("data/x/proj");
        let named = machine.repository("data/x/sibling");
        let bystander = machine.repository("sibling");
        let link = machine.root.path().join("proj-link");
        std::os::unix::fs::symlink(&project, &link).expect("a symlink");
        let spelled = link.join("..").join("sibling");
        trust(&spelled, &machine.record()).expect("trusted");
        trust(&bystander, &machine.record()).expect("trusted");

        let removed = untrust(&spelled, &machine.record()).expect("untrust");

        assert_eq!(removed.repository, named.canonicalize().expect("canonical"));
        assert!(removed.also.is_empty(), "{removed:?}");
        let left = load(&machine.record()).expect("reads");
        assert!(left.admits(&bystander.canonicalize().expect("canonical")));
    }

    #[cfg(unix)]
    #[test]
    fn untrust_reads_a_dangling_relative_link_against_the_directory_it_really_sits_in() {
        let machine = Machine::new();
        let real_work = machine.repository("data/work");
        let release = machine.repository("data/releases/v2");
        let entry = release.canonicalize().expect("canonical");
        std::os::unix::fs::symlink("../releases/v2", real_work.join("current")).expect("a link");
        let work = machine.root.path().join("work");
        std::os::unix::fs::symlink(&real_work, &work).expect("a symlinked directory");
        trust(&work.join("current"), &machine.record()).expect("trusted");
        std::fs::remove_dir_all(&release).expect("deleted, leaving the link dangling");

        let removed = untrust(&work.join("current"), &machine.record()).expect("untrust");

        assert!(removed.changed, "{removed:?}");
        assert_eq!(removed.repository, entry);
        assert_eq!(load(&machine.record()).expect("reads"), Trust::default());
    }

    #[cfg(unix)]
    #[test]
    fn untrust_of_a_cycle_of_links_ends_and_changes_nothing() {
        let machine = Machine::new();
        let directory = machine.repository("loop");
        std::os::unix::fs::symlink("b/x", directory.join("a")).expect("a link");
        std::os::unix::fs::symlink("a/y", directory.join("b")).expect("a link");
        let removed = untrust(&directory.join("a").join("z"), &machine.record()).expect("untrust");
        assert!(!removed.changed, "{removed:?}");
    }

    #[cfg(unix)]
    #[test]
    fn untrust_of_a_path_that_became_a_link_removes_both_entries_it_names() {
        let machine = Machine::new();
        let work = machine.repository("work");
        let moved = machine.repository("moved");
        let (work_canonical, moved_canonical) = (
            work.canonicalize().expect("canonical"),
            moved.canonicalize().expect("canonical"),
        );
        trust(&work, &machine.record()).expect("trusted");
        trust(&moved, &machine.record()).expect("trusted");
        std::fs::remove_dir_all(&work).expect("removed");
        std::os::unix::fs::symlink(&moved, &work).expect("the old path now a link");

        let removed = untrust(&work, &machine.record()).expect("untrust");
        assert!(removed.changed);
        assert_eq!(
            removed.repository, moved_canonical,
            "where it points, first"
        );
        assert_eq!(
            removed.also,
            vec![work_canonical],
            "and the location it names"
        );
        assert_eq!(load(&machine.record()).expect("reads"), Trust::default());
    }

    #[test]
    fn a_missing_record_trusts_nothing() {
        let machine = Machine::new();
        let repository = machine.repository("repo");
        let trust = load(&machine.record()).expect("an absent record is an empty one");
        assert!(!trust.admits(&repository.canonicalize().expect("canonical")));
    }

    #[test]
    fn a_path_that_does_not_exist_cannot_be_trusted() {
        let machine = Machine::new();
        let missing = machine.root.path().join("not-there");
        let error = trust(&missing, &machine.record()).expect_err("refused");
        assert!(error.contains("not-there"), "{error}");
        assert!(
            !machine.record().exists(),
            "a refused trust writes no record"
        );
    }

    #[cfg(unix)]
    #[test]
    fn trust_is_keyed_by_the_canonical_path_through_a_symlink() {
        let machine = Machine::new();
        let target = machine.repository("real");
        let link = machine.root.path().join("link");
        std::os::unix::fs::symlink(&target, &link).expect("a symlink");
        let canonical = target.canonicalize().expect("canonical");

        let trusted = trust(&link, &machine.record()).expect("trusted");
        assert_eq!(
            trusted.repository, canonical,
            "the link's target is recorded"
        );
        let record = load(&machine.record()).expect("reads");
        assert!(record.admits(&canonical));
        assert!(
            !record.admits(&link),
            "the record is compared with canonical paths, and the link is not one"
        );

        let removed = untrust(&link, &machine.record()).expect("untrusted");
        assert!(
            removed.changed,
            "untrusting through the link removes the target"
        );
    }

    #[test]
    fn a_record_for_one_path_does_not_admit_another() {
        let machine = Machine::new();
        let a = machine.repository("a");
        let b = machine.repository("b");
        let nested = machine.repository("a/inner");
        trust(&a, &machine.record()).expect("trusted");
        let record = load(&machine.record()).expect("reads");
        assert!(record.admits(&a.canonicalize().expect("canonical")));
        assert!(!record.admits(&b.canonicalize().expect("canonical")));
        assert!(
            !record.admits(&nested.canonicalize().expect("canonical")),
            "trust is per path, not per subtree"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_record_is_private_and_its_directory_is_created_private() {
        use std::os::unix::fs::PermissionsExt as _;

        let machine = Machine::new();
        let repository = machine.repository("repo");
        trust(&repository, &machine.record()).expect("trusted");
        let mode = |path: &Path| {
            std::fs::metadata(path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode(&machine.record()), 0o600);
        assert_eq!(
            mode(machine.record().parent().expect("a directory")),
            0o700,
            "a directory this created is its owner's alone"
        );
        let lock = machine.record().with_file_name(LOCK_FILE);
        assert_eq!(mode(&lock), 0o600, "the lock file is its owner's alone too");
        let leftovers: Vec<_> = std::fs::read_dir(machine.record().parent().expect("dir"))
            .expect("lists")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .filter(|name| name != TRUST_FILE && name != LOCK_FILE)
            .collect();
        assert!(
            leftovers.is_empty(),
            "the temporary file is renamed, not left behind: {leftovers:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_configuration_directory_keeps_its_mode() {
        use std::os::unix::fs::PermissionsExt as _;

        let machine = Machine::new();
        let directory = machine.record().parent().expect("dir").to_path_buf();
        std::fs::create_dir_all(&directory).expect("the operator's directory");
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
        trust(&machine.repository("repo"), &machine.record()).expect("trusted");
        let mode = std::fs::metadata(&directory)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            mode, 0o755,
            "an operator's existing directory is not chmodded"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_record_others_can_write_is_refused_by_name_and_not_trusted() {
        use std::os::unix::fs::PermissionsExt as _;

        for loose in [0o666, 0o620, 0o602] {
            let machine = Machine::new();
            let repository = machine.repository("repo");
            trust(&repository, &machine.record()).expect("trusted");
            std::fs::set_permissions(machine.record(), std::fs::Permissions::from_mode(loose))
                .expect("chmod");
            let error = load(&machine.record()).expect_err("refused");
            assert!(error.contains(TRUST_FILE), "{loose:o}: {error}");
            assert!(error.contains("writable"), "{loose:o}: {error}");
            let error = trust(&machine.repository("other"), &machine.record())
                .expect_err("trust refuses to build on it");
            assert!(error.contains(TRUST_FILE), "{loose:o}: {error}");
        }
    }

    #[test]
    fn a_record_owned_by_someone_else_is_refused_by_name() {
        let record = Path::new("/config/b10x/harness-trust.toml");
        let error = judge(record, 0, 0o600, 1000).expect_err("owned by another user");
        assert!(error.contains(TRUST_FILE), "{error}");
        assert!(error.contains("owned by uid 0"), "{error}");
        assert!(error.contains("1000"), "{error}");
        judge(record, 1000, 0o600, 1000).expect("the user's own private record");
        judge(record, 1000, 0o644, 1000).expect("readable by others is not writable by them");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_effective_user_is_the_owner_of_a_file_this_process_creates() {
        use std::os::unix::fs::MetadataExt as _;

        let machine = Machine::new();
        let created = machine.root.path().join("mine");
        std::fs::write(&created, "").expect("written");
        assert_eq!(
            effective_uid().expect("read"),
            std::fs::metadata(&created).expect("metadata").uid()
        );
    }

    #[test]
    fn a_record_that_does_not_parse_is_named_by_line_and_column_and_never_quoted() {
        let machine = Machine::new();
        std::fs::create_dir_all(machine.record().parent().expect("dir")).expect("dir");
        std::fs::write(
            machine.record(),
            "repositories = [\n    \"/a\",\n    SECRET-0004\n]\n",
        )
        .expect("written");
        let error = load(&machine.record()).expect_err("refused");
        assert!(error.contains(TRUST_FILE), "{error}");
        assert!(error.contains("line 3, column"), "{error}");
        assert!(!error.contains("SECRET"), "{error}");
        assert!(!error.contains('\n'), "no snippet beneath it: {error}");
    }

    #[test]
    fn the_operator_configuration_is_byte_identical_after_trust() {
        let machine = Machine::new();
        let config = machine.record().with_file_name("harness.toml");
        std::fs::create_dir_all(config.parent().expect("dir")).expect("dir");
        let bytes = b"[default]\nprovider = \"claude\"   # the operator's own words\n".to_vec();
        std::fs::write(&config, &bytes).expect("written");
        let repository = machine.repository("repo");

        trust(&repository, &machine.record()).expect("trusted");
        untrust(&repository, &machine.record()).expect("untrusted");

        assert_eq!(std::fs::read(&config).expect("reads"), bytes);
    }

    #[test]
    fn a_path_toml_would_need_to_escape_survives_the_record() {
        let machine = Machine::new();
        let repository = machine.repository("quote \" back \\ slash");
        let canonical = repository.canonicalize().expect("canonical");
        trust(&repository, &machine.record()).expect("trusted");
        assert!(load(&machine.record()).expect("reads").admits(&canonical));
    }

    #[test]
    fn a_record_this_build_cannot_read_is_named_rather_than_treated_as_empty() {
        let machine = Machine::new();
        std::fs::create_dir_all(machine.record().parent().expect("dir")).expect("dir");
        std::fs::write(machine.record(), "repositories = [\"/a\"]\nall = true\n").expect("written");
        let error = load(&machine.record()).expect_err("refused");
        assert!(error.contains(TRUST_FILE), "{error}");
        assert!(error.contains("all"), "{error}");
    }
}
