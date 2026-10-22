//! `story:harness-events-is-retired` and `story:workflow-verb-is-removed`, through the shipped
//! binary.
//!
//! Both acceptances open with the same observable: clap refuses the verb as an **unknown
//! subcommand**. Not a verb that still parses and then fails, and not one hidden from `--help` and
//! still served — a peer that typed it gets clap's own refusal and the exit status every refused
//! command line gets on this binary (`1`; `2` means a run that stopped for a named reason).

use std::process::{Command, Stdio};

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

struct Output {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

fn invoke(arguments: &[&str]) -> Output {
    let output = Command::new(BINARY)
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");
    Output {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn assert_unknown_subcommand(arguments: &[&str]) {
    let verb = arguments[0];
    let output = invoke(arguments);
    assert_eq!(
        output.status,
        Some(1),
        "`{}` must be refused by clap; stdout: {} stderr: {}",
        arguments.join(" "),
        output.stdout,
        output.stderr
    );
    assert!(
        output
            .stderr
            .contains(&format!("unrecognized subcommand '{verb}'")),
        "`{}` must be refused as an unknown subcommand, not parsed: {}",
        arguments.join(" "),
        output.stderr
    );
}

#[test]
fn events_is_an_unknown_subcommand() {
    assert_unknown_subcommand(&["events"]);
    assert_unknown_subcommand(&["events", "--in", "record.jsonl", "--out", "events.jsonl"]);
}

#[test]
fn workflow_is_an_unknown_subcommand() {
    assert_unknown_subcommand(&["workflow"]);
    assert_unknown_subcommand(&["workflow", "plan", "flow.yaml"]);
    assert_unknown_subcommand(&["workflow", "run", "flow.yaml"]);
}

/// A refused command line under `--json` still leaves its one terminal line, so a driver that
/// still launches either verb sees a record saying why, not silence.
#[test]
fn a_retired_verb_under_json_leaves_the_refused_line() {
    for verb in ["events", "workflow"] {
        let output = invoke(&[verb, "--json"]);
        assert_eq!(output.status, Some(1), "{verb}: {}", output.stderr);
        let line: serde_json::Value =
            serde_json::from_str(output.stdout.trim()).expect("one JSON line on stdout");
        assert_eq!(line["kind"], serde_json::json!("refused"), "{verb}");
        assert!(
            line["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains(&format!("unrecognized subcommand '{verb}'"))),
            "{verb}: {line}"
        );
    }
}

#[test]
fn the_help_page_names_neither_verb() {
    let output = invoke(&["--help"]);
    assert_eq!(output.status, Some(0), "{}", output.stderr);
    for line in output.stdout.lines() {
        let first = line.split_whitespace().next().unwrap_or("");
        assert!(
            first != "events" && first != "workflow",
            "`--help` still lists a retired verb: {line}"
        );
    }
}
