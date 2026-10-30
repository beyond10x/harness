//! What a run is permitted to do, from a file the operator wrote.
//!
//! # The line between this and `provider.rs`
//!
//! A provider says where to talk; a **profile says what may happen**. That is why the provider
//! collection ships compiled in and nothing here does: there is no permission bundle inside the
//! binary, and every rule a run obeys is in a file you can read, diff and version.
//!
//! # Why a file is safer than the flags it replaces, not more dangerous
//!
//! The instinct is the other way round, so the evidence: run W4-2 lost all eight of its post-fix
//! sessions to a hand-assembled command line that dropped `--plugin-dir`, and ran unenforced while
//! looking clean. Sixteen flags retyped is where that lives. A named table, digested into the run's
//! own record, is the same declaration made once and attributable afterwards.
//!
//! That last clause is the condition, not a bonus. A profile may carry permissions **because**
//! `session.started` names which profiles ran and hashes what they said. If that ever stops being
//! true, this stops being safe.
//!
//! # `write` is one key, and it is off
//!
//! Whether a run may change anything is a single switch, so a reader of a config never has to
//! assemble the answer from four keys. Off, the catalogue is the four read-only tools whatever else
//! the table says — and a profile that declares programs without `write` is told so at startup
//! rather than discovering it when the model is refused mid-run.
//!
//! # A repository's own file carries plumbing, never permission
//!
//! `<workspace>/.b10x/harness.toml` arrives with the code, so it is read only for a workspace the
//! operator trusted by canonical path (`trust.rs`), and it may set only [`REPOSITORY_KEYS`]:
//! `provider`, `model` and `max-turns`. Every other key and table is refused by name, so nothing
//! that decides what a run may do, and no endpoint or credential, comes from a file the operator
//! did not write. It sits above `[default]` and beneath every `-p`, and the record names it like
//! any other profile.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use harness_substrate::inert;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::provider::ProviderOverride;

/// The config file, under one directory shared by every b10x tool.
const CONFIG_DIR: &str = "b10x";
const CONFIG_FILE: &str = "harness.toml";

/// What a run may do, as the operator wrote it.
///
/// Every field is optional because a profile is a *partial* declaration layered over `[default]`.
/// `deny_unknown_fields` is the load-bearing attribute: a key this build does not read is a rule
/// its author wrote and the run would not apply, which is the failure `skills.rs` refuses for the
/// same reason.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Profile {
    /// Only on a `[[profiles]]` entry; `[default]` is named by its table.
    #[serde(default)]
    pub name: Option<String>,
    /// Which provider to reach. A profile may set it, so one `-p` can switch model and endpoint.
    pub provider: Option<String>,
    pub model: Option<String>,
    /// May this run change anything at all.
    pub write: Option<bool>,
    pub approve_up_to: Option<String>,
    pub allow_program: Option<Vec<String>>,
    /// Build toolchains to admit. Replaced as a set by a later profile, never merged.
    pub toolchains: Option<Vec<String>>,
    /// Explicit operator-authored toolchain specifications. Later profiles replace the set.
    pub toolchain_specs: Option<Vec<PathBuf>>,
    pub write_scope: Option<Vec<String>>,
    pub plugin_dir: Option<Vec<String>>,
    pub max_turns: Option<u64>,
}

/// The whole config document.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Config {
    #[serde(default)]
    pub default: Profile,
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderOverride>,
}

/// Which profile a resolved value came from, for the run's own record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRef {
    pub name: String,
    pub source: String,
    pub sha256: String,
}

/// Where the config lives: `$XDG_CONFIG_HOME/b10x/harness.toml`, or `~/.config/b10x/harness.toml`.
#[must_use]
pub fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join(CONFIG_DIR).join(CONFIG_FILE))
}

/// The directory a repository keeps its own profile in, under the run's workspace.
const REPOSITORY_DIR: &str = ".b10x";

/// The keys a repository profile may set: plumbing, never permission.
pub const REPOSITORY_KEYS: [&str; 3] = ["provider", "model", "max-turns"];

/// The name a repository profile is recorded under in `session.started.profiles`.
pub const REPOSITORY_PROFILE: &str = "repository";

/// The most a repository profile may hold, in bytes: 64 KiB.
///
/// Three short keys fit in well under a kilobyte. The cap is what keeps a profile that is a
/// growing file, or one swapped for something endless between the checks and the read, from being
/// read without a bound.
pub const REPOSITORY_LIMIT: u64 = 64 * 1024;

/// Where a workspace's own profile would be: `<workspace>/.b10x/harness.toml`.
#[must_use]
pub fn repository_file(workspace: &Path) -> PathBuf {
    workspace.join(REPOSITORY_DIR).join(CONFIG_FILE)
}

/// Where the trust record lives: beside the operator's config, never inside it.
#[must_use]
pub fn trust_path() -> Option<PathBuf> {
    config_path().map(|config| config.with_file_name(crate::trust::TRUST_FILE))
}

/// A repository profile the operator trusted, as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    /// Only `provider`, `model` and `max-turns` are ever set.
    pub profile: Profile,
    /// The file it was read from, for the run's record.
    pub source: String,
}

/// What a workspace's own profile came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositoryProfile {
    /// There is no `<workspace>/.b10x/harness.toml`.
    Absent,
    /// There is one, and the operator has not trusted the workspace, so it was not read.
    Untrusted { file: PathBuf, workspace: PathBuf },
    /// There is one, the workspace is trusted, and it carries only admitted keys.
    Trusted(Box<Repository>),
}

/// The workspace's own profile, read only when the trust record names the canonical workspace.
///
/// # Errors
///
/// Names a trust record that cannot be read, and a trusted file that cannot be read, does not
/// parse, or carries a key or table a repository profile may not.
pub fn repository(
    workspace: &Path,
    trust_record: Option<&Path>,
) -> Result<RepositoryProfile, String> {
    // A workspace that is not there holds no file; the run refuses it by name later.
    let Ok(workspace) = workspace.canonicalize() else {
        return Ok(RepositoryProfile::Absent);
    };
    let file = repository_file(&workspace);
    match std::fs::symlink_metadata(&file) {
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(RepositoryProfile::Absent);
        }
        _ => {}
    }
    let trusted = match trust_record {
        Some(record) => crate::trust::load(record)?.admits(&workspace),
        None => false,
    };
    if !trusted {
        // Not read, not parsed: an untrusted file is text nobody vouched for, and a parser is
        // the first thing such text reaches.
        return Ok(RepositoryProfile::Untrusted { file, workspace });
    }
    let profile = load_repository(&workspace)?;
    Ok(RepositoryProfile::Trusted(Box::new(Repository {
        profile,
        source: file.display().to_string(),
    })))
}

/// Reads a workspace's own profile, refusing every key and table it may not carry, by name.
///
/// The text comes from `read_repository_text`: a regular file at exactly
/// `<canonical workspace>/.b10x/harness.toml`, at most [`REPOSITORY_LIMIT`] bytes. It is parsed as
/// a plain table first, so a refusal can name **every** key and table it will not apply at once,
/// rather than one per attempt the way a typed parse would. The admitted keys' types are checked
/// by key before the `[default]` table becomes a [`Profile`], so no refusal quotes a value.
///
/// # Errors
///
/// Names the file and every refused key or table, or what is wrong with it. A refusal never
/// quotes the file's text.
pub fn load_repository(workspace: &Path) -> Result<Profile, String> {
    let workspace = workspace.canonicalize().map_err(|error| {
        format!(
            "the workspace `{}` cannot be resolved: {error}",
            shown(workspace)
        )
    })?;
    let file = repository_file(&workspace);
    let text = read_repository_text(&file)?;
    parse_repository(&file, &text)
}

/// The text of `file`, which must be `<canonical workspace>/.b10x/harness.toml` itself.
///
/// Checked before it is opened: a symbolic link, a FIFO, a device or a directory there is refused
/// by name, and so is a path that resolves anywhere else, which is what a symlinked `.b10x/`
/// directory does. Checked again on the opened file: it must be the same file the path named a
/// moment earlier. Then read through [`std::io::Read::take`], so a file that grows past
/// [`REPOSITORY_LIMIT`] while it is read is refused rather than read further.
///
/// What is left: a file swapped for a FIFO between the first check and the open can make the open
/// wait for a writer. The window is two system calls wide.
fn read_repository_text(file: &Path) -> Result<String, String> {
    use std::io::Read as _;

    let refused = |what: &str| {
        format!(
            "the repository profile `{}` {what}. A repository profile is read only as a regular \
             file of at most 64 KiB, at exactly `.b10x/harness.toml` inside the workspace. Nothing \
             from this file was applied.",
            shown(file)
        )
    };
    let unreadable = |error: std::io::Error| {
        format!("reading the repository profile `{}`: {error}", shown(file))
    };
    let named = std::fs::symlink_metadata(file).map_err(unreadable)?;
    if named.file_type().is_symlink() {
        return Err(refused("is a symbolic link"));
    }
    if !named.file_type().is_file() {
        return Err(refused("is not a regular file"));
    }
    let resolved = file.canonicalize().map_err(unreadable)?;
    if resolved != file {
        return Err(refused(&format!(
            "resolves to `{}`, through a symbolic link on the way",
            shown(&resolved)
        )));
    }
    let mut opened = std::fs::File::open(file).map_err(unreadable)?;
    let held = opened.metadata().map_err(unreadable)?;
    if !held.is_file() || !same_file(&named, &held) {
        return Err(refused("changed while it was being opened"));
    }
    if held.len() > REPOSITORY_LIMIT {
        return Err(refused(&format!("is {} bytes", held.len())));
    }
    let mut bytes = Vec::new();
    (&mut opened)
        .take(REPOSITORY_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(unreadable)?;
    if bytes.len() as u64 > REPOSITORY_LIMIT {
        return Err(refused("grew past 64 KiB while it was being read"));
    }
    String::from_utf8(bytes).map_err(|error| {
        refused(&format!(
            "is not UTF-8 text (byte {})",
            error.utf8_error().valid_up_to()
        ))
    })
}

/// Whether two metadata describe one file: same device, same inode.
#[cfg(unix)]
fn same_file(named: &std::fs::Metadata, held: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    named.dev() == held.dev() && named.ino() == held.ino()
}

#[cfg(not(unix))]
fn same_file(named: &std::fs::Metadata, held: &std::fs::Metadata) -> bool {
    named.len() == held.len()
}

/// A TOML parse failure as `line L, column C: <the parser's message>`.
///
/// Never the source line or a snippet under it, which is what the parser's own `Display` adds:
/// a file reached by mistake, a credential store for one, would have its text printed into a log.
pub(crate) fn parse_failure(text: &str, error: &toml::de::Error) -> String {
    let message = error
        .message()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    let Some(span) = error.span() else {
        return message;
    };
    let before = text.get(..span.start).unwrap_or(text);
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .map_or(0, |tail| tail.chars().count())
        + 1;
    format!("line {line}, column {column}: {message}")
}

/// [`load_repository`] once the text is in hand.
fn parse_repository(file: &Path, text: &str) -> Result<Profile, String> {
    let document: toml::Table = toml::from_str(text).map_err(|error| {
        format!(
            "the repository profile `{}` does not parse, at {}. Nothing from this file was \
             applied.",
            shown(file),
            parse_failure(text, &error)
        )
    })?;
    // A refused name is the file's text, and a quoted TOML key may hold any character: it is named
    // through `inert`, so a control or bidi character in it is shown escaped and never reaches the
    // terminal as one.
    let mut refused = Vec::new();
    let mut default = toml::Table::new();
    for (key, value) in document {
        let shown = inert(&key);
        match (key.as_str(), value) {
            ("default", toml::Value::Table(table)) => default = table,
            (_, toml::Value::Table(_)) => refused.push(format!("`[{shown}]`")),
            (_, toml::Value::Array(items))
                if !items.is_empty() && items.iter().all(toml::Value::is_table) =>
            {
                refused.push(format!("`[[{shown}]]`"));
            }
            _ => refused.push(format!("`{shown}`")),
        }
    }
    for key in default.keys() {
        if !REPOSITORY_KEYS.contains(&key.as_str()) {
            refused.push(format!("`{}`", inert(key)));
        }
    }
    if !refused.is_empty() {
        return Err(format!(
            "the repository profile `{}` carries {}. A repository profile is one `[default]` table \
             setting only `provider`, `model` and `max-turns`: what a run may do (`write`, \
             `approve-up-to`, `allow-program`, the write scope, toolchains, plugins, hooks) and \
             where a provider points (`[providers]`) stay in the operator's own configuration. \
             Nothing from this file was applied.",
            shown(file),
            refused.join(", ")
        ));
    }
    // Typed key by key rather than through serde, whose message quotes the value it refused: a
    // file reached by mistake would have its text printed into a log.
    let mut profile = Profile::default();
    let mut mistyped = Vec::new();
    for (key, value) in default {
        match (key.as_str(), value) {
            // A provider is a name from a table, and a name holding a control, bidi or other
            // unprintable character names none; refused here by key, since every message that
            // names an unknown provider would otherwise quote it.
            ("provider", toml::Value::String(text)) if unprintable(&text) => {
                mistyped.push(
                    "`provider` holds a control, bidi or other unprintable character".to_owned(),
                );
            }
            ("provider", toml::Value::String(text)) => profile.provider = Some(text),
            ("model", toml::Value::String(text)) => profile.model = Some(text),
            // Zero admits no turn: the run would refuse it anyway, without naming this file.
            ("max-turns", toml::Value::Integer(turns)) if turns >= 1 => {
                profile.max_turns = u64::try_from(turns).ok();
            }
            ("max-turns", _) => {
                mistyped.push("`max-turns` must be a whole number, at least one".to_owned());
            }
            // Only `provider` and `model` are left: every other key was refused above.
            (other, _) => mistyped.push(format!("`{other}` must be a string")),
        }
    }
    if mistyped.is_empty() {
        Ok(profile)
    } else {
        Err(format!(
            "the repository profile `{}`: {}. Nothing from this file was applied.",
            shown(file),
            mistyped.join("; ")
        ))
    }
}

/// Whether `text` holds a character `std` judges unprintable: a control, bidi or other format
/// character, which a terminal acts on or does not show. Quotes and the backslash print as
/// themselves.
fn unprintable(text: &str) -> bool {
    text.chars().any(|character| {
        !matches!(character, '"' | '\'' | '\\') && character.escape_debug().len() > 1
    })
}

/// A path as a terminal may print it, through `inert`: a workspace's name and a link target inside
/// it are the repository's text, and every character a terminal acts on is shown escaped.
fn shown(path: &Path) -> String {
    inert(&path.display().to_string())
}

/// The one line a run prints when the workspace's profile was not read.
///
/// One line whatever the workspace is called: the paths are shown through `inert`, and the
/// command through `shell_word`, which spells a control character with an escape.
#[must_use]
pub fn untrusted_notice(file: &Path, workspace: &Path) -> String {
    format!(
        "warning [repository-profile] `{}` was not read: `{}` is not a trusted repository. To \
         apply it: b10x-harness profiles trust {}",
        shown(file),
        shown(workspace),
        shell_word(&workspace.display().to_string())
    )
}

/// `text` as one word a POSIX shell reads back unchanged, on one line.
///
/// Plain text as it is; otherwise single-quoted; and text holding an unprintable character (a
/// newline, ESC, a bidi override) as a dollar-single-quoted word (POSIX.1-2024, and bash, zsh and
/// ksh before it) with every byte of such a character spelled as a three-digit octal escape.
pub(crate) fn shell_word(text: &str) -> String {
    let plain =
        |character: char| character.is_ascii_alphanumeric() || "/._-+=:,@%".contains(character);
    if !text.is_empty() && text.chars().all(plain) {
        return text.to_owned();
    }
    if unprintable(text) {
        let mut word = String::from("$'");
        for character in text.chars() {
            match character {
                '\\' => word.push_str("\\\\"),
                '\'' => word.push_str("\\'"),
                other if unprintable(other.encode_utf8(&mut [0; 4])) => {
                    for byte in other.encode_utf8(&mut [0; 4]).bytes() {
                        let _ = write!(word, "\\{byte:03o}");
                    }
                }
                other => word.push(other),
            }
        }
        word.push('\'');
        return word;
    }
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Reads the config, or an empty one where there is no file.
///
/// A missing file is **not** an error: a run given every flag needs no config at all, and that is
/// how every existing invocation works. What is an error is a file that is there and cannot be
/// read — a config half-applied is worse than none.
///
/// # Errors
///
/// Names the file and what is wrong with it, including a key this build does not read.
pub fn load(path: &Path) -> Result<Config, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Config::default());
        }
        Err(error) => return Err(format!("reading `{}`: {error}", path.display())),
    };
    toml::from_str(&text).map_err(|error| {
        format!(
            "the config `{}`: {error}. A key this build does not read is refused rather than \
             skipped: a rule its author wrote that the run would not apply is worse than a run \
             that would not start.",
            path.display()
        )
    })
}

/// A profile's digest, over its own table rather than the file it came from.
///
/// So a run stays attributable to the exact bundle it used even after somebody edits an unrelated
/// profile in the same file.
#[must_use]
pub fn digest(profile: &Profile) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{profile:?}").as_bytes());
    hasher
        .finalize()
        .iter()
        .fold(String::new(), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

/// `[default]`, then each named profile in the order given, later winning a contested key.
///
/// **Replaces rather than merges**, unlike a provider override: a half-merged permission bundle —
/// one profile's allow-list beside another's ceiling — is a set of rules nobody wrote down.
///
/// # Errors
///
/// Names a profile that is not in the file, listing the ones that are.
pub fn resolve(
    config: &Config,
    repository: Option<&Repository>,
    wanted: &[String],
    source: &str,
) -> Result<Resolved, String> {
    let mut effective = config.default.clone();
    let mut used = Vec::new();
    let mut repository_provider = None;
    if config.default != Profile::default() {
        used.push(ProfileRef {
            name: "default".to_owned(),
            source: source.to_owned(),
            sha256: digest(&config.default),
        });
    }
    // Above `[default]` and beneath every `-p`: the repository knows its own work better than the
    // operator's machine-wide default, and the operator naming a profile now knows this run better
    // than either.
    if let Some(repository) = repository
        && repository.profile != Profile::default()
    {
        used.push(ProfileRef {
            name: REPOSITORY_PROFILE.to_owned(),
            source: repository.source.clone(),
            sha256: digest(&repository.profile),
        });
        // The repository decided the provider only where its value differs from the one the
        // operator's own `[default]` already chose; a `-p` that names one takes it back below.
        if let Some(provider) = &repository.profile.provider
            && config.default.provider.as_ref() != Some(provider)
        {
            repository_provider = Some(provider.clone());
        }
        overlay_plumbing(&mut effective, &repository.profile);
    }
    for name in wanted {
        let profile = config
            .profiles
            .iter()
            .find(|profile| profile.name.as_deref() == Some(name.as_str()))
            .ok_or_else(|| {
                format!(
                    "`{name}` is not a profile in `{source}`. It has: {}.",
                    config
                        .profiles
                        .iter()
                        .filter_map(|profile| profile.name.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
        used.push(ProfileRef {
            name: name.clone(),
            source: source.to_owned(),
            sha256: digest(profile),
        });
        if profile.provider.is_some() {
            repository_provider = None;
        }
        overlay(&mut effective, profile);
    }
    if effective.write != Some(true)
        && (effective
            .allow_program
            .as_ref()
            .is_some_and(|programs| !programs.is_empty())
            || effective
                .toolchains
                .as_ref()
                .is_some_and(|toolchains| !toolchains.is_empty()))
    {
        return Err(
            "this configuration declares programs to run and does not set `write = true`, \
             so no `run` tool is published and none of them can start. Set `write = true`, or drop \
             `allow-program`/`toolchains`. Said here rather than left for the model to discover by being \
             refused mid-run."
                .to_owned(),
        );
    }
    Ok(Resolved {
        profile: effective,
        used,
        repository_provider,
    })
}

/// What the profiles came to, and which ones they were.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub profile: Profile,
    pub used: Vec<ProfileRef>,
    /// The provider, when a trusted repository profile's value is what chose it: it differs from
    /// the operator's `[default]` and no `-p` named one. A credential the operator typed is
    /// refused beside it, because the operator typed it for a provider they chose.
    pub repository_provider: Option<String>,
}

/// Later wins, key by key, and only for keys the later one actually set.
fn overlay(into: &mut Profile, from: &Profile) {
    macro_rules! take {
        ($field:ident) => {
            if from.$field.is_some() {
                into.$field.clone_from(&from.$field);
            }
        };
    }
    take!(provider);
    take!(model);
    take!(write);
    take!(approve_up_to);
    take!(allow_program);
    take!(toolchains);
    take!(toolchain_specs);
    take!(write_scope);
    take!(plugin_dir);
    take!(max_turns);
}

/// [`overlay`] for a repository profile: the three admitted keys, and nothing else.
///
/// [`load_repository`] already refuses every other key. This is the second wall, so a
/// [`Repository`] built some other way still cannot reach a permission.
fn overlay_plumbing(into: &mut Profile, from: &Profile) {
    if from.provider.is_some() {
        into.provider.clone_from(&from.provider);
    }
    if from.model.is_some() {
        into.model.clone_from(&from.model);
    }
    if from.max_turns.is_some() {
        into.max_turns = from.max_turns;
    }
}

/// The write scope a run gets when the configuration names none.
///
/// **`.git/**` is denied, and that default is the guard on the whole feature.** Running in a real
/// checkout is new; a model rewriting history there is the failure it makes possible for the first
/// time, and it must not depend on a key somebody remembered to write.
///
/// **`.b10x/**` is denied for the same reason**: a trusted repository's `.b10x/harness.toml` is
/// applied by the next run, so a run that could rewrite it could choose the next run's provider,
/// model and turn budget.
#[must_use]
pub fn default_write_scope() -> Vec<String> {
    vec![
        ".git/**=denied".to_owned(),
        ".b10x/**=denied".to_owned(),
        "**=allowed".to_owned(),
    ]
}

/// A starter config, for `profiles init`.
#[must_use]
pub fn starter() -> String {
    format!(
        "# {CONFIG_FILE} — read by `b10x-harness`. Every b10x tool has one file in this directory.\n\
         #\n\
         # `[default]` applies to every run, and is the base under every `-p`.\n\
         [default]\n\
         provider = \"claude\"   # `b10x-harness providers list` for the others\n\
         write = false         # four read-only tools until this is true\n\
         \n\
         # A profile is what a run may DO. Nothing of this shape ships inside the binary.\n\
         [[profiles]]\n\
         name = \"write\"\n\
         write = true\n\
         approve-up-to = \"high\"\n\
         # Absent, the scope denies `.git/**` and `.b10x/**` and allows the rest. Restate it to widen\n\
         # or narrow.\n\
         # allow-program = [\"/usr/bin/git\"]\n\
         \n\
         # Optional: change one field of a built-in provider, keeping the rest.\n\
         # [providers.claude]\n\
         # model = \"claude-sonnet-4-5\"\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(text: &str) -> Config {
        toml::from_str(text).expect("parses")
    }

    #[test]
    fn a_later_profile_wins_a_contested_key_and_leaves_the_rest() {
        let config = config(
            "[default]\nprovider = \"claude\"\nwrite = false\n\
             [[profiles]]\nname = \"a\"\nwrite = true\napprove-up-to = \"low\"\n\
             [[profiles]]\nname = \"b\"\napprove-up-to = \"high\"\n",
        );
        let resolved =
            resolve(&config, None, &["a".to_owned(), "b".to_owned()], "<test>").expect("resolves");
        assert_eq!(resolved.profile.approve_up_to.as_deref(), Some("high"));
        assert_eq!(
            resolved.profile.write,
            Some(true),
            "`b` said nothing about writing, so `a`'s answer stands"
        );
        assert_eq!(resolved.profile.provider.as_deref(), Some("claude"));
    }

    #[test]
    fn every_profile_that_contributed_is_in_the_record_with_its_own_digest() {
        // The condition on which a profile may carry a permission at all: a run whose approval
        // ceiling came from a file must name the file.
        let config = config("[default]\nwrite = false\n[[profiles]]\nname = \"w\"\nwrite = true\n");
        let resolved =
            resolve(&config, None, &["w".to_owned()], "/x/harness.toml").expect("resolves");
        let names: Vec<&str> = resolved
            .used
            .iter()
            .map(|used| used.name.as_str())
            .collect();
        assert_eq!(names, vec!["default", "w"]);
        assert!(resolved.used.iter().all(|used| used.sha256.len() == 64));
        assert_ne!(
            resolved.used[0].sha256, resolved.used[1].sha256,
            "the digest is over the profile's own table, so two profiles in one file differ"
        );
    }

    #[test]
    fn programs_without_write_are_refused_at_startup_not_mid_run() {
        // Publication works by absence: with `write = false` there is no `run` tool, so every one
        // of these programs is unreachable and the model would spend turns finding that out.
        let config = config("[[profiles]]\nname = \"p\"\nallow-program = [\"/usr/bin/git\"]\n");
        let error = resolve(&config, None, &["p".to_owned()], "<test>").expect_err("refused");
        assert!(error.contains("write = true"), "{error}");
    }

    #[test]
    fn a_later_toolchain_set_replaces_the_earlier_one() {
        let config = config(
            "[default]\nwrite = true\ntoolchains = [\"rust\"]\n\
             toolchain-specs = [\"base.yaml\"]\n\
             [[profiles]]\nname = \"go\"\ntoolchains = [\"go\"]\n\
             toolchain-specs = [\"go.yaml\"]\n",
        );
        let resolved = resolve(&config, None, &["go".to_owned()], "<test>").expect("resolves");
        assert_eq!(resolved.profile.toolchains, Some(vec!["go".to_owned()]));
        assert_eq!(
            resolved.profile.toolchain_specs,
            Some(vec![PathBuf::from("go.yaml")])
        );
    }

    #[test]
    fn a_key_this_build_does_not_read_refuses_the_file() {
        let error = toml::from_str::<Config>("[default]\nwrite = true\nallow-everything = true\n")
            .expect_err("refused");
        assert!(format!("{error}").contains("allow-everything"), "{error}");
    }

    #[test]
    fn a_profile_that_is_not_there_names_the_ones_that_are() {
        let config = config("[[profiles]]\nname = \"write\"\nwrite = true\n");
        let error = resolve(&config, None, &["wrote".to_owned()], "<test>").expect_err("refused");
        assert!(error.contains("wrote"), "{error}");
        assert!(error.contains("write"), "{error}");
    }

    #[test]
    fn a_missing_config_is_not_an_error_because_flags_alone_have_always_worked() {
        let absent = Path::new("/nonexistent/b10x/harness.toml");
        let config = load(absent).expect("an absent config is an empty one");
        assert_eq!(config.profiles, [] as [Profile; 0]);
        assert_eq!(config.default, Profile::default());
    }

    #[test]
    fn the_default_write_scope_denies_git() {
        // Running in a real checkout is what this feature makes possible; a model rewriting
        // history there is what it makes possible for the first time.
        assert!(
            default_write_scope()
                .iter()
                .any(|rule| rule.starts_with(".git/**=denied")),
            "{:?}",
            default_write_scope()
        );
    }

    #[test]
    fn the_default_write_scope_denies_the_repository_profile_and_allows_the_rest() {
        // A trusted repository's `.b10x/harness.toml` is applied by the next run: a run able to
        // rewrite it could choose the next run's provider, model and turn budget.
        let scope = harness_catalogue::Scope::of(
            default_write_scope()
                .iter()
                .map(|rule| harness_catalogue::ScopeRule::parse(rule).expect("a rule"))
                .collect(),
        );
        for operation in ["file.write", "file.edit"] {
            for path in [".b10x/harness.toml", ".b10x/other", ".git/config"] {
                assert!(
                    scope.refusal(operation, path).is_some(),
                    "{operation} {path} is denied by default"
                );
            }
            assert_eq!(
                scope.refusal(operation, "src/main.rs"),
                None,
                "{operation} elsewhere is allowed"
            );
        }
        assert!(
            scope.refusal_of_directory("file.write", ".b10x").is_some(),
            "a write through the `.b10x` directory is refused too"
        );
    }

    #[test]
    fn the_starter_config_this_ships_is_one_this_build_can_read() {
        // `profiles init` writing something the parser refuses would be a bad first five minutes.
        let config: Config = toml::from_str(&starter()).expect("the starter parses");
        assert_eq!(config.default.provider.as_deref(), Some("claude"));
        assert_eq!(config.default.write, Some(false));
        assert!(
            config
                .profiles
                .iter()
                .any(|p| p.name.as_deref() == Some("write"))
        );
    }

    /// A workspace with a repository profile in it, and a trust record that is or is not for it.
    struct Repo {
        root: tempfile::TempDir,
    }

    impl Repo {
        fn with(text: &str) -> Self {
            let root = tempfile::tempdir().expect("a temporary directory");
            let workspace = root.path().join("work");
            std::fs::create_dir_all(workspace.join(REPOSITORY_DIR)).expect("a .b10x directory");
            std::fs::write(repository_file(&workspace), text).expect("written");
            Self { root }
        }

        fn workspace(&self) -> PathBuf {
            self.root.path().join("work")
        }

        fn file(&self) -> PathBuf {
            repository_file(&self.workspace())
        }

        fn record(&self) -> PathBuf {
            self.root
                .path()
                .join("config")
                .join(CONFIG_DIR)
                .join(crate::trust::TRUST_FILE)
        }

        fn trusted(self) -> Self {
            crate::trust::trust(&self.workspace(), &self.record()).expect("trusted");
            self
        }

        fn read(&self) -> Result<RepositoryProfile, String> {
            repository(&self.workspace(), Some(&self.record()))
        }
    }

    fn repository_of(profile: Profile) -> Repository {
        Repository {
            profile,
            source: "/work/.b10x/harness.toml".to_owned(),
        }
    }

    #[test]
    fn a_repository_profile_may_set_provider_model_and_max_turns() {
        let repo =
            Repo::with("[default]\nprovider = \"openai\"\nmodel = \"repo-model\"\nmax-turns = 3\n");
        let profile = load_repository(&repo.workspace()).expect("admitted");
        assert_eq!(
            profile,
            Profile {
                provider: Some("openai".to_owned()),
                model: Some("repo-model".to_owned()),
                max_turns: Some(3),
                ..Profile::default()
            }
        );
    }

    #[test]
    fn every_key_a_repository_profile_may_not_carry_is_refused_by_name() {
        for (key, value) in [
            ("write", "true"),
            ("approve-up-to", "\"high\""),
            ("allow-program", "[\"/usr/bin/git\"]"),
            ("toolchains", "[\"rust\"]"),
            ("toolchain-specs", "[\"specs.yaml\"]"),
            ("write-scope", "[\"**=allowed\"]"),
            ("plugin-dir", "[\"plugins\"]"),
            ("name", "\"repo\""),
            ("allow-everything", "true"),
        ] {
            let repo = Repo::with(&format!("[default]\nmodel = \"m\"\n{key} = {value}\n"));
            let error = load_repository(&repo.workspace()).expect_err(key);
            assert!(error.contains(&format!("`{key}`")), "{key}: {error}");
            assert!(
                error.contains(&repo.file().display().to_string()),
                "{key}: the refusal names the file: {error}"
            );
        }
    }

    #[test]
    fn every_table_but_default_is_refused_by_name() {
        for (text, named) in [
            (
                "[providers.claude]\nbase-url = \"https://elsewhere.example/v1\"\n",
                "[providers]",
            ),
            ("[[profiles]]\nname = \"w\"\nwrite = true\n", "[[profiles]]"),
            ("[hooks]\nbefore-call = \"x\"\n", "[hooks]"),
            ("model = \"top-level\"\n", "`model`"),
        ] {
            let repo = Repo::with(text);
            let error = load_repository(&repo.workspace()).expect_err(named);
            assert!(error.contains(named), "{named}: {error}");
            assert!(
                error.contains(&repo.file().display().to_string()),
                "{named}: {error}"
            );
        }
    }

    #[test]
    fn every_refused_key_is_named_at_once_rather_than_one_per_attempt() {
        let repo = Repo::with("[default]\nwrite = true\napprove-up-to = \"high\"\n[providers]\n");
        let error = load_repository(&repo.workspace()).expect_err("refused");
        for named in ["`write`", "`approve-up-to`", "[providers]"] {
            assert!(error.contains(named), "{named}: {error}");
        }
    }

    #[test]
    fn a_repository_profile_that_does_not_parse_names_its_file() {
        let repo = Repo::with("[default\nmodel = \n");
        let error = load_repository(&repo.workspace()).expect_err("refused");
        assert!(
            error.contains(&repo.file().display().to_string()),
            "{error}"
        );
    }

    #[test]
    fn an_untrusted_repository_profile_is_not_read_at_all() {
        // Text that would refuse the run if it were parsed: not trusting it must mean not reading it.
        let repo = Repo::with("[default]\nwrite = true\nthis is not toml\n");
        let canonical = repo.workspace().canonicalize().expect("canonical");
        assert_eq!(
            repo.read().expect("untrusted is not an error"),
            RepositoryProfile::Untrusted {
                file: repository_file(&canonical),
                workspace: canonical,
            }
        );
    }

    #[test]
    fn with_no_trust_record_location_a_repository_profile_is_untrusted() {
        let repo = Repo::with("[default]\nmodel = \"m\"\n");
        assert!(matches!(
            repository(&repo.workspace(), None).expect("not an error"),
            RepositoryProfile::Untrusted { .. }
        ));
    }

    #[test]
    fn a_trusted_repository_profile_is_read_and_names_its_file() {
        let repo = Repo::with("[default]\nmodel = \"repo-model\"\n").trusted();
        let canonical = repo.workspace().canonicalize().expect("canonical");
        let RepositoryProfile::Trusted(read) = repo.read().expect("reads") else {
            panic!("trusted");
        };
        assert_eq!(read.profile.model.as_deref(), Some("repo-model"));
        assert_eq!(
            read.source,
            repository_file(&canonical).display().to_string()
        );
    }

    #[test]
    fn a_trusted_repository_profile_with_a_refused_key_is_an_error() {
        let repo = Repo::with("[default]\nwrite = true\n").trusted();
        let error = repo.read().expect_err("refused");
        assert!(error.contains("`write`"), "{error}");
    }

    #[test]
    fn a_workspace_without_the_file_has_no_repository_profile() {
        let root = tempfile::tempdir().expect("a temporary directory");
        assert_eq!(
            repository(root.path(), None).expect("not an error"),
            RepositoryProfile::Absent
        );
        assert_eq!(
            repository(&root.path().join("missing"), None).expect("not an error"),
            RepositoryProfile::Absent,
            "a workspace that is not there has no file in it"
        );
    }

    #[test]
    fn trust_for_one_path_does_not_admit_the_file_at_another() {
        let a = Repo::with("[default]\nmodel = \"a\"\n");
        let b = Repo::with("[default]\nmodel = \"b\"\n");
        crate::trust::trust(&a.workspace(), &b.record()).expect("trusted");
        assert!(
            matches!(
                b.read().expect("reads"),
                RepositoryProfile::Untrusted { .. }
            ),
            "`b`'s record trusts `a`, so `b`'s own file is not read"
        );
        assert!(matches!(
            repository(&a.workspace(), Some(&b.record())).expect("reads"),
            RepositoryProfile::Trusted(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_workspace_is_judged_by_its_canonical_path() {
        let repo = Repo::with("[default]\nmodel = \"repo-model\"\n").trusted();
        let link = repo.root.path().join("link");
        std::os::unix::fs::symlink(repo.workspace(), &link).expect("a symlink");
        let RepositoryProfile::Trusted(read) =
            repository(&link, Some(&repo.record())).expect("reads")
        else {
            panic!("the link resolves to the trusted workspace");
        };
        let canonical = repo.workspace().canonicalize().expect("canonical");
        assert_eq!(
            read.source,
            repository_file(&canonical).display().to_string()
        );
    }

    #[test]
    fn the_notice_names_the_file_and_the_exact_trust_command() {
        let notice = untrusted_notice(
            Path::new("/srv/my repo/.b10x/harness.toml"),
            Path::new("/srv/my repo"),
        );
        assert!(!notice.contains('\n'), "one line: {notice}");
        assert!(
            notice.contains("/srv/my repo/.b10x/harness.toml"),
            "{notice}"
        );
        assert!(
            notice.contains("b10x-harness profiles trust '/srv/my repo'"),
            "a command a shell reads as one path: {notice}"
        );
    }

    #[test]
    fn the_notice_is_one_line_when_the_workspace_name_holds_a_newline_or_an_escape() {
        let notice = untrusted_notice(
            Path::new("/srv/a\nb\u{1b}[2J\u{202e}c/.b10x/harness.toml"),
            Path::new("/srv/a\nb\u{1b}[2J\u{202e}c"),
        );
        assert!(!notice.contains('\n'), "one line: {notice:?}");
        assert!(
            !notice.contains('\u{1b}') && !notice.contains('\u{202e}'),
            "{notice:?}"
        );
        assert!(
            notice.ends_with("b10x-harness profiles trust $'/srv/a\\012b\\033[2J\\342\\200\\256c'"),
            "the command spells each byte of what it cannot show: {notice}"
        );
    }

    #[test]
    fn a_shell_word_keeps_plain_and_quoted_spellings_and_escapes_only_the_unprintable() {
        assert_eq!(shell_word("/srv/repo"), "/srv/repo");
        assert_eq!(shell_word("/srv/it's"), "'/srv/it'\\''s'");
        assert_eq!(shell_word("/srv/x\ty'z\\"), "$'/srv/x\\011y\\'z\\\\'");
    }

    #[test]
    fn a_refused_key_name_is_shown_escaped_and_on_one_line() {
        let repo =
            Repo::with("[default]\n\"x\\u001B[2J\\nforged\" = 1\n[\"t\\u202Eable\"]\nk = 1\n")
                .trusted();
        let error = repo.read().expect_err("refused");
        assert!(!error.contains('\n'), "one line: {error:?}");
        assert!(
            !error.contains('\u{1b}') && !error.contains('\u{202e}'),
            "{error:?}"
        );
        assert!(error.contains("`x\\u{1b}[2J\\nforged`"), "{error}");
        assert!(error.contains("`[t\\u{202e}able]`"), "{error}");
    }

    #[test]
    fn a_provider_holding_an_unprintable_character_is_refused_by_key_and_not_shown() {
        for provider in [
            "VALUE-ONE\\r\\u001B[2K",
            "VALUE\\u202ETWO",
            "VALUE-THREE\\u0007",
        ] {
            let repo = Repo::with(&format!("[default]\nprovider = \"{provider}\"\n")).trusted();
            let error = repo.read().expect_err(provider);
            assert!(error.contains("`provider`"), "{provider}: {error}");
            assert!(
                error.contains(&canonical_file(&repo)),
                "{provider}: {error}"
            );
            assert!(
                !error.contains("VALUE"),
                "the value is never shown: {error:?}"
            );
        }
        let repo = Repo::with("[default]\nprovider = \"openai\"\n").trusted();
        assert_eq!(
            read_profile(&repo).provider.as_deref(),
            Some("openai"),
            "a printable name is read"
        );
    }

    #[test]
    fn a_repository_max_turns_of_zero_is_refused_at_read_and_one_is_read() {
        let repo = Repo::with("[default]\nmax-turns = 0\n").trusted();
        let error = repo.read().expect_err("zero admits no turn");
        assert!(error.contains("`max-turns`"), "{error}");
        assert!(error.contains("at least one"), "{error}");
        assert!(error.contains(&canonical_file(&repo)), "{error}");
        let repo = Repo::with("[default]\nmax-turns = 1\n").trusted();
        assert_eq!(read_profile(&repo).max_turns, Some(1), "one is read");
    }

    /// The profile a trusted repository's file gives.
    fn read_profile(repo: &Repo) -> Profile {
        match repo.read().expect("read") {
            RepositoryProfile::Trusted(read) => read.profile,
            other => panic!("not read as trusted: {other:?}"),
        }
    }

    #[test]
    fn the_repository_profile_sits_above_default() {
        let config = config("[default]\nmodel = \"operator\"\nmax-turns = 9\nwrite = false\n");
        let repository = repository_of(Profile {
            model: Some("repo".to_owned()),
            ..Profile::default()
        });
        let resolved = resolve(&config, Some(&repository), &[], "<test>").expect("resolves");
        assert_eq!(resolved.profile.model.as_deref(), Some("repo"));
        assert_eq!(
            resolved.profile.max_turns,
            Some(9),
            "the repository said nothing about turns, so `[default]`'s answer stands"
        );
    }

    #[test]
    fn a_named_profile_sits_above_the_repository_profile() {
        let config = config("[[profiles]]\nname = \"pick\"\nmodel = \"profile\"\n");
        let repository = repository_of(Profile {
            model: Some("repo".to_owned()),
            max_turns: Some(2),
            ..Profile::default()
        });
        let resolved =
            resolve(&config, Some(&repository), &["pick".to_owned()], "<test>").expect("resolves");
        assert_eq!(resolved.profile.model.as_deref(), Some("profile"));
        assert_eq!(resolved.profile.max_turns, Some(2));
    }

    #[test]
    fn a_repository_value_never_reaches_a_permission_key() {
        // `load_repository` refuses them; this is the second wall, for a `Repository` built some
        // other way.
        let config = config("[default]\nwrite = false\n");
        let repository = repository_of(Profile {
            write: Some(true),
            approve_up_to: Some("high".to_owned()),
            plugin_dir: Some(vec!["plugins".to_owned()]),
            model: Some("repo".to_owned()),
            ..Profile::default()
        });
        let resolved = resolve(&config, Some(&repository), &[], "<test>").expect("resolves");
        assert_eq!(resolved.profile.write, Some(false));
        assert_eq!(resolved.profile.approve_up_to, None);
        assert_eq!(resolved.profile.plugin_dir, None);
        assert_eq!(resolved.profile.model.as_deref(), Some("repo"));
    }

    #[test]
    fn the_record_carries_the_repository_between_default_and_the_named_ones() {
        let config = config(
            "[default]\nmodel = \"operator\"\n[[profiles]]\nname = \"pick\"\nmax-turns = 4\n",
        );
        let repository = repository_of(Profile {
            model: Some("repo".to_owned()),
            ..Profile::default()
        });
        let resolved = resolve(
            &config,
            Some(&repository),
            &["pick".to_owned()],
            "/x/harness.toml",
        )
        .expect("resolves");
        let names: Vec<(&str, &str)> = resolved
            .used
            .iter()
            .map(|used| (used.name.as_str(), used.source.as_str()))
            .collect();
        assert_eq!(
            names,
            vec![
                ("default", "/x/harness.toml"),
                (REPOSITORY_PROFILE, "/work/.b10x/harness.toml"),
                ("pick", "/x/harness.toml"),
            ]
        );
        assert_eq!(resolved.used[1].sha256, digest(&repository.profile));
    }

    /// The canonical path a refusal names.
    fn canonical_file(repo: &Repo) -> String {
        repository_file(&repo.workspace().canonicalize().expect("canonical"))
            .display()
            .to_string()
    }

    /// [`Repo::read`] on a thread of its own, given ten seconds: a read that blocks fails the case
    /// instead of hanging the suite.
    fn read_within_ten_seconds(repo: &Repo) -> Result<RepositoryProfile, String> {
        let (workspace, record) = (repo.workspace(), repo.record());
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(repository(&workspace, Some(&record)));
        });
        receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the read returned within ten seconds")
    }

    #[cfg(unix)]
    #[test]
    fn a_trusted_repository_profile_that_is_a_fifo_is_refused_by_name_and_never_opened() {
        let repo = Repo::with("").trusted();
        std::fs::remove_file(repo.file()).expect("removed");
        let made = std::process::Command::new("mkfifo")
            .arg(repo.file())
            .status()
            .expect("mkfifo runs");
        assert!(made.success(), "a FIFO at the repository profile's place");
        let error = read_within_ten_seconds(&repo).expect_err("refused");
        assert!(error.contains(&canonical_file(&repo)), "{error}");
        assert!(error.contains("not a regular file"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn a_trusted_repository_profile_that_is_a_symlink_is_refused_even_when_it_stays_inside() {
        let repo = Repo::with("").trusted();
        let inside = repo.workspace().join("profile.toml");
        std::fs::write(&inside, "[default]\nmodel = \"linked-model\"\n").expect("written");
        std::fs::remove_file(repo.file()).expect("removed");
        std::os::unix::fs::symlink(&inside, repo.file()).expect("a symlink");
        let error = repo.read().expect_err("refused");
        assert!(error.contains(&canonical_file(&repo)), "{error}");
        assert!(error.contains("symbolic link"), "{error}");
        assert!(
            !error.contains("linked-model"),
            "nothing of it is quoted: {error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_trusted_repository_profile_reached_through_a_symlinked_b10x_directory_is_refused() {
        for leads_inside in [false, true] {
            let repo = Repo::with("").trusted();
            let elsewhere = if leads_inside {
                repo.workspace().join("config")
            } else {
                repo.root.path().join("outside")
            };
            std::fs::create_dir_all(&elsewhere).expect("a directory");
            std::fs::write(
                elsewhere.join(CONFIG_FILE),
                "[default]\nmodel = \"elsewhere-model\"\n",
            )
            .expect("written");
            std::fs::remove_dir_all(repo.workspace().join(REPOSITORY_DIR)).expect("removed");
            std::os::unix::fs::symlink(&elsewhere, repo.workspace().join(REPOSITORY_DIR))
                .expect("a symlinked .b10x");
            let error = repo.read().expect_err("refused");
            assert!(error.contains(&canonical_file(&repo)), "{error}");
            assert!(
                !error.contains("elsewhere-model"),
                "nothing of it is quoted: {error}"
            );
        }
    }

    #[test]
    fn a_trusted_repository_profile_over_64_kib_is_refused_by_name() {
        let limit = usize::try_from(REPOSITORY_LIMIT).expect("fits");
        let head = "[default]\nmodel = \"m\"\n";
        let exactly = format!("{head}{}\n", "#".repeat(limit - head.len() - 1));
        assert_eq!(exactly.len(), limit);
        let repo = Repo::with(&exactly).trusted();
        assert!(
            matches!(repo.read(), Ok(RepositoryProfile::Trusted(_))),
            "64 KiB exactly is read"
        );

        let repo = Repo::with(&format!("{exactly}#")).trusted();
        let error = repo.read().expect_err("refused");
        assert!(error.contains(&canonical_file(&repo)), "{error}");
        assert!(error.contains("64 KiB"), "{error}");
    }

    #[test]
    fn a_repository_profile_that_does_not_parse_is_named_by_line_and_column_and_never_quoted() {
        let repo =
            Repo::with("[default]\nmodel = \"m\"\ncolour = unquoted-SECRET-0001\n").trusted();
        let error = repo.read().expect_err("refused");
        assert!(error.contains(&canonical_file(&repo)), "{error}");
        assert!(error.contains("line 3, column"), "{error}");
        assert!(!error.contains("SECRET"), "no text of the file: {error}");
        assert!(!error.contains('\n'), "no snippet beneath it: {error}");
    }

    #[test]
    fn a_wrongly_typed_value_is_named_by_its_key_and_never_quoted() {
        for (text, key) in [
            ("[default]\nmax-turns = \"SECRET-0002\"\n", "`max-turns`"),
            ("[default]\nmax-turns = -1\n", "`max-turns`"),
            ("[default]\nmodel = 7\n", "`model`"),
            ("[default]\nprovider = [\"SECRET-0003\"]\n", "`provider`"),
        ] {
            let repo = Repo::with(text).trusted();
            let error = repo.read().expect_err(key);
            assert!(error.contains(key), "{key}: {error}");
            assert!(error.contains(&canonical_file(&repo)), "{key}: {error}");
            assert!(!error.contains("SECRET"), "{key}: {error}");
        }
    }

    #[test]
    fn a_repository_profile_that_is_not_utf8_is_refused_by_name() {
        let repo = Repo::with("").trusted();
        std::fs::write(repo.file(), b"[default]\nmodel = \"\xff\xfe\"\n").expect("written");
        let error = repo.read().expect_err("refused");
        assert!(error.contains(&canonical_file(&repo)), "{error}");
        assert!(error.contains("UTF-8"), "{error}");
    }

    #[test]
    fn the_provider_is_the_repositorys_only_where_its_value_decided_it() {
        let repository = repository_of(Profile {
            provider: Some("openai".to_owned()),
            ..Profile::default()
        });
        for (operator, wanted, expected) in [
            ("", vec![], Some("openai")),
            ("[default]\nprovider = \"claude\"\n", vec![], Some("openai")),
            ("[default]\nprovider = \"openai\"\n", vec![], None),
            (
                "[[profiles]]\nname = \"pin\"\nprovider = \"claude\"\n",
                vec!["pin".to_owned()],
                None,
            ),
            (
                "[[profiles]]\nname = \"pin\"\nprovider = \"openai\"\n",
                vec!["pin".to_owned()],
                None,
            ),
            (
                "[[profiles]]\nname = \"turns\"\nmax-turns = 3\n",
                vec!["turns".to_owned()],
                Some("openai"),
            ),
        ] {
            let resolved =
                resolve(&config(operator), Some(&repository), &wanted, "<test>").expect("resolves");
            assert_eq!(
                resolved.repository_provider.as_deref(),
                expected,
                "{operator} {wanted:?}"
            );
        }
        let model_only = repository_of(Profile {
            model: Some("m".to_owned()),
            ..Profile::default()
        });
        let resolved = resolve(&config(""), Some(&model_only), &[], "<test>").expect("resolves");
        assert_eq!(resolved.repository_provider, None);
    }
}
