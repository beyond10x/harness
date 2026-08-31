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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
            use std::fmt::Write as _;
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
pub fn resolve(config: &Config, wanted: &[String], source: &str) -> Result<Resolved, String> {
    let mut effective = config.default.clone();
    let mut used = Vec::new();
    if config.default != Profile::default() {
        used.push(ProfileRef {
            name: "default".to_owned(),
            source: source.to_owned(),
            sha256: digest(&config.default),
        });
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
    })
}

/// What the profiles came to, and which ones they were.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub profile: Profile,
    pub used: Vec<ProfileRef>,
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

/// The write scope a run gets when the configuration names none.
///
/// **`.git/**` is denied, and that default is the guard on the whole feature.** Running in a real
/// checkout is new; a model rewriting history there is the failure it makes possible for the first
/// time, and it must not depend on a key somebody remembered to write.
#[must_use]
pub fn default_write_scope() -> Vec<String> {
    vec![".git/**=denied".to_owned(), "**=allowed".to_owned()]
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
         # Absent, the scope denies `.git/**` and allows the rest. Restate it to widen or narrow.\n\
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
            resolve(&config, &["a".to_owned(), "b".to_owned()], "<test>").expect("resolves");
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
        let resolved = resolve(&config, &["w".to_owned()], "/x/harness.toml").expect("resolves");
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
        let error = resolve(&config, &["p".to_owned()], "<test>").expect_err("refused");
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
        let resolved = resolve(&config, &["go".to_owned()], "<test>").expect("resolves");
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
        let error = resolve(&config, &["wrote".to_owned()], "<test>").expect_err("refused");
        assert!(error.contains("wrote"), "{error}");
        assert!(error.contains("write"), "{error}");
    }

    #[test]
    fn a_missing_config_is_not_an_error_because_flags_alone_have_always_worked() {
        let absent = Path::new("/nonexistent/b10x/harness.toml");
        let config = load(absent).expect("an absent config is an empty one");
        assert!(config.profiles.is_empty());
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
}
