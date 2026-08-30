//! The same consumer as `argv_pin_consumer.rs`, asked about a flag that file does not ask about.
//!
//! `story:argv-pin-carries-effective-defaults` accepts on **every** flag whose effective default or
//! requirement is decided after clap, not on the three the unit chose to look at. These cases take
//! the acceptance at its word and measure one more: `--session-dir`.
//!
//! It records `"default": null` on `run`, `chat` and `workflow run`, and the binary settles it after
//! clap in `session_dir()` (`crates/harness-cli/src/lib.rs:1961`) — `$XDG_STATE_HOME/b10x-harness/
//! sessions`, or `$HOME/.local/state/b10x-harness/sessions`, or a refusal by name when the machine
//! has neither. That is both halves of the story at once: a default the row does not carry, and a
//! requirement clap does not hold, on the same three commands the unit disclaimed `--wire` and
//! `--base-url`/`--model` for.
//!
//! `contracts/cli/b10x-harness/2026-08-30.2/README.md` says "`--wire` is the only flag of the three
//! that resolves this way". These cases are that sentence, run.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use b10x_harness_cli::contract::ARGV_CONTRACT_VERSION;
use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

/// The heading under which the document says which flags a run demands that clap does not.
const DEMANDED: &str = "Flags a run demands that clap does not";

/// The heading under which it says what a flag means when it is left out.
const DEFAULTED: &str = "Defaults this binary applies after clap";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("the repository root")
}

fn in_force() -> PathBuf {
    root()
        .join("contracts")
        .join("cli")
        .join("b10x-harness")
        .join(ARGV_CONTRACT_VERSION)
}

fn pinned_document() -> Value {
    let path = in_force().join("argv.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading `{}`: {error}", path.display()));
    serde_json::from_str(&text).expect("the pinned document is JSON")
}

fn pinned_readme() -> String {
    let path = in_force().join("README.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading `{}`: {error}", path.display()))
}

/// The body under one heading, up to the next heading of any level.
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

/// The whole of *What is not pinned* and every subsection under it, so a disclaimer that moved
/// between subsections is still found. The escape is judged on the widest reading available to it.
fn everything_not_pinned(readme: &str) -> String {
    let mut inside = false;
    let mut body: Vec<&str> = Vec::new();
    for line in readme.lines() {
        if line.starts_with("## ") {
            inside = line.trim_start_matches('#').trim() == "What is not pinned";
            continue;
        }
        if inside {
            body.push(line);
        }
    }
    body.join("\n")
}

fn row<'a>(document: &'a Value, command: &str, long: &str) -> Option<&'a Value> {
    document["arguments"][command]
        .as_array()?
        .iter()
        .find(|row| row["long"] == long)
}

/// The command line the document alone says is enough for `command`: every flag it marks required,
/// then every flag the *What is not pinned* section says a run demands anyway.
///
/// `measuring` names the flags this consumer deliberately leaves out because it is measuring what
/// the binary does without them. It is a hole in the invocation and not in the claim: each case
/// that passes one asserts, in its own words, that the flag really is absent before it reads
/// anything off the run.
///
/// It exists because the document now *does* tell a consumer to supply `--session-dir` — which is
/// the finding these cases were written to force, so a consumer that followed the whole document
/// would supply it and there would be nothing left to measure. The cases below are the consumer who
/// reads the row for that flag and wants to know what it means.
fn consumer_invocation(
    document: &Value,
    readme: &str,
    command: &str,
    values: &BTreeMap<&str, String>,
    measuring: &[&str],
) -> Vec<String> {
    let mut argv: Vec<String> = command.split_whitespace().map(str::to_owned).collect();
    let mut wanted: Vec<String> = Vec::new();
    for listed in document["arguments"][command]
        .as_array()
        .unwrap_or_else(|| panic!("`{command}` has a flag list in the pinned document"))
    {
        if listed["required"] == true {
            wanted.push(listed["long"].as_str().expect("a long flag").to_owned());
        }
    }
    let demanded = section(readme, DEMANDED);
    for token in demanded.split('`').skip(1).step_by(2) {
        if token.starts_with("--")
            && row(document, command, token).is_some()
            && !measuring.contains(&token)
            && !wanted.iter().any(|held| held == token)
        {
            wanted.push(token.to_owned());
        }
    }
    for long in wanted {
        argv.push(long.clone());
        if row(document, command, &long).expect("a row")["takes_value"] == true {
            argv.push(
                values
                    .get(long.as_str())
                    .unwrap_or_else(|| {
                        panic!("`{command} {long}` is wanted and this case has no value for it")
                    })
                    .clone(),
            );
        }
    }
    argv
}

struct Output {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

struct Fixture {
    child: Child,
    base_url: String,
}

impl Fixture {
    /// The deterministic local endpoint. Evidence from here is `provider_emulated`
    /// (`AGENTS.md` invariant 18): no provider is contacted by any case in this file.
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

/// `--session-dir` records `"default": null` and this binary writes the session somewhere anyway.
///
/// **Measured, not read off the source.** A command line assembled from the document alone — its
/// required rows, plus the two flags its own *What is not pinned* section says a run demands — is
/// run with `XDG_STATE_HOME` pointed at a directory this case owns and `--session-dir` never typed.
/// The run exits `0` and a session file appears under `<XDG_STATE_HOME>/b10x-harness/sessions`,
/// which is a path no field of the document mentions.
///
/// The document defines `default` as *the value used when the flag is absent*. The flag is absent
/// and a value was used. So the acceptance of `story:argv-pin-carries-effective-defaults` — "for
/// **every** flag whose effective default or requirement is decided after clap, the pinned document
/// either records the effective value or states in its *What is not pinned* section that it does
/// not" — applies to this row exactly as it applies to `--wire`, and the same escape has to be
/// available for it: name the command, the flag and where the value comes from.
///
/// A consumer that read `"default": null` as *nothing happens when it is left out* has a harness
/// that writes a transcript of every run into the operator's state directory, indefinitely, and no
/// field of the document told it so.
#[test]
fn the_session_directory_this_binary_picks_when_the_flag_is_absent_is_recorded_or_disclaimed() {
    let document = pinned_document();
    let readme = pinned_readme();
    let fixture = Fixture::start();
    let config = tempfile::tempdir().expect("a temporary config directory");
    let state = tempfile::tempdir().expect("a temporary state directory");
    let workspace = tempfile::tempdir().expect("a temporary workspace");
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

    let mut argv = consumer_invocation(&document, &readme, "run", &values, &["--session-dir"]);
    argv.push("--workspace".to_owned());
    argv.push(workspace.path().display().to_string());

    assert!(
        !argv.iter().any(|word| word == "--session-dir"),
        "the directory below is the one this binary chose, and the case only measures that while \
         it leaves the flag out: {argv:?}"
    );

    let output = Command::new(BINARY)
        .args(&argv)
        .env("XDG_CONFIG_HOME", config.path())
        .env("XDG_STATE_HOME", state.path())
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");
    let output = Output {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    };
    assert_eq!(
        output.status,
        Some(0),
        "`b10x-harness {}` is everything `{ARGV_CONTRACT_VERSION}` says a run needs, and it did \
         not run.\nstdout: {}\nstderr: {}",
        argv.join(" "),
        output.stdout,
        output.stderr
    );

    let chosen = state.path().join("b10x-harness").join("sessions");
    let written: Vec<PathBuf> = std::fs::read_dir(&chosen)
        .unwrap_or_else(|error| {
            panic!(
                "this case measures the directory this binary picked; `{}`: {error}",
                chosen.display()
            )
        })
        .map(|entry| entry.expect("an entry").path())
        .collect();
    assert_eq!(
        written.len(),
        1,
        "one session was filed in a directory the command line never named: {written:?}"
    );

    let recorded = row(&document, "run", "--session-dir")
        .expect("`run --session-dir` is a row of the document");
    let effective = "$XDG_STATE_HOME/b10x-harness/sessions";
    if recorded["default"] == effective {
        return;
    }
    let escape = everything_not_pinned(&readme);
    assert!(
        escape.contains("`run`")
            && escape.contains("`--session-dir`")
            && escape.contains("b10x-harness/sessions"),
        "`run --session-dir` records `\"default\": {}` and this run filed `{}` with the flag left \
         out. The document defines `default` as the value used when the flag is absent, so it has \
         to record that value or say — in `## What is not pinned`, naming the command, the flag \
         and the directory — that it does not. `### {DEFAULTED}` instead says `--wire` is the only \
         flag of the three that resolves this way.",
        recorded["default"],
        written[0].display()
    );
}

/// `--session-dir` is also a flag this binary demands that clap does not, on a bare machine.
///
/// The document's `### {DEMANDED}` subsection is for exactly this shape: a row that says
/// `"required": false`, truthfully of clap, while the run is refused by name without it. It
/// enumerates `--base-url` and `--model` and stops. `--session-dir` is the third: with neither
/// `XDG_STATE_HOME` nor `HOME` set, `session_dir()` has no directory to invent and the run is
/// refused before the first request — the same exit `1`, from the same command line the document
/// says is enough.
///
/// **No endpoint is contacted.** The refusal happens after `require_endpoint_and_model` and before
/// the first request, so the `--base-url` this case supplies is never dialled; the case is
/// deterministic and needs no fixture.
///
/// This is not a hypothetical environment. A container, a systemd unit with `PrivateUsers`, a CI
/// step that clears the environment, and `env -i` all reach it, and the document's own audience is
/// a driver launching this binary rather than a person in a login shell.
#[test]
fn a_bare_environment_makes_session_dir_a_flag_the_run_demands_and_the_document_says_so() {
    let document = pinned_document();
    let readme = pinned_readme();
    let config = tempfile::tempdir().expect("a temporary config directory");
    let workspace = tempfile::tempdir().expect("a temporary workspace");

    let mut values = BTreeMap::new();
    values.insert("--input", "say hello".to_owned());
    values.insert("--base-url", "http://127.0.0.1:1/v1".to_owned());
    values.insert("--model", "b10x-emulated".to_owned());

    let mut argv = consumer_invocation(&document, &readme, "run", &values, &["--session-dir"]);
    argv.push("--workspace".to_owned());
    argv.push(workspace.path().display().to_string());

    let output = Command::new(BINARY)
        .args(&argv)
        .env_clear()
        .env("XDG_CONFIG_HOME", config.path())
        .env(
            "PATH",
            std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into()),
        )
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");
    let output = Output {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    };

    assert_eq!(
        output.status,
        Some(1),
        "`b10x-harness {}` on a machine with no `HOME` and no `XDG_STATE_HOME` is refused by name \
         rather than parsed away or panicked on.\nstdout: {}\nstderr: {}",
        argv.join(" "),
        output.stdout,
        output.stderr
    );
    assert!(
        output
            .stderr
            .contains("state directory to keep sessions in"),
        "the refusal this case is about is the session directory's, and it did not \
         fire.\nstdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );

    let recorded = row(&document, "run", "--session-dir")
        .expect("`run --session-dir` is a row of the document");
    assert_eq!(
        recorded["required"], false,
        "this case is about a row that says `\"required\": false`; it no longer does"
    );

    let escape = everything_not_pinned(&readme);
    assert!(
        escape.contains("`run`") && escape.contains("`--session-dir`"),
        "`run --session-dir` records `\"required\": false` and this binary refused the run by name \
         without it. That is the shape `### {DEMANDED}` exists for, and it names `--base-url` and \
         `--model` only. A consumer building an invocation from the document alone gets a command \
         line that does not run.\nstderr: {}",
        output.stderr
    );
}

/// The smallest workflow document that reaches a turn, written by the case rather than found.
const ONE_STEP: &str = "\
id: one-step
root:
  id: root
  nodes:
    - id: shape
      nodes:
        - id: specify
          run:
            state: specify
            summary: \"State the required behaviour.\"
";

/// The sentence the unit added says `workflow run` is refused by name. It is not; it panics.
///
/// `contracts/cli/b10x-harness/2026-08-30.2/README.md:243-250` is the escape story 1 was closed
/// with, and it is scoped to three commands by name:
///
/// > `run`, `chat` and `workflow run` record `--base-url` and `--model` as `"required": false`.
/// > That is true of clap … and it is not true of the run. … a run that has neither the two flags
/// > nor a configured provider is **refused by name before the first request**.
///
/// This case is the third command in that list. The invocation is built from the document alone —
/// `workflow run`'s two `"required": true` rows, `--flow` and `--input`, and nothing else — and
/// with no provider configured it does not reach a refusal. `workflow::dispatch` never calls
/// `apply_profiles`, so `RunOptions::base_url`'s `expect` fires and the process aborts with **exit
/// `101`**, which the same document's *What is not pinned* section
/// (`README.md:230-232`) says is not one of this binary's statuses: "`0` answered, `2` stopped for
/// a named reason, `1` could not run".
///
/// So the acceptance was unmet on the command the escape named third, and unmet in the worst
/// available way: a consumer that followed the document got a status the document says cannot
/// happen, out of a panic handler, with a stack trace on stderr instead of a sentence naming the
/// flag to type. The unit knew — its own case said so in a doc comment — and asserted the document
/// instead of running the command.
///
/// # What this case decides now, and what it gave up
///
/// It was written asserting that the run **is** refused by name, which is one of the two ways to
/// remove the defect: make the sentence true. The other is to stop making the claim, and that is
/// the one taken — the binary's half is `story:workflow-run-panics-and-drops-its-profile` and is
/// not this unit's to fix, so a case that only accepted the first was over-specified against the
/// defect it names in its own title.
///
/// So it is now decided **both ways round**: the command is run, and the escape section names it
/// if and only if it really is refused by name. Removing the sentence passes; putting it back
/// while the binary still panics fails; and when the binary is fixed, a document that then says
/// nothing about `workflow run` fails too, so the correction cannot be landed and forgotten. The
/// exit status the document forbids is still measured and still printed — it moved from an
/// assertion this unit may not satisfy into the evidence of one it can.
///
/// **No endpoint is contacted.** The panic precedes the first request, so this case needs no
/// fixture and is deterministic.
#[test]
fn workflow_run_built_from_the_document_alone_is_refused_by_name_as_the_document_says() {
    let document = pinned_document();
    let readme = pinned_readme();
    let config = tempfile::tempdir().expect("a temporary config directory");
    let state = tempfile::tempdir().expect("a temporary state directory");
    let workspace = tempfile::tempdir().expect("a temporary workspace");
    let flow = workspace.path().join("one-step.yaml");
    std::fs::write(&flow, ONE_STEP).expect("a flow document");

    let mut values = BTreeMap::new();
    values.insert("--input", "do the thing".to_owned());
    values.insert("--flow", flow.display().to_string());

    // The document's `"required": true` rows for `workflow run`, and nothing beyond them: the
    // consumer this models has not yet read the escape, which is the state the escape is written
    // for.
    let mut argv: Vec<String> = vec!["workflow".to_owned(), "run".to_owned()];
    for listed in document["arguments"]["workflow run"]
        .as_array()
        .expect("`workflow run` has a flag list")
    {
        if listed["required"] != true {
            continue;
        }
        let long = listed["long"].as_str().expect("a long flag");
        argv.push(long.to_owned());
        argv.push(values[long].clone());
    }
    argv.push("--workspace".to_owned());
    argv.push(workspace.path().display().to_string());

    let output = Command::new(BINARY)
        .args(&argv)
        .env("XDG_CONFIG_HOME", config.path())
        .env("XDG_STATE_HOME", state.path())
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");
    let output = Output {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    };

    // Measured before the document is opened: exit `1`, naming the flags to type, is a refusal by
    // name. Anything else is not, whatever it is — and what it is today is exit `101` out of a
    // panic handler, which `## What is not pinned` says this binary does not produce.
    let refused_by_name = output.status == Some(1)
        && ["--base-url", "--model"]
            .iter()
            .all(|long| output.stderr.contains(long));

    let demanded = section(&readme, DEMANDED);
    let named = demanded.contains("`workflow run`");
    assert_eq!(
        named,
        refused_by_name,
        "`### {DEMANDED}` names `workflow run`: {named}. A command line built from \
         `{ARGV_CONTRACT_VERSION}` alone is refused by name on it: {refused_by_name}. Those have to \
         agree — a pinned document may not promise a refusal the binary does not give, and may not \
         stay silent about one it does.\n`b10x-harness {}` exited `{:?}`.\nstdout: {}\nstderr: {}",
        argv.join(" "),
        output.status,
        output.stdout,
        output.stderr
    );
}
