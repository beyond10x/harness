//! Security review case for `story:api-key-from-the-secrets-store`: without `--api-key-secret`
//! the binary never opens the platform's secrets store.
//!
//! On Linux the store is Secret Service over the D-Bus session bus, and `zbus` finds that bus
//! through `DBUS_SESSION_BUS_ADDRESS`, else `$XDG_RUNTIME_DIR/bus` (zbus 5.19
//! `src/address/mod.rs:64-76`). Both point at a socket this test listens on, with the rest of the
//! environment cleared, so the real bus is unreachable and any connection attempt is observed.
//! No case passes a reference that parses (the review brief forbids it).

#![cfg(target_os = "linux")]

use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

fn connections(listener: &UnixListener) -> usize {
    let mut count = 0;
    while listener.accept().is_ok() {
        count += 1;
    }
    count
}

/// The binary's environment: cleared, with every directory it may read or write a fresh temporary
/// one and both of zbus's ways to the session bus pointed at `bus`.
struct Sandbox {
    runtime: tempfile::TempDir,
    state: tempfile::TempDir,
    config: tempfile::TempDir,
    bus: std::path::PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let runtime = tempfile::tempdir().expect("a runtime directory");
        let bus = runtime.path().join("bus");
        Self {
            runtime,
            state: tempfile::tempdir().expect("a state directory"),
            config: tempfile::tempdir().expect("a config directory"),
            bus,
        }
    }

    fn run(&self, arguments: &[&str], variables: &[(&str, &str)]) -> String {
        let mut command = Command::new(BINARY);
        command
            .args(arguments)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.config.path())
            .env("XDG_CONFIG_HOME", self.config.path())
            .env("XDG_STATE_HOME", self.state.path())
            .env("XDG_RUNTIME_DIR", self.runtime.path())
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}", self.bus.display()),
            )
            .stdin(Stdio::null());
        for (name, value) in variables {
            command.env(name, value);
        }
        let output = command.output().expect("the binary runs");
        format!(
            "status {:?}\nstdout: {}\nstderr: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }
}

#[test]
fn a_run_or_server_without_the_flag_never_dials_the_session_bus() {
    let sandbox = Sandbox::new();
    let workspace = tempfile::tempdir().expect("a workspace");
    let listener = UnixListener::bind(&sandbox.bus).expect("a fixture bus socket");
    listener.set_nonblocking(true).expect("non-blocking");
    let run = |arguments: &[&str], variables: &[(&str, &str)]| sandbox.run(arguments, variables);
    let workspace = workspace.path().display().to_string();
    let base = [
        "run",
        "--base-url",
        "http://127.0.0.1:1/v1",
        "--model",
        "m",
        "--input",
        "hi",
        "--workspace",
        &workspace,
    ];

    for (what, extra, variables) in [
        ("no credential", &[][..], &[][..]),
        (
            "an API key from the environment",
            &["--api-key-env", "REVIEW_FIXTURE_KEY"][..],
            &[("REVIEW_FIXTURE_KEY", "synthetic-review-env-key")][..],
        ),
    ] {
        let arguments: Vec<&str> = base.iter().copied().chain(extra.iter().copied()).collect();
        let output = run(&arguments, variables);
        assert!(!output.contains("panicked"), "{what}: {output}");
        assert_eq!(
            connections(&listener),
            0,
            "{what}: the run dialled the session bus without `--api-key-secret`\n{output}"
        );
        eprintln!("{what}: {output}");
    }

    let output = run(
        &[
            "app-server",
            "--base-url",
            "http://127.0.0.1:1/v1",
            "--model",
            "m",
        ],
        &[],
    );
    assert_eq!(
        connections(&listener),
        0,
        "app-server dialled the session bus without `--api-key-secret`\n{output}"
    );
    eprintln!("app-server: {output}");

    // The app-server reads the flag too: a malformed reference refuses the server by name rather
    // than starting it with no credential (a dropped branch would exit 0 on stdin's end).
    let output = run(
        &[
            "app-server",
            "--base-url",
            "http://127.0.0.1:1/v1",
            "--model",
            "m",
            "--api-key-secret",
            "fixture-api-key",
        ],
        &[],
    );
    assert!(!output.starts_with("status Some(0)"), "{output}");
    assert!(output.contains("`fixture-api-key`"), "{output}");
    assert_eq!(connections(&listener), 0, "{output}");

    // A malformed reference is refused before the store is opened.
    let mut arguments = base.to_vec();
    arguments.extend(["--api-key-secret", "fixture-api-key"]);
    let output = run(&arguments, &[]);
    assert!(output.contains("`fixture-api-key`"), "{output}");
    assert_eq!(
        connections(&listener),
        0,
        "a malformed reference dialled the session bus\n{output}"
    );
}
