//! Adversary pass 2 on `story:daemon-exec-start-adoption`, through the shipped binary.
//!
//! The fix (ea12a46) stops a profile's `write = true` from setting the embedded driver under a
//! typed `--substrate`, and lets the profile's other write keys fill as if typed. These drive the
//! other ways a profile reaches a socket run: a named `-p` profile, `chat`, and a profile carrying
//! `toolchains`, which under a socket now refuses the run.
//!
//! The model endpoint is a closed port: every case here is decided before the first turn.

use std::fs;
use std::process::Command;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

struct Ran {
    status: Option<i32>,
    stderr: String,
}

/// `subcommand` in a fresh directory holding a Rust `project/`, with `config` as harness.toml.
fn launch(subcommand: &str, config: &str, extra: &[&str]) -> Ran {
    let scratch = tempfile::Builder::new()
        .prefix("adv-u2-p2-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .expect("scratch");
    let dir = scratch.path();
    let project = dir.join("project");
    fs::create_dir(&project).expect("project");
    fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"p\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("manifest");
    let xdg = dir.join("xdg");
    fs::create_dir_all(xdg.join("b10x")).expect("config dir");
    fs::write(xdg.join("b10x").join("harness.toml"), config).expect("config");
    let mut command = Command::new(BINARY);
    command.args([
        subcommand,
        "--base-url",
        "http://127.0.0.1:9/v1",
        "--model",
        "m",
        "--api-key-env",
        "B10X_ADV_U2_KEY",
        "--no-session",
        "--workspace",
        "project",
    ]);
    if subcommand == "run" {
        command.args(["--input", "write a file"]);
    }
    let output = command
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

fn assert_refused_at_the_socket(ran: &Ran, case: &str) {
    assert_ne!(ran.status, Some(0), "{case}: {}", ran.stderr);
    assert!(
        !ran.stderr.contains("· turn 1") && !ran.stderr.contains("file_write"),
        "{case}: write tools were published or a turn started:\n{}",
        ran.stderr
    );
    assert!(
        last_error(&ran.stderr).contains("no usable substrate daemon at `absent.sock`"),
        "{case}: not refused at the socket that was typed:\n{}",
        ran.stderr
    );
}

const NAMED_WRITE: &str = "[default]\nwrite = false\n[[profiles]]\nname = \"w\"\nwrite = true\n";

#[test]
fn a_named_write_profile_does_not_turn_an_adopt_into_the_embedded_driver() {
    let ran = launch(
        "run",
        NAMED_WRITE,
        &[
            "-p",
            "w",
            "--substrate",
            "absent.sock",
            "--substrate-adopt",
            "project",
        ],
    );
    assert_refused_at_the_socket(&ran, "run -p w");
}

#[test]
fn chat_with_a_write_profile_is_decided_at_the_typed_socket() {
    let ran = launch(
        "chat",
        "[default]\nwrite = true\n",
        &["--substrate", "absent.sock", "--substrate-adopt", "project"],
    );
    assert_refused_at_the_socket(&ran, "chat with [default] write = true");
}

/// The operator typed no `--toolchain`. A refusal that names only `--toolchain` sends them looking
/// for a flag that is not on their command line; the toolchain came from the profile, and the
/// refusal has to say so (the profile key or the file), or nobody can act on it.
#[test]
fn a_profiles_toolchains_under_a_typed_socket_are_refused_naming_where_they_came_from() {
    let ran = launch(
        "run",
        "[default]\nwrite = true\ntoolchains = [\"rust\"]\n",
        &["--substrate", "absent.sock"],
    );
    assert_ne!(ran.status, Some(0), "{}", ran.stderr);
    let refusal = last_error(&ran.stderr);
    assert!(
        refusal.contains("toolchains") || refusal.contains("harness.toml"),
        "the refusal does not name the profile the toolchain came from: {refusal}"
    );
}
