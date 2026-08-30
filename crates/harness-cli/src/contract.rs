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
pub const ARGV_CONTRACT_VERSION: &str = "2026-08-30.2";

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
/// `Cli::command().debug_assert()` already rules out for every field it reads, or if a command
/// this program defines is missing from the same command line after clap has built it.
#[must_use]
pub fn argv() -> String {
    // Two views of one command line. The paths come from the tree **this program defines**; the
    // arguments are read off the **built** tree, which is the only place clap's own `--help` and
    // `--version` exist. The build also grows a `help` subcommand and a copy of every path under
    // it — `help run`, `profiles help explain`, `help help` — and those are not recorded: `help
    // <command>` prints the help text, and the help text is what this contract explicitly does not
    // pin. The flags are a different question, because `b10x-harness -V` is what a driver reads to
    // know which binary it drove.
    let declared = Cli::command();
    let built = definition();
    let mut paths: Vec<String> = Vec::new();
    reachable(&declared, "", &mut paths);
    paths.sort();

    let mut arguments = Map::new();
    arguments.insert(built.get_name().to_owned(), flags(&built));
    for path in &paths {
        let command = at_path(&built, path)
            .unwrap_or_else(|| panic!("`{path}` is defined but is not in the built tree"));
        arguments.insert(path.clone(), flags(command));
    }

    let document = json!({
        "product": built.get_name(),
        "subcommands": paths.clone(),
        "arguments": Value::Object(arguments),
    });
    let mut text = serde_json::to_string_pretty(&document)
        .expect("a document of strings and booleans encodes");
    text.push('\n');
    text
}

/// This binary's clap definition, **built**.
///
/// Built, because clap does not insert its own arguments until it is. `-h, --help` on every
/// command and `-V, --version` on the root are added during the build, and `b10x-harness -V`
/// prints the version and exits `0` — so a document read off the unbuilt definition describes a
/// command line the shell does not serve, and silently omits two of the three short flags a
/// consumer can type. `Command::build` is the getter clap documents for exactly this: *"call this
/// on the top-level `Command` when done building and before reading state"*.
fn definition() -> clap::Command {
    let mut command = Cli::command();
    command.build();
    command
}

/// Every command a caller can type, named by the words that reach it.
///
/// **Depth-first and space-joined** — `workflow`, `workflow plan`, `workflow run` — because a
/// nested verb is a command line a consumer types, and a document that recorded only the top level
/// would say `workflow` accepts no flags at all: true of the word, false of every verb under it,
/// and the second is what would break a driver. The word itself is recorded too, with the empty
/// flag list it really has, so `subcommands` still names everything that exists.
///
/// Read from the **unbuilt** definition, so what it names is what this program declares. clap
/// generates a `help` subcommand carrying a copy of every path beneath it, and enumerating those
/// would put `help help help` in the list a driver reads.
fn reachable(command: &clap::Command, prefix: &str, into: &mut Vec<String>) {
    for subcommand in command.get_subcommands() {
        let path = if prefix.is_empty() {
            subcommand.get_name().to_owned()
        } else {
            format!("{prefix} {}", subcommand.get_name())
        };
        reachable(subcommand, &path, into);
        into.push(path);
    }
}

/// The command reached by typing these words, or nothing where no such command exists.
fn at_path<'a>(root: &'a clap::Command, path: &str) -> Option<&'a clap::Command> {
    let mut command = root;
    for word in path.split_whitespace() {
        command = command
            .get_subcommands()
            .find(|candidate| candidate.get_name() == word)?;
    }
    Some(command)
}

/// One command's long flags, in name order.
///
/// Positional arguments are not recorded because this command line has none: every value is named,
/// which is what makes an invocation readable in a driver's source three months later. One added
/// later would need its own field here rather than being folded into this one.
///
/// A row is keyed by its long flag and carries the short spelling beside it, because `-p` is a
/// command line a consumer can type today and a document that recorded only `--profile` could lose
/// it without either half of the check noticing.
fn flags(command: &clap::Command) -> Value {
    let mut rows: Vec<Value> = command
        .get_arguments()
        .filter(|argument| !argument.is_positional())
        .filter_map(|argument| {
            let long = argument.get_long()?;
            // The one that broke a consumer: whether the flag eats the next word. It decides the
            // placeholder and the default too — clap holds a value name for every argument
            // including the bare ones, and gives a bare flag the built-in default `"false"` when
            // the command is built. Neither is a word a consumer may type after this flag, and a
            // document that recorded either would be describing an argument clap refuses.
            let takes_value = argument.get_action().takes_values();
            Some(json!({
                "long": format!("--{long}"),
                // The other spelling of the same flag, or null where there is only one.
                "short": argument.get_short().map(|short| format!("-{short}")),
                "takes_value": takes_value,
                "value_name": takes_value
                    .then(|| {
                        argument
                            .get_value_names()
                            .and_then(|names| names.first())
                            .map(std::string::ToString::to_string)
                    })
                    .flatten(),
                "default": takes_value
                    .then(|| {
                        argument
                            .get_default_values()
                            .first()
                            .map(|value| value.to_string_lossy().into_owned())
                    })
                    .flatten(),
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
        let versions = versions_directory();
        // Ordered by the day and the cut within it, not as strings: `2026-08-29.10` is the
        // eleventh cut of that day and belongs after `.9`, where a plain sort puts it after `.1`
        // and makes `last()` name a version that is not the one in force.
        let present = in_cut_order(&versions_present());
        assert_eq!(
            present,
            vec![
                "2026-08-29",
                "2026-08-29.1",
                "2026-08-29.2",
                "2026-08-29.3",
                "2026-08-30",
                "2026-08-30.1",
                "2026-08-30.2"
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

    /// Every pinned version, in whatever order the filesystem hands them over.
    fn versions_present() -> Vec<String> {
        let directory = versions_directory();
        std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("reading `{}`: {error}", directory.display()))
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect()
    }

    /// Where a version sits in time: the day it was cut, and which cut of that day it was.
    ///
    /// Split rather than compared as a string. `Vec::sort` puts `2026-08-30.10` between `.1` and
    /// `.2`, and invariant 13's scheme has no ceiling on `.N` — this repository already cut four
    /// versions of this contract in one day. At the tenth a lexicographic order names the wrong
    /// version in force, diffs one pair backwards, and never diffs the pair that really is
    /// consecutive, all without saying anything.
    fn cut_order(version: &str) -> (&str, u32) {
        version
            .rsplit_once('.')
            .and_then(|(day, nth)| nth.parse::<u32>().ok().map(|nth| (day, nth)))
            .unwrap_or((version, 0))
    }

    /// Pinned versions oldest first — chronologically, which is not lexicographically.
    fn in_cut_order(versions: &[String]) -> Vec<String> {
        let mut ordered = versions.to_vec();
        ordered.sort_by(|left, right| cut_order(left).cmp(&cut_order(right)));
        ordered
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

    /// Every command a document records flags for: the root and every nested verb.
    fn described_by(document: &Value) -> std::collections::BTreeSet<String> {
        document["arguments"]
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect()
    }

    /// Every command a document's `subcommands` list says exists.
    ///
    /// Held apart from [`described_by`] rather than unioned with it, because the two lists answer
    /// different questions and a name can leave one without leaving the other. Dropping `tools`
    /// from `subcommands` while its flag rows stay behind makes the document say the command does
    /// not exist — `subcommands` is what a driver enumerates — and a union would go on seeing it.
    fn declared_by(document: &Value) -> std::collections::BTreeSet<String> {
        document["subcommands"]
            .as_array()
            .expect("a list")
            .iter()
            .map(|name| name.as_str().expect("a name").to_owned())
            .collect()
    }

    /// One thing about the command line that is not what it was, in the words a README must use.
    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct Moved {
        subcommand: String,
        long: String,
        field: String,
        before: String,
        after: String,
    }

    impl Moved {
        /// The five cells a README row has to carry, in the order it has to carry them.
        fn cells(&self) -> [String; 5] {
            [
                format!("`{}`", self.subcommand),
                format!("`{}`", self.long),
                format!("`{}`", self.field),
                format!("`{}`", self.before),
                format!("`{}`", self.after),
            ]
        }
    }

    impl std::fmt::Display for Moved {
        /// The row itself, so a failure prints the line that answers it.
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let cells = self.cells();
            write!(formatter, "| {} |", cells.join(" | "))
        }
    }

    /// The name a vanished flag or command is recorded under, and the values that say it vanished.
    const GONE: (&str, &str, &str) = ("present", "true", "false");

    /// Everything that moved between two argv documents, oldest first.
    ///
    /// The key set is a **union** and absence is a value. Comparing only what both documents hold
    /// makes a renamed flag, a removed flag and a dropped command invisible, and those are exactly
    /// the changes a pinned consumer cannot survive: `--substrate-embedded` changed shape and clap
    /// refused the whole command line before any harness code ran (`AGENTS.md:84-86`).
    ///
    /// Arrivals are not returned. A flag or a command that did not exist before cannot break an
    /// invocation that already worked — that is what "strictly additive" means, and why the phrase
    /// is worth checking rather than banning. A departure always can. Recording an arrival is the
    /// *what is new* prose's job; demanding a row for each would put seventy of them in this
    /// chain's documents and bury the nine that matter.
    ///
    /// A command that is gone takes its flags with it and is reported once, not once per flag.
    fn moves_between(before: &Value, after: &Value) -> Vec<Moved> {
        // `short` is one of them: `-p` is a command line a consumer can type, and a flag that
        // loses its short spelling breaks an invocation that already worked exactly as a renamed
        // long flag does. A document cut before the key existed holds no `short` on either side of
        // a pair, so absence never reads as a move.
        const PINNED_FIELDS: [&str; 7] = [
            "conflicts_with",
            "default",
            "required",
            "requires",
            "short",
            "takes_value",
            "value_name",
        ];
        let (was_described, now_described) = (described_by(before), described_by(after));
        let undescribed: std::collections::BTreeSet<&String> =
            was_described.difference(&now_described).collect();
        let (was_declared, now_declared) = (declared_by(before), declared_by(after));
        let mut departed: std::collections::BTreeSet<&String> = undescribed.clone();
        departed.extend(was_declared.difference(&now_declared));
        let mut moved: Vec<Moved> = departed
            .iter()
            .map(|command| Moved {
                subcommand: (*command).clone(),
                long: "the command itself".to_owned(),
                field: GONE.0.to_owned(),
                before: GONE.1.to_owned(),
                after: GONE.2.to_owned(),
            })
            .collect();

        let (was, now) = (flags_of(before), flags_of(after));
        for (key, row) in &was {
            let (subcommand, long) = key;
            if undescribed.contains(subcommand) {
                continue;
            }
            let Some(now_row) = now.get(key) else {
                moved.push(Moved {
                    subcommand: subcommand.clone(),
                    long: long.clone(),
                    field: GONE.0.to_owned(),
                    before: GONE.1.to_owned(),
                    after: GONE.2.to_owned(),
                });
                continue;
            };
            for field in PINNED_FIELDS {
                if row[field] != now_row[field] {
                    moved.push(Moved {
                        subcommand: subcommand.clone(),
                        long: long.clone(),
                        field: field.to_owned(),
                        before: row[field].to_string(),
                        after: now_row[field].to_string(),
                    });
                }
            }
        }
        moved.sort();
        moved
    }

    /// Every span between a pair of backticks, in order.
    ///
    /// A version is matched as a whole backticked token and never as a substring: `2026-08-30` is
    /// a prefix of `2026-08-30.1`, so a heading naming the first would otherwise read as naming
    /// the second, and `2026-08-30.99` would pass as either.
    fn backticked(text: &str) -> impl Iterator<Item = &str> {
        text.split('`').skip(1).step_by(2)
    }

    /// The body under every `##` heading that names all of `must_name`, and nothing else.
    ///
    /// Scoped rather than searched whole, because evidence has to be about the pair it is filed
    /// under. `2026-08-30`'s defect was attributing a diff to versions it was not between, and a
    /// table that absolves a move it does not claim to be about repeats that defect exactly.
    fn section_naming(readme: &str, must_name: &[&str]) -> String {
        let mut inside = false;
        let mut body: Vec<&str> = Vec::new();
        for line in readme.lines() {
            if let Some(heading) = line.strip_prefix("## ") {
                inside = must_name
                    .iter()
                    .all(|name| backticked(heading).any(|token| token == *name));
                continue;
            }
            if inside {
                body.push(line);
            }
        }
        body.join("\n")
    }

    /// The trimmed cells of one markdown table row, or nothing where the line is not a row.
    fn row_cells(line: &str) -> Option<Vec<&str>> {
        let trimmed = line.trim();
        if trimmed.len() < 2 || !trimmed.starts_with('|') || !trimmed.ends_with('|') {
            return None;
        }
        Some(
            trimmed
                .trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect(),
        )
    }

    /// Whether one line is a table row carrying exactly this move's five cells, side by side.
    ///
    /// Cells rather than substrings, and equality rather than containment, because three separate
    /// lies passed a bag-of-substrings test on one line:
    ///
    /// - the nine rows written **backwards** — `false` then `true`, so the flags became *more*
    ///   required rather than less, the opposite of what happened;
    /// - a sentence **denying** the move in the very words that describe it: *"Strictly additive
    ///   after all. No change to `run` `--model` `required`: it is `true` and was never
    ///   `false`."*;
    /// - **one** junk line carrying every token at once, which absolved all nine moves together.
    ///
    /// A cell in the right column kills the first, being a table cell at all kills the second, and
    /// [`unstated`]'s one-line-per-move kills the third.
    fn row_states(line: &str, moved: &Moved) -> bool {
        let Some(cells) = row_cells(line) else {
            return false;
        };
        let wanted = moved.cells();
        cells.windows(wanted.len()).any(|window| {
            window
                .iter()
                .zip(wanted.iter())
                .all(|(cell, want)| *cell == want.as_str())
        })
    }

    /// The moves this section does not state, each having had to claim a line of its own.
    ///
    /// A line is consumed by the first move it answers. Without that, one row wide enough to hold
    /// every token stands in for every move at once, and the document says nine things by saying
    /// one.
    fn unstated(section: &str, moves: &[Moved]) -> Vec<Moved> {
        let lines: Vec<&str> = section.lines().collect();
        let mut claimed: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
        let mut missing = Vec::new();
        for moved in moves {
            let answer = lines
                .iter()
                .enumerate()
                .find(|(number, line)| !claimed.contains(number) && row_states(line, moved));
            match answer {
                Some((number, _)) => {
                    claimed.insert(number);
                }
                None => missing.push(moved.clone()),
            }
        }
        missing
    }

    /// The version in force accounts for everything that moved, measured against what preceded it.
    ///
    /// A *what changed* section is the part of a contract a consumer reads before deciding to
    /// change nothing, so it is the part that has to be true. `2026-08-30` measured itself against
    /// `2026-08-29.1` while `.2` and `.3` stood between them, and concluded "strictly additive"
    /// for a diff in which `--model` and `--base-url` stopped being required and `--wire` lost its
    /// default on three commands. Nobody reading that document could have found out.
    ///
    /// So the claim is read out of the bytes. Every move between two consecutive pinned versions
    /// must be stated as a **table row of its own** — the command, the flag, the field, the value
    /// before and the value after, five cells side by side in that order — inside a `##` section
    /// whose heading names the versions it is about. Either the README of the version it moved in,
    /// under a heading naming the older one, or the README of the version in force, under a
    /// heading naming **both**.
    ///
    /// The second alternative exists because a released version is immutable (`AGENTS.md`
    /// invariant 13): a wrong one cannot be corrected in place, so the correction has to live
    /// where a consumer of the current pin will actually find it. That means every later cut
    /// carries the correction forward, which is the cost of having published the wrong thing once.
    ///
    /// Everything this rests on — the order, the diff, the section, the row — takes its inputs as
    /// values and is exercised on documents written for the purpose in the tests below. A guard
    /// that can only ever run against the six directories that happen to exist is the same class
    /// of thing as the document nobody checked.
    #[test]
    fn the_version_in_force_names_every_field_that_moved_between_pinned_versions() {
        let present = in_cut_order(&versions_present());
        let (current, earlier) = present.split_last().expect("a version in force");
        assert_eq!(
            current.as_str(),
            ARGV_CONTRACT_VERSION,
            "this build pins the newest one"
        );
        let previous = earlier.last().expect("a version before the one in force");
        let in_force = readme_of(current);

        let headings: Vec<&str> = in_force
            .lines()
            .filter_map(|line| line.strip_prefix("## What changed since "))
            .collect();
        assert_eq!(
            headings.len(),
            1,
            "`{current}/README.md` says what changed exactly once: {headings:?}"
        );
        assert_eq!(
            backticked(headings[0]).next(),
            Some(previous.as_str()),
            "`{current}/README.md` must measure itself against `{previous}`, the version \
             immediately before it — backticked, so a heading naming a longer version that starts \
             with the same characters is not read as naming this one"
        );

        let mut unnamed: Vec<String> = Vec::new();
        for pair in present.windows(2) {
            let (older, newer) = (&pair[0], &pair[1]);
            let moves = moves_between(&argv_of(older), &argv_of(newer));
            if moves.is_empty() {
                continue;
            }
            let stated_where_it_moved = section_naming(&readme_of(newer), &[older.as_str()]);
            let missing = unstated(&stated_where_it_moved, &moves);
            let carried = if newer == current {
                String::new()
            } else {
                section_naming(&in_force, &[older.as_str(), newer.as_str()])
            };
            for moved in unstated(&carried, &missing) {
                unnamed.push(format!("  `{older}` -> `{newer}`: {moved}"));
            }
        }
        assert!(
            unnamed.is_empty(),
            "something moved and no document a consumer of `{ARGV_CONTRACT_VERSION}` reads states \
             it. Put each row below in the README of the version it moved in, under a `##` heading \
             naming the older version — or, when that version is released and therefore immutable \
             (`AGENTS.md` invariant 13), in `{ARGV_CONTRACT_VERSION}/README.md` under a `##` \
             heading naming both. One row per move, cells exactly as printed:\n{}",
            unnamed.join("\n")
        );
    }

    /// A synthetic argv document, so the diff can be exercised on something other than the six
    /// directories that happen to exist.
    fn document(rows: &[(&str, &str, Value)]) -> Value {
        let mut arguments = Map::new();
        for (subcommand, long, row) in rows {
            let mut row = row.clone();
            row["long"] = json!(long);
            arguments
                .entry((*subcommand).to_owned())
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .expect("a list")
                .push(row);
        }
        let mut subcommands: Vec<String> = arguments.keys().cloned().collect();
        subcommands.sort();
        json!({ "product": "b10x-harness", "subcommands": subcommands, "arguments": arguments })
    }

    /// A flag row with every pinned field at its quietest value.
    fn flag() -> Value {
        json!({
            "long": "--placeholder",
            "short": Value::Null,
            "takes_value": true,
            "value_name": "VALUE",
            "default": Value::Null,
            "required": false,
            "conflicts_with": [],
            "requires": [],
        })
    }

    /// A section that would answer these moves honestly, one row each.
    fn honest(moves: &[Moved]) -> String {
        let rows: Vec<String> = moves.iter().map(ToString::to_string).collect();
        format!(
            "| command | flag | field | before | after |\n| --- | --- | --- | --- | --- |\n{}",
            rows.join("\n")
        )
    }

    /// The order the pair diff rests on stops being chronological at the tenth cut of one day.
    ///
    /// Invariant 13's scheme is `2026-08-29`, then `.1`, then `.2`, and nothing in it stops at
    /// `.9` — this repository already cut four versions of this contract in one day. At the tenth,
    /// lexicographic order puts `.10` between `.1` and `.2`, and three things follow at once:
    /// `split_last` names `.9` as the version in force and the run fails against a correct cut,
    /// `windows(2)` pairs a newer document with an older one and diffs it **backwards**, and the
    /// pair that really is consecutive is never diffed at all.
    ///
    /// [`in_cut_order`] is called rather than reproduced, and it takes the sequence as an
    /// argument, so the rule the guard actually uses is the rule under test here.
    #[test]
    fn the_version_order_is_still_chronological_at_the_tenth_cut_of_one_day() {
        let cut_in_this_order = [
            "2026-08-30",
            "2026-08-30.1",
            "2026-08-30.2",
            "2026-08-30.9",
            "2026-08-30.10",
        ];
        let mut arrived: Vec<String> = cut_in_this_order
            .iter()
            .map(|version| (*version).to_owned())
            .collect();
        // `read_dir` promises no order, and a plain sort puts `.10` between `.1` and `.2`.
        arrived.sort();
        assert_eq!(
            in_cut_order(&arrived),
            cut_in_this_order,
            "the tenth cut of a day is the newest one, not the second"
        );
    }

    /// One move, in the words the nine real ones are written in.
    fn one_move() -> Moved {
        Moved {
            subcommand: "run".to_owned(),
            long: "--model".to_owned(),
            field: "required".to_owned(),
            before: "true".to_owned(),
            after: "false".to_owned(),
        }
    }

    #[test]
    fn a_move_written_the_wrong_way_round_does_not_state_it() {
        let moves = vec![one_move()];
        let backwards = "| `run` | `--model` | `required` | `false` | `true` |";
        assert_eq!(
            unstated(backwards, &moves),
            moves,
            "`false` -> `true` is the opposite claim: the flag became more required, not less"
        );
        assert!(unstated(&honest(&moves), &moves).is_empty());
    }

    #[test]
    fn a_sentence_denying_a_move_in_its_own_words_does_not_state_it() {
        let moves = vec![one_move()];
        let denial = "Strictly additive after all. No change to `run` `--model` `required`: it \
                      is `true` and was never `false`.";
        assert_eq!(
            unstated(denial, &moves),
            moves,
            "a denial carries every word the statement carries, in the same order"
        );
    }

    #[test]
    fn one_line_carrying_every_token_states_at_most_one_move() {
        let moves: Vec<Moved> = ["chat", "run", "workflow run"]
            .iter()
            .map(|subcommand| Moved {
                subcommand: (*subcommand).to_owned(),
                long: "--model".to_owned(),
                field: "required".to_owned(),
                before: "true".to_owned(),
                after: "false".to_owned(),
            })
            .collect();
        let junk = "| `chat` | `--model` | `required` | `true` | `false` | `run` | `--model` | \
                    `required` | `true` | `false` | `workflow run` | `--model` | `required` | \
                    `true` | `false` |";
        assert_eq!(
            unstated(junk, &moves).len(),
            2,
            "one line answers one move; the document has to say the other two out loud"
        );
        assert!(unstated(&honest(&moves), &moves).is_empty());
    }

    #[test]
    fn evidence_filed_under_the_wrong_pair_absolves_nothing() {
        let moves = vec![one_move()];
        let table = honest(&moves);
        let wrong_pair =
            format!("## What `2026-08-29.1` got wrong, and `2026-08-29.2` with it\n\n{table}\n");
        let right_pair =
            format!("## What `2026-08-29.3` got wrong, and `2026-08-30` with it\n\n{table}\n");
        let about = ["2026-08-29.3", "2026-08-30"];
        assert_eq!(
            unstated(&section_naming(&wrong_pair, &about), &moves),
            moves,
            "a table headed for one pair states nothing about another"
        );
        assert!(
            unstated(&section_naming(&right_pair, &about), &moves).is_empty(),
            "the same table, filed under the pair it is about, states it"
        );
    }

    #[test]
    fn a_renamed_flag_is_a_move_even_though_no_field_of_it_changed() {
        let before = document(&[("run", "--substrate-embedded", flag())]);
        let after = document(&[("run", "--substrate", flag())]);
        assert_eq!(
            moves_between(&before, &after),
            vec![Moved {
                subcommand: "run".to_owned(),
                long: "--substrate-embedded".to_owned(),
                field: "present".to_owned(),
                before: "true".to_owned(),
                after: "false".to_owned(),
            }],
            "the flag a consumer types is gone, and no field of any surviving flag says so"
        );
    }

    #[test]
    fn a_dropped_command_is_one_move_and_does_not_drag_its_flags_in_with_it() {
        let before = document(&[
            ("run", "--input", flag()),
            ("workflow run", "--flow", flag()),
            ("workflow run", "--input", flag()),
        ]);
        let after = document(&[("run", "--input", flag())]);
        assert_eq!(
            moves_between(&before, &after),
            vec![Moved {
                subcommand: "workflow run".to_owned(),
                long: "the command itself".to_owned(),
                field: "present".to_owned(),
                before: "true".to_owned(),
                after: "false".to_owned(),
            }],
            "a command that is gone is said once, not once for each flag it took with it"
        );
    }

    #[test]
    fn a_command_struck_from_the_subcommand_list_is_gone_even_with_its_flags_left_behind() {
        let before = document(&[("run", "--input", flag()), ("tools", "--driver", flag())]);
        let mut after = before.clone();
        after["subcommands"] = json!(["run"]);
        assert_eq!(
            moves_between(&before, &after),
            vec![Moved {
                subcommand: "tools".to_owned(),
                long: "the command itself".to_owned(),
                field: "present".to_owned(),
                before: "true".to_owned(),
                after: "false".to_owned(),
            }],
            "`subcommands` is the list a driver enumerates: a name struck from it is a command \
             the document says does not exist, whatever rows are left under `arguments`"
        );
    }

    #[test]
    fn an_arriving_flag_is_not_a_move_and_a_changed_one_is() {
        let before = document(&[("run", "--model", flag())]);
        let mut arrived = flag();
        arrived["required"] = json!(true);
        let after = document(&[("run", "--model", flag()), ("run", "--profile", arrived)]);
        assert!(
            moves_between(&before, &after).is_empty(),
            "an arrival cannot break an invocation that already worked"
        );
        let mut changed = flag();
        changed["takes_value"] = json!(false);
        let reshaped = document(&[("run", "--model", changed)]);
        assert_eq!(
            moves_between(&before, &reshaped),
            vec![Moved {
                subcommand: "run".to_owned(),
                long: "--model".to_owned(),
                field: "takes_value".to_owned(),
                before: "true".to_owned(),
                after: "false".to_owned(),
            }]
        );
    }

    /// A flag that loses its short spelling is a move, like a flag that loses its name.
    ///
    /// `-p` is a command line a consumer types, so `run -p fast` breaking is the same event as
    /// `run --profile fast` breaking, and the *what changed* section has to carry a row for it.
    /// Without this, `short` could be struck from `PINNED_FIELDS` and every test in this file
    /// still passed — measured, and the reason this case exists.
    #[test]
    fn a_flag_that_loses_its_short_spelling_is_a_move() {
        let mut spelled = flag();
        spelled["short"] = json!("-m");
        let before = document(&[("run", "--model", spelled.clone())]);
        let mut unspelled = spelled.clone();
        unspelled["short"] = Value::Null;
        assert_eq!(
            moves_between(&before, &document(&[("run", "--model", unspelled)])),
            vec![Moved {
                subcommand: "run".to_owned(),
                long: "--model".to_owned(),
                field: "short".to_owned(),
                before: "\"-m\"".to_owned(),
                after: "null".to_owned(),
            }],
            "`run -m gpt` stops parsing and no other field of the row says so"
        );
        let mut repointed = spelled.clone();
        repointed["short"] = json!("-M");
        assert_eq!(
            moves_between(&before, &document(&[("run", "--model", repointed)])).len(),
            1,
            "a short flag repointed at another letter is a move too"
        );
        assert!(
            moves_between(&before, &document(&[("run", "--model", spelled)])).is_empty(),
            "and an unchanged one is not"
        );
    }

    /// A flag that eats no word records no placeholder for one.
    ///
    /// Every pinned README says `value_name` holds "the placeholder in the usage line", and clap
    /// prints no placeholder for a bare flag: `b10x-harness run --help` renders
    /// `--substrate-embedded` and `--delegate` with nothing after them, beside a
    /// `-p, --profile <NAME>` that has one. Up to `2026-08-30.1` the document recorded
    /// `"value_name": "SUBSTRATE_EMBEDDED"` against `"takes_value": false` on 23 rows — including
    /// the one flag this whole contract exists because of. A driver that generated a command line
    /// from `value_name` emitted `--substrate-embedded SUBSTRATE_EMBEDDED`, which is the exact word
    /// clap refused from the consumer pinned to `0.1.0`.
    ///
    /// `a_flag_that_eats_the_next_word_is_distinguished_from_one_that_does_not` asserts
    /// `takes_value` on that flag and never looks at the placeholder beside it.
    // Red from the day it was written until `2026-08-30.2`, on purpose, and carrying `#[ignore]`
    // with the story that would close it named in the attribute: making it green re-pins
    // `argv.json`, and a released version is immutable, so it took a new cut
    // (`story:argv-pin-misdescribes-the-command-line`).
    #[test]
    fn a_flag_that_eats_no_word_records_no_placeholder_for_one() {
        let document: Value = serde_json::from_str(&argv()).expect("valid JSON");
        let mut contradictory: Vec<String> = Vec::new();
        for ((subcommand, long), row) in flags_of(&document) {
            if row["takes_value"] == false && !row["value_name"].is_null() {
                contradictory.push(format!(
                    "  `{subcommand}` `{long}`: takes_value `false`, value_name {}",
                    row["value_name"]
                ));
            }
        }
        assert!(
            contradictory.is_empty(),
            "the pinned document names a placeholder for a flag that eats no word, and its own \
             README says a placeholder is what appears in the usage line:\n{}",
            contradictory.join("\n")
        );
    }

    /// Every short flag a consumer can type is pinned, or the document says it is not pinned.
    ///
    /// `-p` is a short flag on `--profile`, on every command that takes a run's options and on
    /// `profiles explain` (`crates/harness-cli/src/lib.rs:340` and `:480`), and clap prints it:
    /// `-p, --profile <NAME>`. Up to `2026-08-30.1` the pinned document had no field for it —
    /// every row was keyed by `long` alone — and that README named it in neither direction: "What
    /// is pinned" said "one row per long flag" and "What is not pinned" listed the help text, the
    /// summaries, clap's print order and the exit statuses. The word "short" appeared nowhere in
    /// the file.
    ///
    /// So `-p` was a piece of the argv surface that could be renamed, repointed at another flag or
    /// dropped with both halves of the check green — the failure this contract was cut to prevent,
    /// on a flag a consumer can already type today. `2026-08-30.2` pins it: every row carries
    /// `short`, and `short` is a field [`moves_between`] diffs, so losing it is a move the README
    /// of the cut that loses it has to state.
    // Red from the day it was written until `2026-08-30.2`, on purpose, and carrying `#[ignore]`
    // with the story that would close it named in the attribute. The `readme.contains("short")`
    // hatch below would also have gone green on a README sentence disclaiming short flags — that
    // is moving the goalpost rather than closing the gap, and it is not the route that was taken:
    // `-p` is in `argv.json`, where the byte-for-byte pin above holds it.
    #[test]
    fn a_short_flag_a_consumer_can_type_is_pinned_or_named_as_unpinned() {
        let declared = Cli::command();
        let built = definition();
        let mut paths: Vec<String> = Vec::new();
        reachable(&declared, "", &mut paths);
        let document: Value = serde_json::from_str(&argv()).expect("valid JSON");
        let rows = flags_of(&document);
        let disclaimed = what_is_not_pinned(&readme_of(ARGV_CONTRACT_VERSION));

        let mut unpinned: Vec<String> = Vec::new();
        let commands = std::iter::once((built.get_name().to_owned(), &built)).chain(
            paths.iter().map(|path| {
                let reached =
                    at_path(&built, path).expect("a declared command is in the built tree");
                (path.clone(), reached)
            }),
        );
        for (path, subcommand) in commands {
            for argument in subcommand.get_arguments() {
                let (Some(short), Some(long)) = (argument.get_short(), argument.get_long()) else {
                    continue;
                };
                let (short, long) = (format!("-{short}"), format!("--{long}"));
                let recorded = rows
                    .get(&(path.clone(), long.clone()))
                    .is_some_and(|row| row["short"] == short);
                // The escape, scoped to the one section that may take it. Named as a backticked
                // token, so a sentence has to say which flag it is giving up on.
                let named_as_unpinned = disclaimed.contains(&format!("`{short}`"))
                    || disclaimed.contains(&format!("`{long}`"));
                if !recorded && !named_as_unpinned {
                    unpinned.push(format!("  `{path}`: `{short}` on `{long}`"));
                }
            }
        }
        assert!(
            unpinned.is_empty(),
            "a short flag clap accepts is in neither the pinned document nor its README's `What \
             is not pinned` section, so it can move without either half of the check noticing:\n{}",
            unpinned.join("\n")
        );
    }

    /// The body under `## What is not pinned`, and nothing else.
    ///
    /// Scoped to that one heading, because a whole-file search is not a disclaimer: `2026-08-30.2`
    /// documents a field **named** `short`, so `readme.contains("short")` — what this guard asked
    /// before — is open for good the moment the gap it guards is closed. Measured: with it, every
    /// `"short"` in `argv.json` could be `null` and the guard would still report nothing. The
    /// story's acceptance statement forbids satisfying this guard by disclaiming, so the escape is
    /// narrowed to a sentence in the section that exists to name what a consumer may not rely on.
    fn what_is_not_pinned(readme: &str) -> String {
        let mut inside = false;
        let mut body: Vec<&str> = Vec::new();
        for line in readme.lines() {
            if let Some(heading) = line.strip_prefix("## ") {
                inside = heading.trim() == "What is not pinned";
                continue;
            }
            if inside {
                body.push(line);
            }
        }
        body.join("\n")
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
        // The word itself takes no flag of its own — only the `--help` clap gives every command,
        // which is a row here because `-h` is a short flag a consumer can type. An empty row set
        // would now be the document forgetting one, not the word having none.
        let word: Vec<&str> = value["arguments"]["workflow"]
            .as_array()
            .expect("a list")
            .iter()
            .map(|row| row["long"].as_str().expect("a long flag"))
            .collect();
        assert_eq!(word, vec!["--help"], "the word itself takes no flags");
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
