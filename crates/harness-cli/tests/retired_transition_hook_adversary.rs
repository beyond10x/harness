//! Adversary cases for `story:workflow-verb-is-removed`: the `transition` hook point went with the
//! walk, and a hooks file naming it must be refused **by name**, through the shipped binary, on
//! every command that takes `--hooks` (`run` and `chat`, per the in-force argv contract), before a
//! single connection is attempted — and a file that also names a point that still exists must not
//! be half-loaded.
//!
//! The unit's own case (`hooks.rs`, `transition_is_no_longer_a_hook_point`) drives `Hooks::parse`
//! directly and only for a file whose sole entry is `transition`. These drive the binary, and count
//! connections on a socket this test owns, so "before anything is sent" is measured rather than
//! assumed.

use std::io::ErrorKind;
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

/// A sandbox: a home, every XDG directory and a workspace, so no profile or state of the machine
/// running the test reaches the run.
struct Sandbox {
    root: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("a temporary directory");
        for directory in ["home", "config", "state", "workspace"] {
            std::fs::create_dir(root.path().join(directory)).expect("a sandbox directory");
        }
        std::fs::write(root.path().join("workspace/README.md"), "hello harness\n")
            .expect("a readable file");
        Self { root }
    }

    fn path(&self, relative: &str) -> std::path::PathBuf {
        self.root.path().join(relative)
    }
}

struct Output {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Runs `command` (`run` or `chat`) under `--json --hooks <hooks>` against `base_url`.
fn launch(sandbox: &Sandbox, command: &str, base_url: &str, hooks: &Path) -> Output {
    let mut argv: Vec<String> = vec![
        command.to_owned(),
        "--json".to_owned(),
        "--base-url".to_owned(),
        base_url.to_owned(),
        "--model".to_owned(),
        "b10x-emulated".to_owned(),
        "--api-key-env".to_owned(),
        "B10X_ADVERSARY_KEY".to_owned(),
        "--no-session".to_owned(),
        "--workspace".to_owned(),
        sandbox.path("workspace").display().to_string(),
        "--hooks".to_owned(),
        hooks.display().to_string(),
    ];
    if command == "run" {
        argv.extend(["--input".to_owned(), "read the readme".to_owned()]);
    }
    let mut child = Command::new(BINARY)
        .args(&argv)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", sandbox.path("home"))
        .env("XDG_CONFIG_HOME", sandbox.path("config"))
        .env("XDG_STATE_HOME", sandbox.path("state"))
        .env("B10X_ADVERSARY_KEY", "test-key")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary starts");
    {
        use std::io::Write as _;
        let mut stdin = child.stdin.take().expect("a stdin pipe");
        // `chat` reads its question here; `run` ignores it. A closed pipe after one line ends the
        // conversation either way, so the case cannot hang on a refusal that did not happen.
        let _ = stdin.write_all(b"read the readme\n");
    }
    let output = child.wait_with_output().expect("the binary finishes");
    Output {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// How many connections reached `listener` while nobody accepted them.
fn connections(listener: &TcpListener) -> usize {
    listener.set_nonblocking(true).expect("non-blocking");
    let mut count = 0;
    loop {
        match listener.accept() {
            Ok(_) => count += 1,
            Err(error) if error.kind() == ErrorKind::WouldBlock => return count,
            Err(error) => panic!("accept failed: {error}"),
        }
    }
}

fn refusal(output: &Output) -> String {
    let line: Value = serde_json::from_str(output.stdout.trim()).unwrap_or_else(|error| {
        panic!(
            "one `refused` line on stdout ({error}); status {:?}\nstdout: {}\nstderr: {}",
            output.status, output.stdout, output.stderr
        )
    });
    assert_eq!(line["kind"], "refused", "{line}");
    line["reason"].as_str().expect("a reason").to_owned()
}

#[test]
fn a_hooks_file_naming_transition_is_refused_by_name_on_run_and_chat_before_any_connection() {
    let sandbox = Sandbox::new();
    let hooks = sandbox.path("hooks.json");
    // A file written for 0.17: a loop guard first, then the governor the walk used to ask. The
    // guard is valid; the second entry is what this release retired.
    std::fs::write(
        &hooks,
        serde_json::json!({"version": 1, "hooks": [
            {"on": "before-call", "tools": ["file_write"], "command": ["/bin/true"]},
            {"on": "transition", "command": ["/bin/true"]},
        ]})
        .to_string(),
    )
    .expect("write the hooks file");

    for command in ["run", "chat"] {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let base_url = format!("http://{}/v1", listener.local_addr().expect("bound"));
        let output = launch(&sandbox, command, &base_url, &hooks);

        assert_eq!(
            output.status,
            Some(1),
            "`{command}` with a hooks file naming `transition` is a refusal\nstdout: {}\nstderr: {}",
            output.stdout,
            output.stderr
        );
        let reason = refusal(&output);
        assert!(
            reason.contains("hook 2 fires `transition`"),
            "`{command}`: refused by the point's own name and its position: {reason}"
        );
        assert!(reason.contains("not a hook point"), "`{command}`: {reason}");
        assert!(
            reason.contains("hooks.json"),
            "`{command}`: the refusal names the file the operator passed: {reason}"
        );
        assert_eq!(
            connections(&listener),
            0,
            "`{command}`: a run whose hooks file was refused contacted the endpoint anyway"
        );
    }
}

#[test]
fn the_three_points_that_remain_still_load_on_run_and_chat() {
    // The positive twin: the same file without the retired entry is not refused, on either
    // command. What it reaches is the endpoint, which here accepts the connection and never
    // answers it — so the measurement is "a connection was attempted", not what the run printed.
    let sandbox = Sandbox::new();
    let hooks = sandbox.path("hooks.json");
    std::fs::write(
        &hooks,
        serde_json::json!({"version": 1, "hooks": [
            {"on": "before-call", "tools": ["file_write"], "command": ["/bin/true"]},
            {"on": "after-call", "command": ["/bin/true"]},
            {"on": "stop", "command": ["/bin/true"]},
        ]})
        .to_string(),
    )
    .expect("write the hooks file");

    for command in ["run", "chat"] {
        // A port nobody listens on: the run gets past its hooks and fails on the wire, which is a
        // different refusal from the one above and never names the hooks file.
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let base_url = format!("http://{}/v1", listener.local_addr().expect("bound"));
        drop(listener);
        let output = launch(&sandbox, command, &base_url, &hooks);
        let all = format!("{}\n{}", output.stdout, output.stderr);
        assert!(
            !all.contains("not a hook point") && !all.contains("the hooks file"),
            "`{command}`: a file naming only the three remaining points was refused\n{all}"
        );
    }
}
