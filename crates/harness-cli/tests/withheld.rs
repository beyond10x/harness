//! What the shipped binary says about a tool it was asked for and could not have.
//!
//! The defect this pins was found with `b10x-harness tools` and nothing else: from a login shell
//! the command answers six entries, and under `systemd-run --user --scope` the same command against
//! the same machine answers seven. Neither answer said that the difference was `run`, or why, so a
//! run whose only legal route was starting a program was indistinguishable from a run that never
//! wanted one.
//!
//! This runs the embedded driver, which probes **this** process's cgroup. A cargo test runs in the
//! session scope of whoever started it, which is outside any delegated root, so the exec facts are
//! absent here by construction — which is exactly the machine the defect was found on.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

struct Output {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run(arguments: &[&str], workspace: &Path) -> Output {
    let output = Command::new(BINARY)
        .args(arguments)
        .arg("--workspace")
        .arg(workspace)
        .output()
        .expect("the binary runs");
    Output {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// A tree substrate's embedded driver can adopt: the named directory, under a root it can own.
///
/// The root has to outlive the call — `--workspace`'s parent becomes substrate's root — so both
/// come back.
fn adoptable_workspace(name: &str) -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().expect("a temporary directory");
    let workspace = root.path().join(name);
    fs::create_dir(&workspace).expect("create the workspace directory");
    (root, workspace)
}

#[test]
fn tools_states_the_program_it_was_declared_and_could_not_admit() {
    let (_root, workspace) = adoptable_workspace("ws_withheld");
    let output = run(
        &[
            "tools",
            "--substrate-embedded",
            "--allow-program",
            "/bin/echo",
        ],
        &workspace,
    );
    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);

    let described: Value = serde_json::from_str(&output.stdout).expect("valid JSON");
    let entries: Vec<&str> = described["catalogue"]["tools"]
        .as_array()
        .expect("a catalogue")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    // The gate still works by absence, and that has not changed: the model is told about no tool
    // it cannot have.
    assert!(!entries.contains(&"run"), "{entries:?}");

    // What has changed is that the document now says so.
    let withheld = described["withheld"].as_array().expect("a withheld list");
    assert_eq!(withheld.len(), 1, "{withheld:?}");
    assert_eq!(withheld[0]["tool"], "run");
    let reason = withheld[0]["reason"].as_str().expect("a reason");
    assert!(
        reason.starts_with("`exec.argv-only` must be true and this machine says nothing."),
        "the predicate that failed, as the machine stated it: {reason}"
    );
    assert!(
        reason.contains("systemd-run --user --scope"),
        "and where to look, because the fault is in how this was started: {reason}"
    );

    // And says it on stderr too, in the same words the run's own renderer uses, so a person
    // reading a screen of JSON cannot miss it and stdout stays parseable.
    assert!(
        output
            .stderr
            .contains("note: `run` is not published on this machine:"),
        "stderr: {}",
        output.stderr
    );
}

#[test]
fn tools_over_a_machine_asked_for_nothing_it_lacks_withholds_nothing() {
    // Absence stays absence: no program was declared, so nothing was refused, and a `withheld`
    // entry here would put a line about `run` in front of every read-only run there has ever been.
    let (_root, workspace) = adoptable_workspace("ws_quiet");
    let output = run(&["tools", "--substrate-embedded"], &workspace);
    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);

    let described: Value = serde_json::from_str(&output.stdout).expect("valid JSON");
    assert_eq!(
        described["withheld"],
        serde_json::json!([]),
        "stated empty rather than omitted: this command's job is to say what a machine is"
    );
    assert!(!output.stderr.contains("note:"), "{}", output.stderr);
}

#[test]
fn a_declared_program_with_no_confinement_at_all_is_reported_rather_than_dropped() {
    // No `--substrate` and no `--substrate-embedded`: the run is read-only, which is a legitimate
    // way to run. But commands *were* named, and nothing here can start one — the same silence,
    // reached from the other side.
    let workspace = tempfile::tempdir().expect("a temporary directory");
    let output = run(&["tools", "--allow-program", "/bin/echo"], workspace.path());
    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);

    let described: Value = serde_json::from_str(&output.stdout).expect("valid JSON");
    let withheld = described["withheld"].as_array().expect("a withheld list");
    assert_eq!(withheld.len(), 1, "{withheld:?}");
    assert_eq!(withheld[0]["tool"], "run");
    assert!(
        withheld[0]["reason"]
            .as_str()
            .expect("a reason")
            .starts_with("this machine states no capability facts at all"),
        "{withheld:?}"
    );
    // No workspace was asked for, so none is reported missing.
    assert!(
        !output.stdout.contains("file_write"),
        "stdout: {}",
        output.stdout
    );
}
