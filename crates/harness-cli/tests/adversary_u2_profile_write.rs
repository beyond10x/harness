//! Adversary pass on `story:daemon-exec-start-adoption`, through the shipped binary.
//!
//! `--substrate-adopt` conflicts with `--substrate-embedded` at the parser. A profile carrying
//! `write = true` sets `substrate_embedded` after the parser has run (`apply_profiles`), and
//! `published()` takes the embedded branch first. So a command line naming a socket and a root name
//! to adopt can be turned, by a config file, into a run that writes `--workspace` on this host
//! through the in-process driver, never asks the daemon anything and adopts nothing.
//!
//! The model endpoint is a closed port: every case here must be decided before the first turn.

use std::fs;

use std::process::Command;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

struct Ran {
    status: Option<i32>,
    stderr: String,
}

/// A `run` in a fresh directory holding `project/`, with `config` as the user's harness.toml (or
/// none), the given extra flags, and a model endpoint nobody listens on.
fn run_with(config: Option<&str>, extra: &[&str]) -> Ran {
    let scratch = tempfile::Builder::new()
        .prefix("adv-u2-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .expect("scratch");
    let dir = scratch.path();
    fs::create_dir(dir.join("project")).expect("project");
    fs::write(dir.join("project").join("README.md"), "hello\n").expect("readme");
    let xdg = dir.join("xdg");
    fs::create_dir_all(xdg.join("b10x")).expect("config dir");
    if let Some(text) = config {
        fs::write(xdg.join("b10x").join("harness.toml"), text).expect("config");
    }
    let output = Command::new(BINARY)
        .args([
            "run",
            "--base-url",
            "http://127.0.0.1:9/v1",
            "--model",
            "m",
            "--api-key-env",
            "B10X_ADV_U2_KEY",
            "--no-session",
            "--input",
            "write a file",
            "--workspace",
            "project",
        ])
        .args(extra)
        .current_dir(dir)
        .env("XDG_CONFIG_HOME", &xdg)
        .env("B10X_ADV_U2_KEY", "not-a-real-key")
        .output()
        .expect("the binary runs");
    Ran {
        status: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn last_error(stderr: &str) -> &str {
    stderr
        .lines()
        .rev()
        .find(|line| line.starts_with("error:"))
        .unwrap_or("")
}

/// The socket and the root name the operator typed are what the run is decided by: it refuses at
/// the socket (or refuses the combination), and no model turn starts.
fn assert_decided_at_the_socket(ran: &Ran, case: &str) {
    assert_ne!(ran.status, Some(0), "{case}: {}", ran.stderr);
    assert!(
        !ran.stderr.contains("· turn 1"),
        "{case}: a model turn started, so the run was not refused at the socket it was given:\n{}",
        ran.stderr
    );
    assert!(
        last_error(&ran.stderr).contains("substrate"),
        "{case}: the refusal names neither the socket nor the adopt:\n{}",
        ran.stderr
    );
}

const WRITE_PROFILE: &str = "[default]\nwrite = true\n";

#[test]
fn control_without_a_profile_an_adopt_over_an_absent_socket_is_refused_at_the_socket() {
    let ran = run_with(
        None,
        &["--substrate", "absent.sock", "--substrate-adopt", "project"],
    );
    assert_decided_at_the_socket(&ran, "no profile");
}

#[test]
fn a_write_profile_does_not_turn_a_typed_substrate_adopt_into_the_embedded_driver() {
    let ran = run_with(
        Some(WRITE_PROFILE),
        &["--substrate", "absent.sock", "--substrate-adopt", "project"],
    );
    assert_decided_at_the_socket(&ran, "`[default] write = true` beside --substrate-adopt");
}

#[test]
fn a_write_profile_does_not_turn_a_typed_substrate_socket_into_the_embedded_driver() {
    let ran = run_with(Some(WRITE_PROFILE), &["--substrate", "absent.sock"]);
    assert_decided_at_the_socket(&ran, "`[default] write = true` beside --substrate");
}
