//! The argv surface, pinned like any other contract.
//!
//! # What went wrong without one
//!
//! `--substrate-embedded` changed from taking a value to being bare. It was the right change — it
//! had demanded a value it then ignored — but a consumer pinned to `0.1.0` went on passing
//! `--substrate-embedded 1`, and clap refused the whole command line before any harness code ran.
//! Nothing in this repository could have caught it: the wire contracts pin what goes to a provider
//! and the profile contract pins what a bridge client sees, and **the command line is a third
//! interface with consumers of its own** — metaharness's `b10x` adapter launches this binary and
//! reads its record.
//!
//! So the argv surface is pinned the same way, from both directions (`AGENTS.md` invariant 14): a
//! Python checker verifies the manifest against the file, and
//! [`the_pinned_argv_contract_is_what_this_binary_defines`] verifies that clap's own definition
//! still produces exactly those bytes. Changing a flag's shape is a **new version directory**
//! (invariant 13), never an edit of a released one.
//!
//! # Generated from clap, never written by hand
//!
//! Every field here is read off `Cli::command()`. A hand-maintained document would be a second
//! description of the command line that drifts from the first, which is the failure this exists to
//! remove rather than to reproduce.

use clap::CommandFactory as _;
use serde_json::{Map, Value, json};

use crate::Cli;

/// The version directory this build's argv surface is pinned in.
///
/// A dated directory and not a semantic version: what a consumer pins is *the shape on that day*,
/// and a change cuts a new one beside it.
pub const ARGV_CONTRACT_VERSION: &str = "2026-08-30.1";

/// This binary's argv surface as canonical JSON: sorted keys, two-space indent, one trailing
/// newline.
///
/// Canonical because it is compared **byte for byte** against a pinned file. Sorting is
/// `serde_json`'s own map ordering plus an explicit sort of every array, so two builds of the same
/// definition produce the same bytes whatever order clap happens to hold its arguments in.
///
/// # Panics
///
/// Only if this document — strings, booleans and nulls — stops being encodable as JSON, which
/// `Cli::command().debug_assert()` already rules out for every field it reads.
#[must_use]
pub fn argv() -> String {
    let command = Cli::command();
    let mut subcommands: Vec<(String, &clap::Command)> = Vec::new();
    reachable(&command, "", &mut subcommands);
    subcommands.sort_by(|left, right| left.0.cmp(&right.0));

    let mut arguments = Map::new();
    arguments.insert(command.get_name().to_owned(), flags(&command));
    for (path, subcommand) in &subcommands {
        arguments.insert(path.clone(), flags(subcommand));
    }

    let document = json!({
        "product": command.get_name(),
        "subcommands": subcommands
            .iter()
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>(),
        "arguments": Value::Object(arguments),
    });
    let mut text = serde_json::to_string_pretty(&document)
        .expect("a document of strings and booleans encodes");
    text.push('\n');
    text
}

/// Every command a caller can type, named by the words that reach it.
///
/// **Depth-first and space-joined** — `workflow`, `workflow plan`, `workflow run` — because a
/// nested verb is a command line a consumer types, and a document that recorded only the top level
/// would say `workflow` accepts no flags at all: true of the word, false of every verb under it,
/// and the second is what would break a driver. The word itself is recorded too, with the empty
/// flag list it really has, so `subcommands` still names everything that exists.
fn reachable<'a>(
    command: &'a clap::Command,
    prefix: &str,
    into: &mut Vec<(String, &'a clap::Command)>,
) {
    for subcommand in command.get_subcommands() {
        let path = if prefix.is_empty() {
            subcommand.get_name().to_owned()
        } else {
            format!("{prefix} {}", subcommand.get_name())
        };
        reachable(subcommand, &path, into);
        into.push((path, subcommand));
    }
}

/// One command's long flags, in name order.
///
/// Positional arguments are not recorded because this command line has none: every value is named,
/// which is what makes an invocation readable in a driver's source three months later. One added
/// later would need its own field here rather than being folded into this one.
fn flags(command: &clap::Command) -> Value {
    let mut rows: Vec<Value> = command
        .get_arguments()
        .filter(|argument| !argument.is_positional())
        .filter_map(|argument| {
            let long = argument.get_long()?;
            Some(json!({
                "long": format!("--{long}"),
                // The one that broke a consumer: whether the flag eats the next word.
                "takes_value": argument.get_action().takes_values(),
                "value_name": argument
                    .get_value_names()
                    .and_then(|names| names.first())
                    .map(std::string::ToString::to_string),
                "default": argument
                    .get_default_values()
                    .first()
                    .map(|value| value.to_string_lossy().into_owned()),
                "required": argument.is_required_set(),
                "conflicts_with": conflicts(command, argument),
                "requires": requires(command, argument),
            }))
        })
        .collect();
    rows.sort_by(|left, right| left["long"].as_str().cmp(&right["long"].as_str()));
    Value::Array(rows)
}

/// Every flag this one may not appear beside, in name order.
///
/// **Both directions.** clap stores a conflict on the argument that declared it and enforces it
/// symmetrically, so a document that recorded only the declaration would say `--approve-up-to`
/// conflicts with `--yes` and that `--yes` conflicts with nothing — true of the definition, false
/// of the behaviour, and the behaviour is what a consumer is pinning.
fn conflicts(command: &clap::Command, argument: &clap::Arg) -> Vec<String> {
    let long_of = |candidate: &clap::Arg| candidate.get_long().map(|long| format!("--{long}"));
    let mut names: Vec<String> = command
        .get_arg_conflicts_with(argument)
        .into_iter()
        .filter_map(long_of)
        .collect();
    names.extend(
        command
            .get_arguments()
            .filter(|other| other.get_id() != argument.get_id())
            .filter(|other| {
                command
                    .get_arg_conflicts_with(other)
                    .iter()
                    .any(|conflicting| conflicting.get_id() == argument.get_id())
            })
            .filter_map(long_of),
    );
    names.sort();
    names.dedup();
    names
}

/// Every flag that must appear alongside this one, in name order.
///
/// `conflicts_with` alone was half the story. `--delegate-turns` is refused without `--delegate`
/// and `--oauth-token-pointer` without an oauth source, and a consumer pinned to the document could
/// not see either: it would read a flag with no conflicts and no default, pass it on its own, and
/// be refused by clap before any harness code ran — which is exactly how `--substrate-embedded`
/// broke a driver.
///
/// # Read from the parser, not from the declaration
///
/// clap exposes no getter for an argument's requirements, and the declaration is not what a
/// consumer is pinning anyway — the *behaviour* is, which is the same argument [`conflicts`] makes.
/// So this asks the parser: the command line is parsed twice, once with the flag and once without,
/// and whatever clap newly reports as a missing requirement is what the flag brought with it.
///
/// A requirement on a **group of alternatives** — `--oauth-token-pointer` needs an oauth source —
/// records the group's members. They also conflict with one another, and the two fields read
/// together say what is true: one of them, not both.
fn requires(command: &clap::Command, argument: &clap::Arg) -> Vec<String> {
    let Some(long) = argument.get_long() else {
        return Vec::new();
    };
    let mut words = vec![format!("--{long}")];
    if argument.get_action().takes_values() {
        // Any value clap will accept: a rejected one ends the parse before requirements are
        // checked, and this would then record silence as "requires nothing".
        words.push(
            argument
                .get_possible_values()
                .first()
                .map(|value| value.get_name().to_owned())
                .or_else(|| {
                    argument
                        .get_default_values()
                        .first()
                        .map(|value| value.to_string_lossy().into_owned())
                })
                .unwrap_or_else(|| "1".to_owned()),
        );
    }
    let already = missing(command, &[]);
    let mut names: Vec<String> = missing(command, &words)
        .into_iter()
        .filter(|name| !already.contains(name))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The long flags clap says are missing when this command line is parsed.
///
/// Empty for anything that is not a missing requirement — a value clap refused, a help request, a
/// parse that succeeded — because none of those says a flag needs another one.
fn missing(command: &clap::Command, words: &[String]) -> std::collections::BTreeSet<String> {
    let mut argv = vec![command.get_name().to_owned()];
    argv.extend(words.iter().cloned());
    let Err(error) = command.clone().try_get_matches_from(argv) else {
        return std::collections::BTreeSet::new();
    };
    if error.kind() != clap::error::ErrorKind::MissingRequiredArgument {
        return std::collections::BTreeSet::new();
    }
    let Some(clap::error::ContextValue::Strings(required)) =
        error.get(clap::error::ContextKind::InvalidArg)
    else {
        return std::collections::BTreeSet::new();
    };
    // Each entry is a usage fragment — `--model <MODEL>`, or `<--a|--b>` for a group of
    // alternatives — so the long flags are read out of it and the placeholders are left behind.
    let known: std::collections::BTreeSet<&str> = command
        .get_arguments()
        .filter_map(clap::Arg::get_long)
        .collect();
    required
        .iter()
        .flat_map(|fragment| longs_in(fragment))
        .filter(|long| known.contains(long.as_str()))
        .map(|long| format!("--{long}"))
        .collect()
}

/// Every `--long-flag` named in one usage fragment, without its leading dashes.
fn longs_in(fragment: &str) -> Vec<String> {
    fragment
        .split("--")
        .skip(1)
        .map(|rest| {
            rest.chars()
                .take_while(|character| character.is_ascii_alphanumeric() || *character == '-')
                .collect::<String>()
        })
        .filter(|long| !long.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the pinned document lives, from this crate's own directory.
    fn pinned() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("contracts")
            .join("cli")
            .join("b10x-harness")
            .join(ARGV_CONTRACT_VERSION)
            .join("argv.json")
    }

    /// The Rust half of the contract: the code produces exactly the pinned bytes.
    ///
    /// The Python half — `scripts/check-cli-contract.py` — verifies the manifest against the file.
    /// Neither is sufficient alone: a checker alone pins a document nothing produces, and this
    /// alone pins a document nothing else can verify was not edited alongside the code.
    #[test]
    fn the_pinned_argv_contract_is_what_this_binary_defines() {
        let path = pinned();
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("reading `{}`: {error}", path.display()));
        let actual = argv();
        if actual == expected {
            return;
        }
        let mut differences = Vec::new();
        for (number, (left, right)) in expected.lines().zip(actual.lines()).enumerate() {
            if left != right {
                differences.push(format!(
                    "  line {}:\n    pinned: {left}\n    built:  {right}",
                    number + 1
                ));
            }
        }
        if expected.lines().count() != actual.lines().count() {
            differences.push(format!(
                "  the pinned document has {} lines and this build produces {}",
                expected.lines().count(),
                actual.lines().count()
            ));
        }
        panic!(
            "the command line no longer matches `contracts/cli/b10x-harness/{ARGV_CONTRACT_VERSION}/argv.json`.\n\
             A released contract version is immutable (`AGENTS.md` invariant 13): **cut a new contract version** \
             — copy the directory to today's date, regenerate `argv.json` and its manifest, and enter the change \
             in `CHANGELOG.md`. Do not edit the pinned file.\n{}",
            differences.join("\n")
        );
    }

    /// Every released version is still pinned beside the current one.
    ///
    /// A released contract version is immutable (`AGENTS.md` invariant 13), and *immutable* is a
    /// claim about the directory as much as about its bytes: `scripts/check-cli-contract.py` walks
    /// whatever directories exist, so an older one that was deleted rather than kept would take its
    /// verification with it and the checker would go on printing a pass. `2026-08-29.1` is on
    /// `main` and consumers pin it; it stays, unchanged, beside `.2`.
    #[test]
    fn every_released_argv_version_is_still_pinned_beside_the_current_one() {
        let versions = pinned()
            .parent()
            .expect("the version directory")
            .parent()
            .expect("the product directory")
            .to_path_buf();
        let mut present: Vec<String> = std::fs::read_dir(&versions)
            .unwrap_or_else(|error| panic!("reading `{}`: {error}", versions.display()))
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        present.sort();
        assert_eq!(
            present,
            vec![
                "2026-08-29",
                "2026-08-29.1",
                "2026-08-29.2",
                "2026-08-29.3",
                "2026-08-30",
                "2026-08-30.1"
            ],
            "a released version may be superseded and never removed"
        );
        assert_eq!(
            present.last().map(String::as_str),
            Some(ARGV_CONTRACT_VERSION),
            "this build pins the newest one"
        );
        for version in &present {
            let manifest = versions.join(version).join("manifest.json");
            let held: Value = serde_json::from_str(
                &std::fs::read_to_string(&manifest)
                    .unwrap_or_else(|error| panic!("reading `{}`: {error}", manifest.display())),
            )
            .expect("a manifest");
            assert_eq!(
                held["version"], *version,
                "`{version}` names itself: {manifest:?}"
            );
        }
    }

    /// Where the version directories live.
    fn versions_directory() -> std::path::PathBuf {
        pinned()
            .parent()
            .expect("the version directory")
            .parent()
            .expect("the product directory")
            .to_path_buf()
    }

    /// Every pinned version, oldest first.
    ///
    /// Lexicographic order is chronological here because the scheme is a date and a `.N` suffix
    /// (`AGENTS.md` invariant 13): `2026-08-29` sorts before `2026-08-29.1`, which sorts before
    /// `2026-08-30`.
    fn versions_in_order() -> Vec<String> {
        let directory = versions_directory();
        let mut present: Vec<String> = std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("reading `{}`: {error}", directory.display()))
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        present.sort();
        present
    }

    /// One pinned version's argv document.
    fn argv_of(version: &str) -> Value {
        let path = versions_directory().join(version).join("argv.json");
        serde_json::from_str(
            &std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("reading `{}`: {error}", path.display())),
        )
        .unwrap_or_else(|error| panic!("`{}` is not JSON: {error}", path.display()))
    }

    /// One pinned version's prose.
    fn readme_of(version: &str) -> String {
        let path = versions_directory().join(version).join("README.md");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("reading `{}`: {error}", path.display()))
    }

    /// Every flag of a pinned document, keyed by the command it is typed after and its long name.
    fn flags_of(document: &Value) -> std::collections::BTreeMap<(String, String), Value> {
        let mut rows = std::collections::BTreeMap::new();
        for (subcommand, listed) in document["arguments"].as_object().expect("an object") {
            for row in listed.as_array().expect("a list of arguments") {
                let long = row["long"].as_str().expect("a long flag").to_owned();
                rows.insert((subcommand.clone(), long), row.clone());
            }
        }
        rows
    }

    /// The version in force accounts for every field that moved, measured against what preceded it.
    ///
    /// A *what changed* section is the part of a contract a consumer reads before deciding to
    /// change nothing, so it is the part that has to be true. `2026-08-30` measured itself against
    /// `2026-08-29.1` while `.2` and `.3` stood between them, and concluded "strictly additive"
    /// for a diff in which `--model` and `--base-url` stopped being required and `--wire` lost its
    /// default on three commands. Nobody reading that document could have found out.
    ///
    /// So the claim is read out of the bytes rather than out of the prose. Every field that moved
    /// between two consecutive pinned versions must be named — the command, the flag, the field,
    /// the value before and the value after, all on one line and each in backticks — either by the
    /// README of the version it moved in, or by the README of the version in force, which must
    /// then also name the two versions the move sits between.
    ///
    /// The second alternative exists because a released version is immutable (`AGENTS.md`
    /// invariant 13): a wrong one cannot be corrected in place, so the correction has to live where
    /// a consumer of the current pin will actually find it. That means every later cut carries the
    /// correction forward, which is the cost of having published the wrong thing once.
    #[test]
    fn the_version_in_force_names_every_field_that_moved_between_pinned_versions() {
        const PINNED_FIELDS: [&str; 6] = [
            "conflicts_with",
            "default",
            "required",
            "requires",
            "takes_value",
            "value_name",
        ];
        let present = versions_in_order();
        let (current, earlier) = present.split_last().expect("a version in force");
        assert_eq!(
            current.as_str(),
            ARGV_CONTRACT_VERSION,
            "this build pins the newest one"
        );
        let previous = earlier.last().expect("a version before the one in force");
        let in_force = readme_of(current);
        assert!(
            in_force.contains(&format!("## What changed since {previous}")),
            "`{current}/README.md` must measure itself against `{previous}`, the version \
             immediately before it, and not against one further back"
        );

        let mut unnamed: Vec<String> = Vec::new();
        for pair in present.windows(2) {
            let (older, newer) = (&pair[0], &pair[1]);
            let was = flags_of(&argv_of(older));
            let now = flags_of(&argv_of(newer));
            let successor = readme_of(newer);
            let between = in_force.contains(&format!("`{older}`"))
                && in_force.contains(&format!("`{newer}`"));
            for ((subcommand, long), before) in &was {
                let Some(after) = now.get(&(subcommand.clone(), long.clone())) else {
                    continue;
                };
                for field in PINNED_FIELDS {
                    if before[field] == after[field] {
                        continue;
                    }
                    let tokens = [
                        format!("`{subcommand}`"),
                        format!("`{long}`"),
                        format!("`{field}`"),
                        format!("`{}`", before[field]),
                        format!("`{}`", after[field]),
                    ];
                    let names = |text: &str| {
                        text.lines()
                            .any(|line| tokens.iter().all(|token| line.contains(token.as_str())))
                    };
                    if names(&successor) || (between && names(&in_force)) {
                        continue;
                    }
                    unnamed.push(format!(
                        "  `{older}` -> `{newer}`: `{subcommand}` `{long}` `{field}` {} -> {}",
                        before[field], after[field]
                    ));
                }
            }
        }
        assert!(
            unnamed.is_empty(),
            "a field moved and no document a consumer of `{ARGV_CONTRACT_VERSION}` reads says so.\n\
             Name it in the README of the version it moved in, or — when that version is released \
             and therefore immutable (`AGENTS.md` invariant 13) — in \
             `{ARGV_CONTRACT_VERSION}/README.md`, on one line carrying the command, the flag, the \
             field, the value before and the value after, each in backticks:\n{}",
            unnamed.join("\n")
        );
    }

    #[test]
    fn the_document_is_canonical_so_two_builds_produce_the_same_bytes() {
        let text = argv();
        assert!(text.ends_with("}\n"), "one trailing newline: {text:?}");
        assert!(text.contains("\n  \"arguments\""), "two-space indent");
        let value: Value = serde_json::from_str(&text).expect("valid JSON");
        let subcommands: Vec<&str> = value["subcommands"]
            .as_array()
            .expect("a list")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let mut sorted = subcommands.clone();
        sorted.sort_unstable();
        assert_eq!(subcommands, sorted, "the subcommand list is ordered");
    }

    #[test]
    fn a_flag_that_eats_the_next_word_is_distinguished_from_one_that_does_not() {
        // The exact confusion that broke a consumer pinned to `0.1.0`.
        let value: Value = serde_json::from_str(&argv()).expect("valid JSON");
        let flag = |subcommand: &str, long: &str| -> Value {
            value["arguments"][subcommand]
                .as_array()
                .expect("a list")
                .iter()
                .find(|entry| entry["long"] == long)
                .unwrap_or_else(|| panic!("{subcommand} has {long}"))
                .clone()
        };
        assert_eq!(flag("run", "--substrate-embedded")["takes_value"], false);
        assert_eq!(flag("run", "--base-url")["takes_value"], true);
        assert_eq!(flag("run", "--surface")["default"], "flat");
        assert_eq!(flag("run", "--approve")["default"], "auto");
        assert_eq!(flag("run", "--input")["required"], true);
        assert_eq!(
            flag("run", "--yes")["conflicts_with"],
            json!(["--approve-up-to"]),
            "recorded on both flags, because clap enforces it on both"
        );
    }

    #[test]
    fn a_nested_verb_is_pinned_by_the_words_that_reach_it() {
        // `workflow` on its own accepts nothing and runs nothing; what a consumer types is
        // `workflow run`, and that is the command line this document has to describe.
        let value: Value = serde_json::from_str(&argv()).expect("valid JSON");
        let subcommands: Vec<&str> = value["subcommands"]
            .as_array()
            .expect("a list")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        for name in ["workflow", "workflow plan", "workflow run"] {
            assert!(subcommands.contains(&name), "{subcommands:?}");
        }
        let flag = |subcommand: &str, long: &str| -> Option<Value> {
            value["arguments"][subcommand]
                .as_array()
                .expect("a list")
                .iter()
                .find(|entry| entry["long"] == long)
                .cloned()
        };
        assert_eq!(
            flag("workflow run", "--flow").expect("the document is named")["required"],
            true
        );
        assert_eq!(
            flag("workflow run", "--input").expect("the task is named")["takes_value"],
            true
        );
        // `plan` contacts nothing, and the pinned document is where a consumer reads that.
        assert!(flag("workflow plan", "--base-url").is_none());
        assert!(flag("workflow plan", "--flow").is_some());
        assert!(
            value["arguments"]["workflow"]
                .as_array()
                .expect("a list")
                .is_empty(),
            "the word itself takes no flags"
        );
    }

    #[test]
    fn a_flag_that_needs_another_one_says_so_and_one_that_needs_none_says_that_too() {
        // The other half of what clap refuses before any harness code runs. A consumer that read
        // only `conflicts_with` would pass `--delegate-turns 1` on its own and be refused.
        let value: Value = serde_json::from_str(&argv()).expect("valid JSON");
        let flag = |subcommand: &str, long: &str| -> Value {
            value["arguments"][subcommand]
                .as_array()
                .expect("a list")
                .iter()
                .find(|entry| entry["long"] == long)
                .unwrap_or_else(|| panic!("{subcommand} has {long}"))
                .clone()
        };
        assert_eq!(
            flag("run", "--delegate-turns")["requires"],
            json!(["--delegate"])
        );
        // A group of alternatives records its members. They conflict with one another, so the two
        // fields read together say one of them, not both.
        assert_eq!(
            flag("run", "--oauth-token-pointer")["requires"],
            json!(["--oauth-token-env", "--oauth-token-file"])
        );
        assert_eq!(
            flag("run", "--oauth-token-env")["conflicts_with"],
            json!(["--api-key-env", "--api-key-file", "--oauth-token-file"]),
            "and the members exclude one another, so the pair reads as one of them, not both"
        );
        // Emitted for every flag, empty or not, exactly as `conflicts_with` is: a key that
        // appeared only sometimes would read as a document that forgot.
        assert_eq!(flag("run", "--json")["requires"], json!([]));
        assert_eq!(
            flag("chat", "--delegate-turns")["requires"],
            json!(["--delegate"])
        );
    }
}
