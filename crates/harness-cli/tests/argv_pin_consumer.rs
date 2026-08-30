//! The pinned command-line document, read by a consumer that has nothing else.
//!
//! `contracts/cli/b10x-harness/<version>/README.md` names metaharness's `b10x` adapter as the
//! consumer this contract is for: it launches this binary and builds the invocation out of the pin.
//! These cases are that consumer. They read `argv.json` and the README, assemble a command line
//! from what those two say and from nothing else, and run it.
//!
//! Two fields decide whether that works. `required` says whether omitting a flag is a parse error,
//! and `default` says what a flag means when it is left out. This binary settles both of them
//! **after** clap has parsed — `RunOptions::wire()` defaults the wire last, so a provider may set
//! it and a typed flag may beat it, and `apply_profiles` refuses a run that has neither the
//! endpoint flags nor a configured provider — and a document generated from clap's own definition
//! (`AGENTS.md` invariant 14) can see neither. What it can do instead is say so, in the section it
//! keeps for what it does not pin, and that is what these cases hold it to.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use b10x_harness_cli::contract::ARGV_CONTRACT_VERSION;
use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

/// The heading under which the document gives up on saying which flags a run really demands.
const DEMANDED: &str = "Flags a run demands that clap does not";

/// The heading under which it gives up on saying what a flag means when it is left out.
const DEFAULTED: &str = "Defaults this binary applies after clap";

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

/// The body under one heading, up to the next heading of any level.
///
/// Empty where there is no such heading, so a document that says nothing fails on what it does not
/// say rather than on the reader not finding it.
fn section(readme: &str, heading: &str) -> String {
    let mut inside = false;
    let mut body: Vec<&str> = Vec::new();
    for line in readme.lines() {
        if line.starts_with('#') {
            inside = line.trim_start_matches('#').trim() == heading;
            continue;
        }
        if inside {
            body.push(line);
        }
    }
    body.join("\n")
}

/// Every span between a pair of backticks, in order.
fn backticked(text: &str) -> impl Iterator<Item = &str> {
    text.split('`').skip(1).step_by(2)
}

/// Every long flag one section names as a backticked token.
fn flags_named(section: &str) -> BTreeSet<String> {
    backticked(section)
        .filter(|token| token.starts_with("--"))
        .map(str::to_owned)
        .collect()
}

/// One command's row for one long flag, as the document holds it.
fn row<'a>(document: &'a Value, command: &str, long: &str) -> Option<&'a Value> {
    document["arguments"][command]
        .as_array()?
        .iter()
        .find(|row| row["long"] == long)
}

/// Every command whose endpoint flags this document records as optional.
///
/// Derived rather than listed: `app-server` takes the same two flags and clap requires them there,
/// so it is honest already and does not belong in the set. What is left is every command that
/// flattens `RunOptions` for a run of the loop.
fn commands_deferring_the_endpoint(document: &Value) -> Vec<String> {
    let mut deferring: Vec<String> = document["arguments"]
        .as_object()
        .expect("an object")
        .keys()
        .filter(|command| {
            ["--base-url", "--model"].iter().all(|long| {
                row(document, command, long).is_some_and(|row| row["required"] == false)
            })
        })
        .cloned()
        .collect();
    deferring.sort();
    deferring
}

/// The command line the document alone says is enough: the words, then every flag it marks
/// required, then every word it says is typed after the verb.
///
/// A required flag whose value the document cannot supply — the document says a placeholder, not a
/// value — is taken from `values`, and one that is in neither is a failure by name rather than a
/// command line quietly assembled without it.
fn minimal_invocation(
    document: &Value,
    command: &str,
    values: &BTreeMap<&str, String>,
) -> Vec<String> {
    let mut argv: Vec<String> = command.split_whitespace().map(str::to_owned).collect();
    for listed in document["arguments"][command]
        .as_array()
        .unwrap_or_else(|| panic!("`{command}` has a flag list in the pinned document"))
    {
        if listed["required"] != true {
            continue;
        }
        let long = listed["long"].as_str().expect("a long flag");
        argv.push(long.to_owned());
        if listed["takes_value"] == true {
            argv.push(
                values
                    .get(long)
                    .unwrap_or_else(|| {
                        panic!("`{command} {long}` is required and this case has no value for it")
                    })
                    .clone(),
            );
        }
    }
    for positional in document["positionals"][command]
        .as_array()
        .unwrap_or_else(|| panic!("`{command}` has a positional list in the pinned document"))
    {
        if positional["required"] != true {
            continue;
        }
        let name = positional["name"].as_str().expect("a placeholder");
        argv.push(
            values
                .get(name)
                .unwrap_or_else(|| {
                    panic!("`{command} <{name}>` is required and this case has no value for it")
                })
                .clone(),
        );
    }
    argv
}

struct Output {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

/// The binary, with a config directory the case owns and nothing on standard input.
///
/// **The config directory is the point.** `--base-url` and `--model` may be supplied by a
/// `[default]` provider in `$XDG_CONFIG_HOME/b10x/harness.toml`, so a case run against whatever the
/// machine happens to have configured would measure that machine rather than this document. An
/// empty directory is the state the pin describes: a consumer with the document and nothing else.
fn refused(argv: &[String], config: &Path) -> Output {
    let output = Command::new(BINARY)
        .args(argv)
        .env("XDG_CONFIG_HOME", config)
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");
    Output {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// Every flag this binary demands, that clap does not, is one the document accounts for.
///
/// `run`, `chat` and `workflow run` record `--base-url` and `--model` as `"required": false`, which
/// is true of clap and false of the run: `apply_profiles` refuses one that has neither those flags
/// nor a `[default]` provider, and the refusal names them. A consumer that took `required` at its
/// word built `b10x-harness run --input …` and got exit `1`.
///
/// So the demand is **measured** — the minimal command line the document describes is assembled and
/// run, and the flags the refusal names in backticks are read out of its own words — and each one
/// has to be either `"required": true` in the document or named in the *What is not pinned* section
/// beside the command it is typed after. Naming both, and as backticked tokens, is what stops the
/// escape being a sentence that gestures at the problem: a document that gave up on `--model`
/// everywhere by naming it once would say nothing about which command lines are affected.
///
/// **`workflow run` is checked against the document and not run**, alone of the three. It flattens
/// the same `RunOptions` and records the same two rows as optional, but `workflow::dispatch` never
/// calls `apply_profiles`, so the same command line panics at `RunOptions::base_url`'s `expect`
/// with exit `101` instead of being refused by name. That is a defect in the binary rather than in
/// the document and it is not this unit's to fix; what is this unit's is that the document has to
/// account for the row on that command too, which is asserted below.
#[test]
fn a_flag_this_binary_demands_and_clap_does_not_is_named_by_the_document() {
    let document = pinned_document();
    let readme = pinned_readme();
    let demanded = section(&readme, DEMANDED);
    let config = tempfile::tempdir().expect("a temporary config directory");

    let mut values = BTreeMap::new();
    values.insert(
        "--input",
        "read the readme and tell me what it says".to_owned(),
    );

    let mut measured: BTreeSet<String> = BTreeSet::new();
    let mut unaccounted: Vec<String> = Vec::new();
    for command in ["run", "chat"] {
        let argv = minimal_invocation(&document, command, &values);
        let output = refused(&argv, config.path());
        assert_eq!(
            output.status,
            Some(1),
            "`b10x-harness {}` is the command line this document describes, and it is refused \
             rather than parsed away or panicked on.\nstdout: {}\nstderr: {}",
            argv.join(" "),
            output.stdout,
            output.stderr
        );
        for named in backticked(&output.stderr).filter(|token| token.starts_with("--")) {
            if row(&document, command, named).is_none() {
                continue;
            }
            measured.insert(named.to_owned());
        }
    }
    assert!(
        !measured.is_empty(),
        "no flag was measured as demanded, so this case is asserting nothing: the refusal stopped \
         naming the flags it wants, or stopped putting them in backticks"
    );

    for command in commands_deferring_the_endpoint(&document) {
        for long in &measured {
            let Some(row) = row(&document, &command, long) else {
                continue;
            };
            if row["required"] == true {
                continue;
            }
            let named = demanded.contains(&format!("`{command}`"))
                && demanded.contains(&format!("`{long}`"));
            if !named {
                unaccounted.push(format!("  `{command}`: `{long}`"));
            }
        }
    }

    assert!(
        unaccounted.is_empty(),
        "`{ARGV_CONTRACT_VERSION}` records these flags as `\"required\": false` and this binary \
         refuses the run by name without them. A consumer building an invocation from the document \
         alone gets a command line that does not run, and the `## What is not pinned` section's \
         `### {DEMANDED}` names neither the command nor the flag:\n{}",
        unaccounted.join("\n")
    );
}

struct Fixture {
    child: Child,
    base_url: String,
}

impl Fixture {
    /// The deterministic local endpoint, announcing the address it came up on.
    ///
    /// Evidence from here is `provider_emulated` and never `vendor_live` (`AGENTS.md` invariant
    /// 18): no provider is contacted by any case in this file.
    fn start() -> Self {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("harness-responses")
            .join("tests")
            .join("fixtures")
            .join("fake_responses.py");
        let mut child = Command::new("python3")
            .arg(&script)
            .arg("--scenario")
            .arg("text")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the fixture starts; python3 must be available");
        let mut line = String::new();
        BufReader::new(child.stdout.as_mut().expect("piped stdout"))
            .read_line(&mut line)
            .expect("the fixture announces its address");
        let ready: Value = serde_json::from_str(&line).expect("the announcement is JSON");
        Self {
            base_url: ready["base_url"].as_str().expect("a base url").to_owned(),
            child,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A consumer with the document and nothing else builds a command line that runs, and can say
/// which wire it will be on before it runs it.
///
/// The two halves of the acceptance, in one invocation. The command line is assembled from the
/// document's `required` rows plus every flag its *What is not pinned* section says a run demands
/// anyway — nothing else is consulted — and it has to reach the endpoint and exit `0`.
///
/// Then the wire. `--wire` records `"default": null`, which the document defines as *the value used
/// when the flag is absent*; the flag is absent here and the session this run files says
/// `openai-responses`, because `RunOptions::wire()` defaults it after clap. So the document either
/// records that value on the row or names the flag, the command and the value it really gets — and
/// the value is the part that makes the sentence worth reading: a consumer told only that
/// *something* is decided elsewhere still cannot say which wire its invocation will speak.
///
/// `--workspace` and `--session-dir` are passed beyond the document's own set, and both are
/// recorded defaults (`.` and `$XDG_STATE_HOME/b10x-harness/sessions`) pointed somewhere this case
/// owns: a suite that files real sessions on the machine it runs on is one nobody can run twice.
/// The session is also how the wire is read at all — it is the run's own record of which wire
/// produced its items, kept because an opaque item may not cross wires.
#[test]
fn an_invocation_built_from_the_document_alone_runs_and_says_which_wire_it_is_on() {
    let document = pinned_document();
    let readme = pinned_readme();
    let fixture = Fixture::start();
    let config = tempfile::tempdir().expect("a temporary config directory");
    let workspace = tempfile::tempdir().expect("a temporary workspace");
    let sessions = tempfile::tempdir().expect("a temporary session directory");
    std::fs::File::create(workspace.path().join("README.md"))
        .and_then(|mut file| file.write_all(b"hello harness\n"))
        .expect("a readable file in the workspace");

    let mut values = BTreeMap::new();
    values.insert(
        "--input",
        "read the readme and tell me what it says".to_owned(),
    );
    values.insert("--base-url", fixture.base_url.clone());
    values.insert("--model", "b10x-emulated".to_owned());

    let mut argv = minimal_invocation(&document, "run", &values);
    for long in flags_named(&section(&readme, DEMANDED)) {
        if row(&document, "run", &long).is_none() || argv.contains(&long) {
            continue;
        }
        argv.push(long.clone());
        argv.push(
            values
                .get(long.as_str())
                .unwrap_or_else(|| {
                    panic!("`run` is told to supply `{long}` and this case has no value for it")
                })
                .clone(),
        );
    }
    argv.push("--workspace".to_owned());
    argv.push(workspace.path().display().to_string());
    argv.push("--session-dir".to_owned());
    argv.push(sessions.path().display().to_string());

    let output = refused(&argv, config.path());
    assert_eq!(
        output.status,
        Some(0),
        "`b10x-harness {}` is everything `{ARGV_CONTRACT_VERSION}` says a run needs, and it did \
         not run.\nstdout: {}\nstderr: {}",
        argv.join(" "),
        output.stdout,
        output.stderr
    );

    let mut files: Vec<PathBuf> = std::fs::read_dir(sessions.path())
        .expect("the session directory exists")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    assert_eq!(files.len(), 1, "one session was written: {files:?}");
    let session: Value = serde_json::from_str(
        &std::fs::read_to_string(files.pop().expect("one file")).expect("readable"),
    )
    .expect("a session file");
    let effective = session["wire"]
        .as_str()
        .expect("the session names its wire");

    assert!(
        !argv.iter().any(|word| word == "--wire"),
        "the wire below is the one this binary chose, and the case only measures that while it \
         leaves the flag out: {argv:?}"
    );
    let recorded = row(&document, "run", "--wire").expect("`run --wire` is a row of the document");
    if recorded["default"] == effective {
        return;
    }
    let defaulted = section(&readme, DEFAULTED);
    assert!(
        defaulted.contains("`run`")
            && defaulted.contains("`--wire`")
            && defaulted.contains(&format!("`{effective}`")),
        "`run --wire` records `\"default\": {}` and this run got `{effective}` with the flag left \
         out. The document defines `default` as the value used when the flag is absent, so it has \
         to record that value or say — in `## What is not pinned`, under `### {DEFAULTED}`, naming \
         the command, the flag and the value — that it does not.",
        recorded["default"]
    );
}
