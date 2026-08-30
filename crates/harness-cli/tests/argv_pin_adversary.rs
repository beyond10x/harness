//! Adversarial cases against the argv pin cut by
//! `story:argv-pin-misdescribes-the-command-line`.
//!
//! Its acceptance statement is *"every short flag clap accepts is **pinned** in the document — not
//! disclaimed in its* What is not pinned *section, which would satisfy the guard without closing
//! the gap"*. These cases assert that statement against the **binary**, not against the unbuilt
//! `Cli::command()` the in-crate guard reads: a short flag is something a consumer types at a
//! shell, so what the shell accepts is what the document has to describe.
//!
//! They live here rather than in `crates/harness-cli/src/contract.rs` because the adversary that
//! wrote them may not touch an implementation file.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use b10x_harness_cli::contract::ARGV_CONTRACT_VERSION;
use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

/// The repository root, from this crate's own directory.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("the repository root")
}

/// The version directory in force.
fn in_force() -> PathBuf {
    root()
        .join("contracts")
        .join("cli")
        .join("b10x-harness")
        .join(ARGV_CONTRACT_VERSION)
}

/// The pinned argv document of the version in force.
fn pinned_document() -> Value {
    let path = in_force().join("argv.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading `{}`: {error}", path.display()));
    serde_json::from_str(&text).expect("the pinned document is JSON")
}

/// The prose of the version in force.
fn pinned_readme() -> String {
    let path = in_force().join("README.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading `{}`: {error}", path.display()))
}

/// Every command a caller can type, as the pinned document lists them, the root first.
fn commands(document: &Value) -> Vec<String> {
    let mut paths = vec![String::new()];
    paths.extend(
        document["subcommands"]
            .as_array()
            .expect("a list of subcommands")
            .iter()
            .map(|name| name.as_str().expect("a name").to_owned()),
    );
    paths
}

/// What `--help` prints for one command path, read off the binary a consumer runs.
fn help_of(path: &str) -> String {
    let mut command = Command::new(BINARY);
    for word in path.split_whitespace() {
        command.arg(word);
    }
    let output = command
        .arg("--help")
        .output()
        .unwrap_or_else(|error| panic!("running `{BINARY} {path} --help`: {error}"));
    assert!(
        output.status.success(),
        "`{BINARY} {path} --help` exited {:?}",
        output.status.code()
    );
    String::from_utf8(output.stdout).expect("help is UTF-8")
}

/// The `-x, --long` pairs one help page prints.
///
/// clap renders every short spelling this way in the options block, in both its short and its long
/// help layout, so the pairs are read out of what a consumer is actually shown.
fn short_flags_in(help: &str) -> BTreeSet<(String, String)> {
    let mut found = BTreeSet::new();
    for line in help.lines() {
        let trimmed = line.trim_start();
        if line.len() == trimmed.len() || !trimmed.starts_with('-') {
            continue;
        }
        let mut characters = trimmed.chars();
        let (Some('-'), Some(letter)) = (characters.next(), characters.next()) else {
            continue;
        };
        if !letter.is_ascii_alphanumeric() {
            continue;
        }
        let Some(rest) = trimmed[2..].strip_prefix(", --") else {
            continue;
        };
        let long: String = rest
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '-')
            .collect();
        if !long.is_empty() {
            found.insert((format!("-{letter}"), format!("--{long}")));
        }
    }
    found
}

/// Every `(command path, short, long)` this binary accepts.
fn short_flags_the_binary_accepts() -> BTreeSet<(String, String, String)> {
    let document = pinned_document();
    let mut accepted = BTreeSet::new();
    for path in commands(&document) {
        for (short, long) in short_flags_in(&help_of(&path)) {
            let named = if path.is_empty() {
                "b10x-harness".to_owned()
            } else {
                path.clone()
            };
            accepted.insert((named, short, long));
        }
    }
    accepted
}

/// The body of the `## What is not pinned` section, where the acceptance statement allows a flag
/// to be disclaimed rather than pinned.
fn what_is_not_pinned(readme: &str) -> String {
    let mut inside = false;
    let mut body = Vec::new();
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

/// The acceptance statement, asserted against the binary a consumer runs.
///
/// `-h`/`--help` on the root and on every one of the seventeen subcommands, and `-V`/`--version` on
/// the root, are short flags clap accepts today: `b10x-harness -V` prints the version and exits 0.
/// They appear in no row of `argv.json` — `flags()` reads `Cli::command()` before clap has built
/// it, and clap inserts its own help and version arguments during that build — and the
/// `## What is not pinned` section names neither flag, so neither branch of the acceptance
/// statement is taken for them.
///
/// The version in force says the opposite in its own prose: *"every short flag clap accepts is
/// recorded here"* (`contracts/cli/b10x-harness/2026-08-30.2/README.md:199`). That sentence is new
/// in this cut — the word "short" appears nowhere in `2026-08-30.1/README.md` — so a consumer is
/// now told something about this command line that is not true.
///
/// The fix is one of the two the acceptance offers, and it is not applied here: either record
/// clap's help and version arguments in `flags()` (`Cli::command()` has to be built first, e.g.
/// with `clap::Command::build`, which is a new cut), or name `--help`/`-h` and `--version`/`-V` in
/// the `## What is not pinned` section of the version in force and stop claiming the stronger
/// thing at `README.md:199`.
#[test]
fn every_short_flag_the_binary_accepts_is_pinned_or_disclaimed() {
    let document = pinned_document();
    let readme = pinned_readme();
    let disclaimed = what_is_not_pinned(&readme);

    let mut rows: BTreeSet<(String, String, String)> = BTreeSet::new();
    for (path, listed) in document["arguments"].as_object().expect("an object") {
        for row in listed.as_array().expect("a list of arguments") {
            if let Some(short) = row["short"].as_str() {
                rows.insert((
                    path.clone(),
                    short.to_owned(),
                    row["long"].as_str().expect("a long flag").to_owned(),
                ));
            }
        }
    }

    let mut unaccounted: Vec<String> = Vec::new();
    for (path, short, long) in short_flags_the_binary_accepts() {
        if rows.contains(&(path.clone(), short.clone(), long.clone())) {
            continue;
        }
        // The escape the acceptance statement allows, scoped to the one section it names — not the
        // whole file, which would go green on the word "short" the field's own documentation uses.
        if disclaimed.contains(&format!("`{long}`")) || disclaimed.contains(&format!("`{short}`")) {
            continue;
        }
        unaccounted.push(format!("  `{path}`: `{short}` on `{long}`"));
    }

    assert!(
        unaccounted.is_empty(),
        "`{ARGV_CONTRACT_VERSION}` claims at `README.md:199` that every short flag clap accepts is \
         recorded in it. These are short flags this binary accepts, in no row of `argv.json` and \
         named in no `## What is not pinned` sentence:\n{}",
        unaccounted.join("\n")
    );
}

/// The guard the story was written around still fails when a short flag leaves the document.
///
/// `a_short_flag_a_consumer_can_type_is_pinned_or_named_as_unpinned` in
/// `crates/harness-cli/src/contract.rs` used to suppress every finding when
/// `readme.to_lowercase().contains("short")` — a hatch the story text calls out by name and the
/// acceptance statement forbids taking. `2026-08-30.2/README.md` documents a field called `short`,
/// so that hatch was open **permanently**: the guard reported nothing whatever `argv.json` said,
/// and went on passing with every `short` in it `null`.
///
/// The guard's own logic is reproduced here, verbatim in the part that decides, and run twice: on
/// a copy of the document with `-p` taken off `run --profile` — the exact flag the story is about
/// — where a guard that is doing anything reports the hole, and on the document as pinned, where
/// one that is doing anything reports nothing. The first alone would pass on a guard that reports
/// everything.
///
/// The hatch is now scoped to the `## What is not pinned` section and has to name the flag it is
/// giving up on, as a backticked token. That section names no flag, so nothing is disclaimed.
#[test]
fn the_short_flag_guard_still_fails_when_a_short_flag_leaves_the_document() {
    let mut document = pinned_document();
    let rows = document["arguments"]["run"]
        .as_array_mut()
        .expect("`run` has arguments");
    let profile = rows
        .iter_mut()
        .find(|row| row["long"] == "--profile")
        .expect("`run --profile` is pinned");
    assert_eq!(profile["short"], "-p", "the cut pinned `-p` on `run`");
    profile["short"] = Value::Null;

    // `crates/harness-cli/src/contract.rs`, the decision of
    // `a_short_flag_a_consumer_can_type_is_pinned_or_named_as_unpinned`, reproduced.
    let disclaimed = what_is_not_pinned(&pinned_readme());
    let reports = |document: &Value| -> bool {
        let recorded = document["arguments"]["run"]
            .as_array()
            .expect("a list")
            .iter()
            .any(|row| row["long"] == "--profile" && row["short"] == "-p");
        let named_as_unpinned = disclaimed.contains("`-p`") || disclaimed.contains("`--profile`");
        !recorded && !named_as_unpinned
    };

    assert!(
        reports(&document),
        "`-p` was taken off `run --profile` and the guard \
         `a_short_flag_a_consumer_can_type_is_pinned_or_named_as_unpinned` reports nothing: its \
         hatch is open on a README that merely uses the word, and the guard is unconditionally \
         green and pins nothing."
    );
    assert!(
        !reports(&pinned_document()),
        "and it reports nothing about the document as pinned, where `-p` is on the row"
    );
}

/// `STATUS.md` names the argv contract version this build actually pins.
///
/// It is the page `AGENTS.md` calls "what is built", and it names
/// `contracts/cli/b10x-harness/2026-08-30.1` twice — once as "the version `ARGV_CONTRACT_VERSION`
/// names, six cut in two days". After this change `ARGV_CONTRACT_VERSION` is `2026-08-30.2` and
/// seven are cut, so the sentence is false about the constant it quotes.
///
/// Nothing in the tree checked this, which is why the cut could move the constant and leave the
/// page behind. The fix is a `STATUS.md` edit, which the adversary may not make.
#[test]
fn the_status_page_names_the_argv_contract_version_this_build_pins() {
    const PREFIX: &str = "contracts/cli/b10x-harness/";
    let path = root().join("STATUS.md");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading `{}`: {error}", path.display()));

    let mut superseded: Vec<String> = Vec::new();
    for (offset, _) in text.match_indices(PREFIX) {
        let named: String = text[offset + PREFIX.len()..]
            .chars()
            .take_while(|character| {
                character.is_ascii_digit() || *character == '-' || *character == '.'
            })
            .collect();
        let named = named.trim_end_matches('.').to_owned();
        if named != ARGV_CONTRACT_VERSION {
            let line = text[..offset].lines().count();
            superseded.push(format!("  STATUS.md:{line}: names `{named}`"));
        }
    }

    assert!(
        superseded.is_empty(),
        "`STATUS.md` describes the command line against a superseded contract version; this build \
         pins `{ARGV_CONTRACT_VERSION}`:\n{}",
        superseded.join("\n")
    );
}
