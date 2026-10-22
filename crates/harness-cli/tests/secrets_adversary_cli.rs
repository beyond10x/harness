//! Adversary cases for `--api-key-secret` at the command line, against the built binary.
//!
//! **No reference used here parses.** A reference that parsed would make the binary open the
//! platform's native store, which no test may do. Each case either stops in clap or uses a
//! reference the secrets grammar refuses (`LLM/…`: an upper-case namespace) before any store is
//! opened. Belt and braces, every invocation also points the D-Bus session bus, `HOME` and every
//! XDG directory into a temporary directory, so even a regression that opened the store could not
//! reach the operator's real one.

use std::net::TcpListener;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use b10x_harness_cli::contract::ARGV_CONTRACT_VERSION;
use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

/// A reference the grammar refuses: `LLM` is not a lower-case namespace.
const UNPARSED: &str = "LLM/fixture-api-key";
/// A variable no case sets, named by the environment-reading credential flags.
const UNSET_VARIABLE: &str = "B10X_ADVERSARY_UNSET";

/// The three commands the in-force document gives `--api-key-secret`.
const COMMANDS: [&str; 3] = ["app-server", "chat", "run"];

fn document() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/cli/b10x-harness")
        .join(ARGV_CONTRACT_VERSION)
        .join("argv.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading `{}`: {error}", path.display()));
    serde_json::from_str(&text).expect("the pinned document is JSON")
}

fn row<'d>(document: &'d Value, command: &str, long: &str) -> Option<&'d Value> {
    document["arguments"][command]
        .as_array()
        .unwrap_or_else(|| panic!("`{command}` has a flag list"))
        .iter()
        .find(|row| row["long"] == long)
}

fn conflicts(row: &Value) -> Vec<String> {
    row["conflicts_with"]
        .as_array()
        .expect("a conflict list")
        .iter()
        .map(|long| long.as_str().expect("a flag").to_owned())
        .collect()
}

/// A sandbox: a home, every XDG directory, a workspace, and a bus address nobody serves.
struct Sandbox {
    root: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let root = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("a sandbox");
        for directory in ["home", "config", "state", "runtime", "workspace"] {
            std::fs::create_dir(root.path().join(directory)).expect("a sandbox directory");
        }
        Self { root }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn bus(&self) -> PathBuf {
        self.path("runtime/bus")
    }

    /// The flags `command` requires, with values, and `--base-url` at `base_url`.
    fn required(&self, command: &str, base_url: &str) -> Vec<String> {
        let mut argv: Vec<String> = command.split_whitespace().map(str::to_owned).collect();
        argv.extend(["--base-url".to_owned(), base_url.to_owned()]);
        argv.extend(["--model".to_owned(), "m".to_owned()]);
        if command == "run" {
            argv.extend(["--input".to_owned(), "say hello".to_owned()]);
        }
        if command != "app-server" {
            argv.extend([
                "--workspace".to_owned(),
                self.path("workspace").display().to_string(),
            ]);
        }
        argv
    }

    fn run(&self, argv: &[String]) -> Output {
        Command::new(BINARY)
            .args(argv)
            .current_dir(self.path("workspace"))
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("XDG_STATE_HOME", self.path("state"))
            .env("XDG_RUNTIME_DIR", self.path("runtime"))
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}", self.bus().display()),
            )
            .stdin(Stdio::null())
            .output()
            .expect("the binary runs")
    }
}

/// A loopback port nothing listens on any more: a request there is refused at once.
fn closed_port() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let address = listener.local_addr().expect("bound");
    drop(listener);
    format!("http://{address}/v1")
}

fn value_for(long: &str, sandbox: &Sandbox) -> String {
    match long {
        "--api-key-file" | "--oauth-token-file" => {
            sandbox.path("home/absent-credential").display().to_string()
        }
        "--api-key-env" | "--oauth-token-env" => UNSET_VARIABLE.to_owned(),
        other => panic!("no fixture value for `{other}`"),
    }
}

#[test]
fn every_conflict_the_document_pins_for_the_secret_flag_is_one_clap_refuses_both_ways_round() {
    let document = document();
    let sandbox = Sandbox::new();
    let base_url = closed_port();
    let mut wrong: Vec<String> = Vec::new();
    for command in COMMANDS {
        let secret = row(&document, command, "--api-key-secret")
            .unwrap_or_else(|| panic!("`{command}` carries `--api-key-secret` in the document"));
        let pinned = conflicts(secret);
        assert_eq!(
            pinned,
            [
                "--api-key-env",
                "--api-key-file",
                "--oauth-token-env",
                "--oauth-token-file"
            ],
            "`{command}`: the secret flag conflicts with every other credential source"
        );
        for other in &pinned {
            let theirs = conflicts(
                row(&document, command, other)
                    .unwrap_or_else(|| panic!("`{command}` carries `{other}`")),
            );
            if !theirs.iter().any(|long| long == "--api-key-secret") {
                wrong.push(format!(
                    "`{command}`: the document's `{other}` row does not list `--api-key-secret`"
                ));
            }
            for secret_first in [true, false] {
                let mut argv = sandbox.required(command, &base_url);
                let pair = [
                    ["--api-key-secret".to_owned(), UNPARSED.to_owned()],
                    [other.clone(), value_for(other, &sandbox)],
                ];
                let ordered: Vec<&[String; 2]> = if secret_first {
                    vec![&pair[0], &pair[1]]
                } else {
                    vec![&pair[1], &pair[0]]
                };
                for flag in ordered {
                    argv.extend(flag.iter().cloned());
                }
                let output = sandbox.run(&argv);
                let stderr = String::from_utf8_lossy(&output.stderr);
                if output.status.success() || !stderr.contains("cannot be used with") {
                    wrong.push(format!(
                        "`{command}` with `--api-key-secret` and `{other}` (secret first: \
                         {secret_first}) exited {:?}: {stderr}",
                        output.status.code()
                    ));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_pointer_beside_the_secret_flag_alone_is_refused_by_clap_on_every_command() {
    let sandbox = Sandbox::new();
    let base_url = closed_port();
    for command in COMMANDS {
        let mut argv = sandbox.required(command, &base_url);
        argv.extend([
            "--api-key-secret".to_owned(),
            UNPARSED.to_owned(),
            "--oauth-token-pointer".to_owned(),
            "/access_token".to_owned(),
        ]);
        let output = sandbox.run(&argv);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success() && stderr.contains("required arguments were not provided"),
            "`{command}`: a pointer names an OAuth document the secret flag does not read: \
             exit {:?}: {stderr}",
            output.status.code()
        );
    }
}

#[test]
fn every_command_the_document_does_not_give_the_flag_refuses_it() {
    let document = document();
    let sandbox = Sandbox::new();
    let mut accepted: Vec<String> = Vec::new();
    for command in document["subcommands"].as_array().expect("a command list") {
        let command = command.as_str().expect("a command");
        if row(&document, command, "--api-key-secret").is_some() {
            continue;
        }
        let mut argv: Vec<String> = command.split_whitespace().map(str::to_owned).collect();
        argv.extend(["--api-key-secret".to_owned(), UNPARSED.to_owned()]);
        let output = sandbox.run(&argv);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if output.status.success() || !stderr.contains("unexpected argument '--api-key-secret'") {
            accepted.push(format!(
                "`{command}` exited {:?}: {stderr}",
                output.status.code()
            ));
        }
    }
    assert!(accepted.is_empty(), "{}", accepted.join("\n"));
}

#[test]
fn a_malformed_reference_is_refused_by_name_on_every_command_before_any_request() {
    // `app-server` resolves its credential in its own function, apart from `run` and `chat`;
    // nothing else drives that branch through the binary, so dropping it would
    // leave app-server serving unauthenticated turns with the flag silently ignored.
    let sandbox = Sandbox::new();
    let mut wrong: Vec<String> = Vec::new();
    for command in COMMANDS {
        let endpoint = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        endpoint.set_nonblocking(true).expect("non-blocking");
        let base_url = format!("http://{}/v1", endpoint.local_addr().expect("bound"));
        let mut argv = sandbox.required(command, &base_url);
        argv.extend(["--api-key-secret".to_owned(), UNPARSED.to_owned()]);
        let output = sandbox.run(&argv);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let contacted = endpoint.accept().is_ok();
        if output.status.success()
            || !stderr.contains(&format!("`{UNPARSED}` is not `<namespace>/<name>`"))
            || contacted
        {
            wrong.push(format!(
                "`{command}` exited {:?}, contacted the endpoint: {contacted}: {stderr}",
                output.status.code()
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn without_the_flag_no_command_reaches_for_the_session_bus() {
    // An observation, not a proof: what would contact the bus is the native keychain, and the only
    // positive control (a reference that parses) is the one thing this file may not run. The bus
    // here is a socket this test serves, so a connection lands here and never on a real bus.
    let sandbox = Sandbox::new();
    let bus = UnixListener::bind(sandbox.bus()).expect("a fixture bus socket");
    bus.set_nonblocking(true).expect("non-blocking");
    let base_url = closed_port();
    let mut reached: Vec<String> = Vec::new();
    for command in COMMANDS {
        let argv = sandbox.required(command, &base_url);
        let output = sandbox.run(&argv);
        if bus.accept().is_ok() {
            reached.push(format!(
                "`{command}` (exit {:?}) connected to the session bus: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    assert!(reached.is_empty(), "{}", reached.join("\n"));
}
