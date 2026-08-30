//! The pinned command-line document, read by a consumer that has nothing else.
//!
//! `contracts/cli/b10x-harness/<version>/README.md` names metaharness's `b10x` adapter as the
//! consumer this contract is for: it launches this binary and builds the invocation out of the pin.
//! These cases are that consumer. They read `argv.json` and the README, assemble a command line
//! from what those two say and from nothing else, and run it.
//!
//! Two fields decide whether that works. `required` says whether omitting a flag is a parse error,
//! and `default` says what a flag means when it is left out. This binary settles both of them
//! **after** clap has parsed — `RunOptions::wire()` defaults the wire last so a provider may set it
//! first, `session_dir()` picks a state directory out of the environment, and `apply_profiles`
//! refuses a run that has neither the endpoint flags nor a configured provider — and a document
//! generated from clap's own definition (`AGENTS.md` invariant 14) can see none of it. What it can
//! do instead is say so, and these cases hold it to *what* it says rather than to *whether it
//! mentions the words*.
//!
//! # The escape is a table, and that is the whole of why these cases bite
//!
//! An earlier version of this file asked whether the disclaiming section contained `` `run` `` and
//! `` `--model` `` somewhere. Every actionable word could be deleted from it and the suite stayed
//! green, and a disclaimer stating the **opposite** of the truth passed for holding the same
//! tokens. So the escape is now one **table row per command and flag**, its cells compared for
//! equality in fixed columns, one line consumed per claim — [`rows_missing`], which is also what
//! the move table in `crates/harness-cli/src/contract.rs` is checked with, so the two cannot drift.
//!
//! And what the row has to say is **measured first**. The commands and flags a run demands are read
//! out of the binary's own refusals by supplying what it names until it stops refusing; the value a
//! flag takes when it is absent is read out of the session the run files. The table then has to
//! match that, in both directions: a command the binary does not demand a flag on may not carry a
//! row promising that it does.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use b10x_harness_cli::contract::{ARGV_CONTRACT_VERSION, rows_missing};
use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

/// The heading under which the document says which flags a run demands that clap does not.
const DEMANDED: &str = "Flags a run demands that clap does not";

/// The heading under which it says what a flag means when it is left out.
const DEFAULTED: &str = "Defaults this binary applies after clap";

/// The cell a demanded flag's row has to carry, in the column after the flag.
///
/// Fixed, and in a column, so that a row cannot state the opposite claim in the same words: a
/// consumer scanning the table reads *what happens without it* out of one place.
const WITHOUT_IT: &str = "`refused by name`";

/// The two cells the `when` column may hold, and the whole of what a consumer acts on.
///
/// **Claimed, because an unclaimed column is an unpinned one.** [`rows_missing`] matches a window
/// of cells, so a column outside the claim is free text — and this is the column that decides
/// whether a driver types the flag. Left free, it took *"never in practice; every endpoint this
/// binary knows is reachable without it"* against `chat`/`--base-url` and *"never; a run always
/// invents a directory"* against `run`/`--session-dir` with the suite green, which is the original
/// defect restored underneath a pinned table.
///
/// So it holds one of exactly two tokens and the measurement decides which: the flag is demanded
/// on the ordinary machine too, or only where the environment names no state directory.
const ALWAYS: &str = "`always`";
const ONLY_WITHOUT_A_STATE_DIRECTORY: &str =
    "`only where the environment names no state directory`";

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
/// Empty where there is no such heading, so a document that stopped saying something fails on what
/// it no longer says rather than on the reader not finding the place it used to say it.
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

/// One command's row for one long flag, as the document holds it.
fn row<'a>(document: &'a Value, command: &str, long: &'a str) -> Option<&'a Value> {
    document["arguments"][command]
        .as_array()?
        .iter()
        .find(|row| row["long"] == long)
}

/// Every command something outside the command line has to complete, named by the document.
///
/// Two shapes, and the second was missed once. A command whose `--base-url` and `--model` rows both
/// say `"required": false` takes its endpoint from a provider; a command recording `--session-dir`
/// takes its state directory from the environment. `sessions` is only the second — it records
/// neither endpoint flag — so a set built from the endpoint pair alone could not see that
/// `b10x-harness sessions`, a complete command line under this document, is refused by name on a
/// machine with no state directory. The `--session-dir` row had already been generalised that way
/// for the defaults table and not for this one; the same flag was in one and not the other.
///
/// `app-server` takes the endpoint pair and clap requires them there, so its rows already say what
/// they mean and it is not in this set.
fn commands_the_environment_completes(document: &Value) -> Vec<String> {
    let mut deferring: Vec<String> = document["arguments"]
        .as_object()
        .expect("an object")
        .keys()
        .filter(|command| {
            ["--base-url", "--model"].iter().all(|long| {
                row(document, command, long).is_some_and(|row| row["required"] == false)
            }) || row(document, command, "--session-dir").is_some()
        })
        .cloned()
        .collect();
    deferring.sort();
    deferring
}

/// A subsection that is nothing but its own table.
///
/// **F5, and it is the same hole as J4 one paragraph over.** The table is pinned row by row; a
/// paragraph beside it is not, and *"None of the rows above is a requirement: every flag in that
/// table is optional in every environment this binary supports"* left the suite green directly
/// under it. A reader takes the sentence and the driver takes the table, and they said opposite
/// things.
///
/// What is checkable is the shape: these two subsections carry table lines and nothing else, so
/// there is no room beside a row for a sentence denying it. Prose that explains the tables lives in
/// a subsection of its own, where it makes no per-row claim.
fn lines_beside_the_table(section: &str) -> Vec<String> {
    section
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter(|line| !line.trim_start().starts_with('|'))
        .map(|line| format!("  {}", line.trim()))
        .collect()
}

/// The words the document alone says are enough to type: the command, then every flag it marks
/// required, then every word it says is typed after the verb.
fn minimal_invocation(
    document: &Value,
    command: &str,
    values: &BTreeMap<String, String>,
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
            argv.push(value_for(values, long, command));
        }
    }
    for positional in document["positionals"][command]
        .as_array()
        .unwrap_or_else(|| panic!("`{command}` has a positional list in the pinned document"))
    {
        if positional["required"] == true {
            let name = positional["name"].as_str().expect("a placeholder");
            argv.push(value_for(values, name, command));
        }
    }
    argv
}

/// A value for a word the document says is needed but cannot supply — it records a placeholder,
/// not a value. Missing is a failure by name, never a command line quietly assembled without it.
fn value_for(values: &BTreeMap<String, String>, key: &str, command: &str) -> String {
    values
        .get(key)
        .unwrap_or_else(|| panic!("`{command}` needs `{key}` and this case has no value for it"))
        .clone()
}

struct Output {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Which machine a case measures on, and how bare it is.
///
/// **The config directory is always the case's own and always empty.** `--base-url` and `--model`
/// may be supplied by a `[default]` provider in `$XDG_CONFIG_HOME/b10x/harness.toml`, so a case run
/// against whatever the machine happens to have configured would measure that machine rather than
/// this document.
#[derive(Clone, Copy)]
enum Machine<'a> {
    /// A state directory exists, so a session has somewhere to go.
    Ordinary { config: &'a Path, state: &'a Path },
    /// Neither `XDG_STATE_HOME` nor `HOME`: a container, a `systemd` unit, `env -i`, a CI step
    /// that cleared the environment. The document's audience is a driver, not a login shell.
    Bare { config: &'a Path },
}

fn invoke(argv: &[String], machine: Machine<'_>) -> Output {
    let mut command = Command::new(BINARY);
    command.args(argv).stdin(Stdio::null());
    match machine {
        Machine::Ordinary { config, state } => {
            command.env("XDG_CONFIG_HOME", config);
            command.env("XDG_STATE_HOME", state);
        }
        Machine::Bare { config } => {
            command.env_clear();
            command.env("XDG_CONFIG_HOME", config);
            command.env(
                "PATH",
                std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into()),
            );
        }
    }
    let output = command.output().expect("the binary runs");
    Output {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// What one command demanded beyond the document's own `required` rows, and whether it ever ran.
enum Demand {
    /// It ran. These flags are the ones whose removal put the refusal back — and put it back
    /// **naming them**, which is the claim the table's third cell makes.
    Ran {
        demanded: BTreeSet<String>,
        /// Removing it refused the run, and the refusal did not name it. Demanded, but not by
        /// name, so a consumer is told to change something and not what.
        unnamed: BTreeSet<String>,
    },
    /// It never ran, and the last refusal named nothing a consumer could add. A command line the
    /// document describes that no reader of the document can repair.
    Stuck {
        supplied: BTreeSet<String>,
        status: Option<i32>,
        stderr: String,
    },
}

/// The command line, with these flags added to the document's own required rows.
fn with(
    document: &Value,
    command: &str,
    values: &BTreeMap<String, String>,
    added: &BTreeSet<String>,
) -> Vec<String> {
    let mut argv = minimal_invocation(document, command, values);
    for long in added {
        argv.push(long.clone());
        if row(document, command, long).expect("a row")["takes_value"] == true {
            argv.push(value_for(values, long, command));
        }
    }
    argv
}

/// Every long flag of this command a message names in its own backticks.
fn flags_named_in(document: &Value, command: &str, message: &str) -> BTreeSet<String> {
    message
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|token| token.starts_with("--"))
        .filter(|token| row(document, command, token).is_some())
        .map(str::to_owned)
        .collect()
}

/// The consumer's own loop, run against the binary: type what the document says, read the flags the
/// refusal names in its own backticks, add them, try again — and then take back out whatever the
/// run turns out not to need.
///
/// This is the measurement every case below rests on, and it is a measurement rather than a list:
/// nothing here names `--base-url` or `--session-dir`, and a flag that stopped being demanded would
/// stop being returned.
///
/// **The taking back out is not tidiness.** A refusal may offer alternatives — the one that wants a
/// state directory names `--session-dir` *and* `--no-session` — and a consumer that added both
/// would have this file report a demand for a flag the run is perfectly happy without. So each
/// added flag is removed again, in name order and therefore deterministically, and kept only if the
/// run is refused without it. What that leaves is one of possibly several sufficient sets, which is
/// the honest answer to *what must a consumer type*: the alternative dropped first is the one the
/// document's readers are not told they need, and the one kept is.
fn demanded_by(
    document: &Value,
    command: &str,
    values: &BTreeMap<String, String>,
    machine: Machine<'_>,
) -> Demand {
    let mut supplied: BTreeSet<String> = BTreeSet::new();
    loop {
        let output = invoke(&with(document, command, values, &supplied), machine);
        if output.status == Some(0) {
            break;
        }
        let named: BTreeSet<String> = flags_named_in(document, command, &output.stderr)
            .difference(&supplied)
            .cloned()
            .collect();
        if named.is_empty() {
            return Demand::Stuck {
                supplied,
                status: output.status,
                stderr: output.stderr,
            };
        }
        supplied.extend(named);
    }

    let mut demanded = supplied.clone();
    let mut unnamed = BTreeSet::new();
    for candidate in &supplied {
        let mut without = demanded.clone();
        without.remove(candidate);
        let output = invoke(&with(document, command, values, &without), machine);
        if output.status == Some(0) {
            demanded = without;
        } else if !flags_named_in(document, command, &output.stderr).contains(candidate) {
            unnamed.insert(candidate.clone());
        }
    }
    Demand::Ran { demanded, unnamed }
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

/// A workspace with one readable file, and a one-step flow document beside it.
fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary directory");
    std::fs::File::create(dir.path().join("README.md"))
        .and_then(|mut file| file.write_all(b"hello harness\n"))
        .expect("a readable file in the workspace");
    std::fs::write(
        dir.path().join("one-step.yaml"),
        "id: one-step\nroot:\n  id: root\n  nodes:\n    - id: shape\n      nodes:\n        - id: \
         specify\n          run:\n            state: specify\n            summary: \"State the \
         required behaviour.\"\n",
    )
    .expect("a flow document");
    dir
}

/// Everything a case may have to type that the document records a placeholder for.
fn values(fixture: &Fixture, workspace: &Path, sessions: &Path) -> BTreeMap<String, String> {
    [
        ("--input", "read the readme and tell me what it says"),
        ("--base-url", fixture.base_url.as_str()),
        ("--model", "b10x-emulated"),
        (
            "--flow",
            &workspace.join("one-step.yaml").display().to_string(),
        ),
        ("--session-dir", &sessions.display().to_string()),
        ("--workspace", &workspace.display().to_string()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect()
}

/// What measuring every command left behind: the rows the tables owe, and why some commands owe
/// none.
struct Measured {
    /// One claim per command and flag, four cells each.
    wanted: Vec<Vec<String>>,
    /// Commands whose command line no reader of the document can repair.
    stuck: Vec<String>,
    /// Rows the section states that the binary does not behave that way on.
    forbidden: Vec<String>,
    /// Whether anything at all was measured, so a silent binary cannot make this vacuous.
    anything: bool,
}

/// Every command, on both machines, reduced to the rows the demands table owes.
///
/// Split out of the case so the case reads as *measure, then hold the table to it* — and because
/// which machine demanded a flag is the whole of the `when` cell, the two are kept apart rather
/// than unioned.
fn measure_demands(
    document: &Value,
    values: &BTreeMap<String, String>,
    config: &Path,
    state: &Path,
    demanded: &str,
) -> Measured {
    // Indices, because the `when` cell is decided by *which* of them demanded the flag.
    const ORDINARY: usize = 0;
    const BARE: usize = 1;
    let machines = [
        Machine::Ordinary { config, state },
        Machine::Bare { config },
    ];

    let mut wanted: Vec<Vec<String>> = Vec::new();
    let mut stuck: Vec<String> = Vec::new();
    let mut forbidden: Vec<String> = Vec::new();
    let mut measured_anything = false;
    for command in commands_the_environment_completes(document) {
        // Kept apart per machine, because which machine demanded a flag *is* the `when` cell.
        let mut demands: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
        let mut unrepairable = false;
        for (nth, machine) in machines.iter().enumerate() {
            match demanded_by(document, &command, values, *machine) {
                Demand::Ran { demanded, unnamed } => {
                    demands.insert(nth, demanded);
                    for long in unnamed {
                        forbidden.push(format!(
                            "  `{command}`: without `{long}` the run is refused and the refusal \
                             does not name it, so `{WITHOUT_IT}` is not what happens"
                        ));
                    }
                }
                Demand::Stuck {
                    supplied,
                    status,
                    stderr,
                } => {
                    unrepairable = true;
                    stuck.push(format!(
                        "  `{command}`: exited {status:?} after supplying {supplied:?}; the \
                         refusal names no further flag of this command.\n    stderr: {}",
                        stderr.trim()
                    ));
                }
            }
        }
        if unrepairable {
            // Nothing the document could say makes this command line run, so a row promising a
            // refusal by name is a promise the binary does not keep.
            if demanded.contains(&format!("`{command}`")) {
                forbidden.push(format!(
                    "  `{command}` is named in `### {DEMANDED}`, and a command line built from the \
                     document alone does not reach a refusal by name on it"
                ));
            }
            continue;
        }
        let ordinary = demands.get(&ORDINARY).cloned().unwrap_or_default();
        let bare = demands.get(&BARE).cloned().unwrap_or_default();
        let missed: Vec<&String> = ordinary.difference(&bare).collect();
        assert!(
            missed.is_empty(),
            "`{command}` demands {missed:?} on the ordinary machine and not on the barer one, \
             which cannot be: the bare machine has strictly less. The two environments have to be \
             re-derived."
        );
        measured_anything |= !bare.is_empty();
        for long in &bare {
            wanted.push(vec![
                format!("`{command}`"),
                format!("`{long}`"),
                WITHOUT_IT.to_owned(),
                if ordinary.contains(long) {
                    ALWAYS.to_owned()
                } else {
                    ONLY_WITHOUT_A_STATE_DIRECTORY.to_owned()
                },
            ]);
        }
    }
    Measured {
        wanted,
        stuck,
        forbidden,
        anything: measured_anything,
    }
}

/// The escape table names exactly the commands and flags this binary demands and clap does not.
///
/// **Both directions, and both measured.** A row that is missing leaves a consumer building a
/// command line that does not run; a row that is there for a command the binary does not behave
/// that way on is a pinned document making a promise the binary does not keep, which is the defect
/// the whole chain of these READMEs exists to stop repeating.
///
/// The demand is measured by [`demanded_by`] on two machines — one with a state directory and one
/// with neither `XDG_STATE_HOME` nor `HOME` — because `--session-dir` is only demanded on the
/// second, and a driver in a container is exactly the reader this document is written for.
///
/// **`workflow run` is measured and comes back [`Demand::Stuck`].** It flattens the same options
/// and records the same rows, and `workflow::dispatch` never calls `apply_profiles`, so the command
/// line the document describes aborts at `RunOptions::base_url`'s `expect` with exit `101` and a
/// panic message naming no flag. A consumer cannot repair that from the document, so the table may
/// not promise a refusal by name there — and this case fails if it does. The binary's half is
/// `story:workflow-run-panics-and-drops-its-profile`; when it lands, `workflow run` starts coming
/// back [`Demand::Ran`] and this case will require its rows.
#[test]
fn the_escape_table_names_the_flags_this_binary_demands_and_clap_does_not() {
    let document = pinned_document();
    let demanded = section(&pinned_readme(), DEMANDED);
    let fixture = Fixture::start();
    let config = tempfile::tempdir().expect("a temporary config directory");
    let state = tempfile::tempdir().expect("a temporary state directory");
    let sessions = tempfile::tempdir().expect("a temporary session directory");
    let workspace = workspace();
    let values = values(&fixture, workspace.path(), sessions.path());

    let Measured {
        wanted,
        stuck,
        forbidden,
        anything,
    } = measure_demands(&document, &values, config.path(), state.path(), &demanded);
    let beside = lines_beside_the_table(&demanded);
    assert!(
        beside.is_empty(),
        "`### {DEMANDED}` carries its table and nothing else, so that no sentence stands beside a \
         row to deny it. Move the prose to a subsection of its own:\n{}",
        beside.join("\n")
    );

    assert!(
        anything,
        "no command demanded a flag the document does not mark required, so this case is asserting \
         nothing: either the binary stopped refusing, or its refusals stopped naming the flags in \
         backticks and the measurement has to be re-derived from what they say now.\nstuck: {}",
        stuck.join("\n")
    );

    let missing = rows_missing(&demanded, &wanted);
    assert!(
        missing.is_empty() && forbidden.is_empty(),
        "`{ARGV_CONTRACT_VERSION}`'s `### {DEMANDED}` has to carry one table row per command and \
         flag — `| <command> | <flag> | {WITHOUT_IT} |`, cells side by side and one line each, so \
         that a sentence holding the same words in prose is not an answer and one wide row is not \
         six answers.\nrows this binary's own refusals demand and the section does not \
         state:\n{}\nrows the section states and the binary does not behave that way on:\n{}\nfor \
         reference, commands whose command line no consumer can repair from the document:\n{}",
        missing
            .iter()
            .map(|cells| format!("  | {} |", cells.join(" | ")))
            .collect::<Vec<_>>()
            .join("\n"),
        forbidden.join("\n"),
        stuck.join("\n")
    );
}

/// A consumer with the document and nothing else builds a command line that runs.
///
/// The first half of the acceptance, and the one a driver feels: the invocation is assembled from
/// the document's `required` rows plus every flag its escape table says a run demands, nothing else
/// is consulted, and it has to reach the endpoint and exit `0`.
///
/// The flags come out of the **table**, cell by cell, rather than out of every backticked token in
/// the section — so a section that named a flag in prose while its table said nothing would build
/// the same command line as one that said nothing at all, and this case would fail exactly as it
/// did before the table existed.
#[test]
fn an_invocation_built_from_the_document_alone_reaches_the_endpoint() {
    let document = pinned_document();
    let demanded = section(&pinned_readme(), DEMANDED);
    let fixture = Fixture::start();
    let config = tempfile::tempdir().expect("a temporary config directory");
    let state = tempfile::tempdir().expect("a temporary state directory");
    let sessions = tempfile::tempdir().expect("a temporary session directory");
    let workspace = workspace();
    let values = values(&fixture, workspace.path(), sessions.path());

    let mut argv = minimal_invocation(&document, "run", &values);
    let told: Vec<String> = demanded
        .lines()
        .filter_map(|line| {
            let cells = table_cells(line)?;
            (cells.first().map(String::as_str) == Some("`run`")).then(|| cells[1].clone())
        })
        .map(|cell| cell.trim_matches('`').to_owned())
        .filter(|long| row(&document, "run", long).is_some())
        .collect();
    assert!(
        !told.is_empty(),
        "`### {DEMANDED}` tells a consumer of `run` nothing, and the case that says it should is \
         `the_escape_table_names_the_flags_this_binary_demands_and_clap_does_not`"
    );
    for long in told {
        if argv.contains(&long) {
            continue;
        }
        argv.push(long.clone());
        if row(&document, "run", &long).expect("a row")["takes_value"] == true {
            argv.push(value_for(&values, &long, "run"));
        }
    }
    argv.push("--workspace".to_owned());
    argv.push(workspace.path().display().to_string());

    let output = invoke(
        &argv,
        Machine::Ordinary {
            config: config.path(),
            state: state.path(),
        },
    );
    assert_eq!(
        output.status,
        Some(0),
        "`b10x-harness {}` is everything `{ARGV_CONTRACT_VERSION}` says a run needs, and it did \
         not run.\nstdout: {}\nstderr: {}",
        argv.join(" "),
        output.stdout,
        output.stderr
    );
}

/// The trimmed cells of one markdown table row, or nothing where the line is not a row.
fn table_cells(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if trimmed.len() < 2 || !trimmed.starts_with('|') || !trimmed.ends_with('|') {
        return None;
    }
    Some(
        trimmed
            .trim_matches('|')
            .split('|')
            .map(|cell| cell.trim().to_owned())
            .collect(),
    )
}

/// The values one `run` uses for `--wire` and `--session-dir` with neither flag typed.
///
/// One run measures both: the wire out of the session file's own record of which wire produced its
/// items, and the directory out of where that file landed. The directory comes back as the variable
/// a document could actually record rather than as this case's own temporary path.
fn defaults_run_uses(
    document: &Value,
    values: &BTreeMap<String, String>,
    workspace: &Path,
    config: &Path,
    state: &Path,
) -> [(&'static str, String); 2] {
    let mut argv = minimal_invocation(document, "run", values);
    for long in ["--base-url", "--model"] {
        argv.push(long.to_owned());
        argv.push(value_for(values, long, "run"));
    }
    argv.push("--workspace".to_owned());
    argv.push(workspace.display().to_string());
    for absent in ["--wire", "--session-dir"] {
        assert!(
            !argv.iter().any(|word| word == absent),
            "the values this returns are the ones the binary chose, and it only measures that \
             while it leaves `{absent}` out: {argv:?}"
        );
    }

    let output = invoke(&argv, Machine::Ordinary { config, state });
    assert_eq!(
        output.status,
        Some(0),
        "the run this measurement rests on did not run.\nstdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );

    let chosen = state.join("b10x-harness").join("sessions");
    let mut filed: Vec<PathBuf> = std::fs::read_dir(&chosen)
        .unwrap_or_else(|error| {
            panic!(
                "this measures the directory the binary picked with `--session-dir` absent; `{}`: \
                 {error}",
                chosen.display()
            )
        })
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.extension().is_some_and(|held| held == "json"))
        .collect();
    assert_eq!(filed.len(), 1, "one session was filed: {filed:?}");
    let session: Value = serde_json::from_str(
        &std::fs::read_to_string(filed.pop().expect("one file")).expect("readable"),
    )
    .expect("a session file");

    [
        (
            "--wire",
            session["wire"]
                .as_str()
                .expect("the session names its wire")
                .to_owned(),
        ),
        (
            "--session-dir",
            format!(
                "$XDG_STATE_HOME{}",
                chosen
                    .display()
                    .to_string()
                    .strip_prefix(&state.display().to_string())
                    .expect("the session landed under the state directory this case named")
            ),
        ),
    ]
}

/// Every default this binary applies after clap is a row of the escape table, carrying its value.
///
/// `default` is defined by the document as *the value used when the flag is absent*, and two rows
/// on every run-taking command record `null` while a value is used: `--wire`, which
/// `RunOptions::wire()` settles so a provider may set it first, and `--session-dir`, which
/// `session_dir()` reads out of the environment. The second is the one with teeth — a consumer
/// reading `null` as *nothing happens* has a harness filing a transcript on the operator's machine
/// every run, at a path no field of the document names.
///
/// **Both are measured from one run**, with neither flag typed: the wire out of the session file's
/// own record of which wire produced its items, and the directory out of where that file landed.
/// The value the table has to carry is then the measured one, in a cell of its own — so a row
/// naming the flag and saying something else about it, or saying the opposite, is not an answer.
///
/// The measurement is on `run` and the rows are required on every command whose row for that flag
/// is **byte-identical** to `run`'s. That is the document's own statement that the flag behaves the
/// same there, and it is the only generalisation available: `chat` reads a conversation from
/// standard input and `workflow run` aborts before it reaches either resolver, so neither can be
/// measured this way today.
#[test]
fn every_default_this_binary_applies_after_clap_is_a_row_carrying_its_value() {
    let document = pinned_document();
    let defaulted = section(&pinned_readme(), DEFAULTED);
    let fixture = Fixture::start();
    let config = tempfile::tempdir().expect("a temporary config directory");
    let state = tempfile::tempdir().expect("a temporary state directory");
    let sessions = tempfile::tempdir().expect("a temporary session directory");
    let workspace = workspace();
    let values = values(&fixture, workspace.path(), sessions.path());

    let measured = defaults_run_uses(
        &document,
        &values,
        workspace.path(),
        config.path(),
        state.path(),
    );

    let mut wanted: Vec<Vec<String>> = Vec::new();
    let mut recorded: Vec<String> = Vec::new();
    for (long, value) in &measured {
        let reference = row(&document, "run", long).expect("`run` has this row");
        if reference["default"] == value.as_str() {
            // The document could record it on the row after all, and did. Nothing to disclaim.
            recorded.push((*long).to_owned());
            continue;
        }
        for command in document["arguments"].as_object().expect("an object").keys() {
            if row(&document, command, long) == Some(reference) {
                wanted.push(vec![
                    format!("`{command}`"),
                    format!("`{long}`"),
                    format!("`{value}`"),
                ]);
            }
        }
    }
    assert!(
        !wanted.is_empty() || recorded.len() == measured.len(),
        "neither flag was measured as defaulted after clap, so this case is asserting nothing"
    );

    let beside = lines_beside_the_table(&defaulted);
    assert!(
        beside.is_empty(),
        "`### {DEFAULTED}` carries its table and nothing else, for the reason `### {DEMANDED}` \
         does: a sentence beside a row can deny it, and a reader cannot tell which of the two was \
         checked. Move the prose to a subsection of its own:\n{}",
        beside.join("\n")
    );

    let missing = rows_missing(&defaulted, &wanted);
    assert!(
        missing.is_empty(),
        "`{ARGV_CONTRACT_VERSION}` records `\"default\": null` for these, and this run used a value \
         with the flag left out. `### {DEFAULTED}` has to carry one table row per command and flag \
         — `| <command> | <flag> | <the value> |`, the value in a cell of its own — or the row has \
         to record it. Missing:\n{}",
        missing
            .iter()
            .map(|cells| format!("  | {} |", cells.join(" | ")))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
