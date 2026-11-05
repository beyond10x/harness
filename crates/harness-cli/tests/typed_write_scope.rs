//! `story:typed-substrate-run-gets-the-default-write-scope`, through the shipped binary.
//!
//! The default write scope (`profile::default_write_scope`: `.git/**` and `.b10x/**` denied, the
//! rest allowed) used to reach only a run whose profile set `write = true`. A run that typed
//! `--substrate-embedded` itself, with no `--write-scope`, got no scope at all, so the model could
//! rewrite the repository profile the next run applies. (`.git/` was held all along, one layer
//! down: substrate's guarded IO refuses every path with a `.git` component. The default's
//! `.git/**=denied` refuses it in Harness's own words first.)
//!
//! Each case runs the binary against a model endpoint served from this test process: the first
//! request is answered with three `file_write` calls, every later one with a short answer. The
//! files on disk afterwards are the observation. Every case runs on a machine of its own: `HOME`
//! and the XDG directories point into one temporary directory holding no configuration, so no
//! profile applies and the operator's real files are never read.

use std::fs;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::json;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

const GIT_CONFIG: &str = "[core]\n\trepositoryformatversion = 0\n";
const REPOSITORY_PROFILE: &str = "# the repository's own profile, applied by the next run\n";
const WRITTEN: &str = "written by the model\n";

/// The three writes the model asks for, in one response.
const TARGETS: [&str; 3] = [".b10x/harness.toml", ".git/config", "notes.md"];

/// A Responses endpoint on a loopback port that asks for [`TARGETS`] to be written, then answers.
fn model_endpoint() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind the model endpoint");
    let address = listener.local_addr().expect("the endpoint's address");
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let body = request_body(&mut stream);
            let events = if body.contains("function_call_output") {
                answer_events()
            } else {
                write_events()
            };
            respond(&mut stream, &events);
        }
    });
    format!("http://{address}/v1")
}

fn request_body(stream: &mut TcpStream) -> String {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let mut length = 0_usize;
    reader.read_line(&mut line).expect("the request line");
    loop {
        line.clear();
        reader.read_line(&mut line).expect("a request header");
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.trim_end().split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().expect("a numeric content length");
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).expect("the request body");
    String::from_utf8_lossy(&body).into_owned()
}

fn respond(stream: &mut TcpStream, events: &[serde_json::Value]) {
    let mut body = String::new();
    for event in events {
        body.push_str("data: ");
        body.push_str(&event.to_string());
        body.push_str("\n\n");
    }
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

fn response(
    status: &str,
    output: &serde_json::Value,
    usage: &serde_json::Value,
) -> serde_json::Value {
    json!({
        "id": "resp_typed_scope",
        "object": "response",
        "created_at": 1_786_706_400,
        "status": status,
        "model": "b10x-emulated",
        "output": output,
        "incomplete_details": null,
        "error": null,
        "usage": usage,
    })
}

fn usage() -> serde_json::Value {
    json!({
        "input_tokens": 42,
        "input_tokens_details": {"cached_tokens": 0},
        "output_tokens": 8,
        "output_tokens_details": {"reasoning_tokens": 0},
        "total_tokens": 50,
    })
}

fn write_events() -> Vec<serde_json::Value> {
    let call = |index: usize, status: &str, arguments: &str| {
        json!({
            "id": format!("fc_typed_scope_{index}"),
            "type": "function_call",
            "status": status,
            "name": "file_write",
            "call_id": format!("call_typed_scope_{index}"),
            "arguments": arguments,
        })
    };
    let mut events = vec![json!({
        "type": "response.created",
        "response": response("in_progress", &json!([]), &serde_json::Value::Null),
    })];
    let mut output = Vec::new();
    for (index, path) in TARGETS.iter().enumerate() {
        let arguments = json!({"path": path, "text": WRITTEN}).to_string();
        let added = call(index, "in_progress", "");
        let done = call(index, "completed", &arguments);
        events.push(json!({
            "type": "response.output_item.added",
            "output_index": index,
            "item": added,
        }));
        events.push(json!({
            "type": "response.function_call_arguments.delta",
            "item_id": format!("fc_typed_scope_{index}"),
            "output_index": index,
            "delta": arguments,
        }));
        events.push(json!({
            "type": "response.output_item.done",
            "output_index": index,
            "item": done,
        }));
        output.push(done);
    }
    events.push(json!({
        "type": "response.completed",
        "response": response("completed", &serde_json::Value::Array(output), &usage()),
    }));
    events
}

fn answer_events() -> Vec<serde_json::Value> {
    let text = "done";
    let item = json!({
        "id": "msg_typed_scope",
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": [{"type": "output_text", "text": text, "annotations": []}],
    });
    vec![
        json!({
            "type": "response.created",
            "response": response("in_progress", &json!([]), &serde_json::Value::Null),
        }),
        json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "item": {
                "id": "msg_typed_scope",
                "type": "message",
                "status": "in_progress",
                "role": "assistant",
                "content": [],
            },
        }),
        json!({
            "type": "response.output_text.delta",
            "item_id": "msg_typed_scope",
            "output_index": 0,
            "delta": text,
        }),
        json!({"type": "response.output_item.done", "output_index": 0, "item": item}),
        json!({
            "type": "response.completed",
            "response": response("completed", &json!([item]), &usage()),
        }),
    ]
}

/// One machine: an empty home and a workspace holding a Git configuration and a repository
/// profile, both of which the default scope protects.
struct Machine {
    root: tempfile::TempDir,
}

impl Machine {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("typed-write-scope-")
            .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
            .expect("a temporary directory");
        let workspace = root.path().join("work");
        fs::create_dir_all(workspace.join(".git")).expect("a .git directory");
        fs::create_dir_all(workspace.join(".b10x")).expect("a .b10x directory");
        fs::write(workspace.join(".git").join("config"), GIT_CONFIG).expect("a Git config");
        fs::write(
            workspace.join(".b10x").join("harness.toml"),
            REPOSITORY_PROFILE,
        )
        .expect("a repository profile");
        fs::write(workspace.join("README.md"), "hello\n").expect("a readme");
        fs::create_dir_all(root.path().join("home")).expect("a home");
        Self { root }
    }

    fn workspace(&self) -> PathBuf {
        self.root.path().join("work")
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.workspace().join(path)).unwrap_or_default()
    }

    /// A run that writes through the embedded driver, with `extra` typed after the rest. `home`
    /// false runs with neither `HOME` nor `XDG_CONFIG_HOME`, so there is no configuration file to
    /// read at all.
    fn run(&self, extra: &[&str], home: bool) -> (Option<i32>, String) {
        let endpoint = model_endpoint();
        let workspace = self.workspace();
        let mut command = Command::new(BINARY);
        command
            .stdin(Stdio::null())
            .args([
                "run",
                "--base-url",
                &endpoint,
                "--model",
                "b10x-emulated",
                "--api-key-env",
                "B10X_TYPED_SCOPE_KEY",
                "--no-session",
                "--yes",
                "--input",
                "write the three files",
                "--substrate-embedded",
                "--workspace",
            ])
            .arg(&workspace)
            .args(extra)
            .env("B10X_TYPED_SCOPE_KEY", "test-key")
            .env("XDG_STATE_HOME", self.home().join("state"))
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY");
        if home {
            command
                .env("HOME", self.home())
                .env("XDG_CONFIG_HOME", self.home().join("config"));
        } else {
            command.env_remove("HOME").env_remove("XDG_CONFIG_HOME");
        }
        let output = command.output().expect("the binary runs");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }

    /// The run happened and wrote the ordinary file: without that, an untouched `.git/config`
    /// proves nothing.
    fn assert_the_ordinary_write_was_admitted(&self, status: Option<i32>, stderr: &str) {
        assert_eq!(status, Some(0), "the run completes: {stderr}");
        assert_eq!(
            self.read("notes.md"),
            WRITTEN,
            "story:typed-substrate-run-gets-the-default-write-scope — a write to an ordinary file \
             is admitted under the default scope\nstderr: {stderr}"
        );
    }

    fn assert_protected_files_untouched(&self, stderr: &str) {
        assert_eq!(
            self.read(".git/config"),
            GIT_CONFIG,
            "story:typed-substrate-run-gets-the-default-write-scope — a typed \
             `--substrate-embedded` run with no `--write-scope` rewrote `.git/config`\nstderr: \
             {stderr}"
        );
        assert_eq!(
            self.read(".b10x/harness.toml"),
            REPOSITORY_PROFILE,
            "story:typed-substrate-run-gets-the-default-write-scope — a typed \
             `--substrate-embedded` run with no `--write-scope` rewrote the repository profile \
             the next run applies\nstderr: {stderr}"
        );
    }
}

#[test]
fn a_typed_embedded_run_without_a_scope_refuses_git_and_the_repository_profile() {
    let machine = Machine::new();
    let (status, stderr) = machine.run(&[], true);
    machine.assert_the_ordinary_write_was_admitted(status, &stderr);
    machine.assert_protected_files_untouched(&stderr);
}

/// The same run on a machine with no configuration directory at all: the profile step returns
/// early there, and the default must not depend on it reaching the end.
#[test]
fn a_typed_embedded_run_with_no_configuration_directory_still_gets_the_default_scope() {
    let machine = Machine::new();
    let (status, stderr) = machine.run(&[], false);
    machine.assert_the_ordinary_write_was_admitted(status, &stderr);
    machine.assert_protected_files_untouched(&stderr);
}

/// A typed `--write-scope` replaces the default whole, as it does for a profile run: the operator
/// who types `**=allowed` has said the run may write `.b10x/` too. (`.git/` stays refused whatever
/// the scope says: substrate's guarded IO refuses every path with a `.git` component itself, so
/// only the repository profile can show the default was replaced.)
#[test]
fn a_typed_write_scope_replaces_the_default() {
    let machine = Machine::new();
    let (status, stderr) = machine.run(&["--write-scope", "**=allowed"], true);
    machine.assert_the_ordinary_write_was_admitted(status, &stderr);
    assert_eq!(
        machine.read(".b10x/harness.toml"),
        WRITTEN,
        "story:typed-substrate-run-gets-the-default-write-scope — a typed `--write-scope` \
         replaces the default\nstderr: {stderr}"
    );
    assert_eq!(
        machine.read(".git/config"),
        GIT_CONFIG,
        "substrate refuses `.git/` under any scope\nstderr: {stderr}"
    );
}

/// And the typed scope is the whole scope, not merged with the default: it refuses a file the
/// default allows and admits the repository profile the default refuses.
#[test]
fn a_typed_write_scope_is_applied_instead_of_the_default_not_beside_it() {
    let machine = Machine::new();
    let (status, stderr) = machine.run(
        &[
            "--write-scope",
            "notes.md=denied",
            "--write-scope",
            "**=allowed",
        ],
        true,
    );
    assert_eq!(status, Some(0), "the run completes: {stderr}");
    assert_eq!(
        machine.read("notes.md"),
        "",
        "story:typed-substrate-run-gets-the-default-write-scope — the typed scope refuses \
         `notes.md`\nstderr: {stderr}"
    );
    assert_eq!(
        machine.read(".b10x/harness.toml"),
        WRITTEN,
        "story:typed-substrate-run-gets-the-default-write-scope — the typed scope is the whole \
         scope, and the default's `.b10x/**=denied` is not added to it\nstderr: {stderr}"
    );
}

/// `profiles explain` says what a run applies. With no profile setting `write`, it names the scope
/// a typed `--substrate-embedded` or `--substrate` run starts from, because that is the scope the
/// run above was held to.
#[test]
fn profiles_explain_names_the_scope_a_typed_confined_run_starts_from() {
    let machine = Machine::new();
    let output = Command::new(BINARY)
        .stdin(Stdio::null())
        .args(["profiles", "explain"])
        .current_dir(machine.workspace())
        .env("HOME", machine.home())
        .env("XDG_CONFIG_HOME", machine.home().join("config"))
        .env("XDG_STATE_HOME", machine.home().join("state"))
        .output()
        .expect("the binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(0),
        "explain: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    for rule in [".git/**=denied", ".b10x/**=denied", "**=allowed"] {
        assert!(
            stdout
                .lines()
                .any(|line| line.contains("--substrate-embedded") && line.contains(rule)),
            "story:typed-substrate-run-gets-the-default-write-scope — `profiles explain` does not \
             say that a typed `--substrate-embedded` run applies `--write-scope {rule}`:\n{stdout}"
        );
    }
}

/// A profile's own `write-scope` is what a typed `--substrate-embedded` run starts from when the
/// profile names one, even when the profile does not set `write`; the default fills only what
/// nobody wrote.
#[test]
fn a_profile_write_scope_applies_to_a_typed_embedded_run() {
    let machine = Machine::new();
    let config = machine.home().join("config").join("b10x");
    fs::create_dir_all(&config).expect("a config directory");
    fs::write(
        config.join("harness.toml"),
        "[default]\nwrite-scope = [\"notes.md=allowed\", \"**=denied\"]\n",
    )
    .expect("a config");
    let (status, stderr) = machine.run(&[], true);
    machine.assert_the_ordinary_write_was_admitted(status, &stderr);
    machine.assert_protected_files_untouched(&stderr);
}

/// `tools` with `extra` typed after `--workspace`, on this machine with no configuration, and the
/// JSON document it prints.
fn tools(machine: &Machine, extra: &[&str]) -> serde_json::Value {
    let output = Command::new(BINARY)
        .stdin(Stdio::null())
        .arg("tools")
        .arg("--workspace")
        .arg(machine.workspace())
        .args(extra)
        .env("HOME", machine.home())
        .env("XDG_CONFIG_HOME", machine.home().join("config"))
        .env("XDG_STATE_HOME", machine.home().join("state"))
        .output()
        .expect("the binary runs");
    assert_eq!(
        output.status.code(),
        Some(0),
        "tools: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("`tools` prints one JSON document")
}

/// `tools --substrate-embedded` with no `--write-scope` names the default scope, rule by rule in
/// the order a run applies it, because that is the scope `run` applies with the same flags.
#[test]
fn tools_names_the_default_scope_a_typed_embedded_run_applies() {
    let machine = Machine::new();
    let described = tools(&machine, &["--substrate-embedded"]);
    assert_eq!(
        described["write_scope"],
        json!([
            {"paths": ".git/**", "write": "denied"},
            {"paths": ".b10x/**", "write": "denied"},
            {"paths": "**", "write": "allowed"},
        ]),
        "story:tools-help-matches-its-output — `tools --substrate-embedded` without \
         `--write-scope` does not name the default scope a run with the same flags applies:\n\
         {described:#}"
    );
}

/// A typed `--write-scope` is the scope `tools` names, whole and in the typed order, with nothing
/// of the default beside it.
#[test]
fn tools_names_a_typed_write_scope_instead_of_the_default() {
    let machine = Machine::new();
    let described = tools(
        &machine,
        &[
            "--substrate-embedded",
            "--write-scope",
            "notes.md=denied",
            "--write-scope",
            "**=partial-only",
        ],
    );
    assert_eq!(
        described["write_scope"],
        json!([
            {"paths": "notes.md", "write": "denied"},
            {"paths": "**", "write": "partial-only"},
        ]),
        "story:tools-help-matches-its-output — `tools` does not name the typed scope:\n\
         {described:#}"
    );
}

/// A listing with no confinement describes a read-only run, which applies no scope: the key is
/// present and empty, typed `--write-scope` or not, so a reader can tell *this run writes nowhere*
/// from *this build does not say*.
#[test]
fn tools_without_confinement_names_an_empty_scope() {
    let machine = Machine::new();
    for extra in [&[][..], &["--write-scope", "**=allowed"][..]] {
        let described = tools(&machine, extra);
        assert_eq!(
            described["write_scope"],
            json!([]),
            "story:tools-help-matches-its-output — a read-only listing ({extra:?}) does not name \
             an empty scope:\n{described:#}"
        );
    }
}
