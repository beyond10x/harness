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
pub const ARGV_CONTRACT_VERSION: &str = "2026-08-29";

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
    let mut subcommands: Vec<&clap::Command> = command.get_subcommands().collect();
    subcommands.sort_by_key(|subcommand| subcommand.get_name());

    let mut arguments = Map::new();
    arguments.insert(command.get_name().to_owned(), flags(&command));
    for subcommand in &subcommands {
        arguments.insert((*subcommand).get_name().to_owned(), flags(subcommand));
    }

    let document = json!({
        "product": command.get_name(),
        "subcommands": subcommands
            .iter()
            .map(|subcommand| (*subcommand).get_name())
            .collect::<Vec<_>>(),
        "arguments": Value::Object(arguments),
    });
    let mut text = serde_json::to_string_pretty(&document)
        .expect("a document of strings and booleans encodes");
    text.push('\n');
    text
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
