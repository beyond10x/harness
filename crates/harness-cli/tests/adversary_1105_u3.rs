//! Adversary pass on `story:tools-help-matches-its-output`, through the shipped binary.
//!
//! `tools` now prints `write_scope`, documented as "the scope `run` applies with the same flags".
//! The unit's own cases compare that key with literals on a machine with **no configuration**,
//! where the two commands happen to agree. Here each case first runs `run` and observes on disk
//! what it actually refused and admitted, then asks `tools` with the same flags on the same
//! machine, and holds the printed scope to what the run enforced, not to the function that
//! computes it.
//!
//! The configuration used is the operator's own `[default]` table, which applies to every run
//! without a `-p`: `tools` takes no `-p`, so `[default]` is the only profile input both commands
//! can be given "with the same flags".
//!
//! The model endpoint is served from this test process: the first request asks for three
//! `file_write` calls, every later one gets a short answer. Every case has a machine of its own.

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
        "id": "resp_adv_u3",
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
            "id": format!("fc_adv_u3_{index}"),
            "type": "function_call",
            "status": status,
            "name": "file_write",
            "call_id": format!("call_adv_u3_{index}"),
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
            "item_id": format!("fc_adv_u3_{index}"),
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
        "id": "msg_adv_u3",
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
                "id": "msg_adv_u3",
                "type": "message",
                "status": "in_progress",
                "role": "assistant",
                "content": [],
            },
        }),
        json!({
            "type": "response.output_text.delta",
            "item_id": "msg_adv_u3",
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

/// One machine: a home whose operator configuration is `config`, and a workspace holding a Git
/// configuration and a repository profile.
struct Machine {
    root: tempfile::TempDir,
}

impl Machine {
    fn with_config(config: &str) -> Self {
        let root = tempfile::Builder::new()
            .prefix("adv-1105-u3-")
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
        let b10x = root.path().join("home").join("config").join("b10x");
        fs::create_dir_all(&b10x).expect("a config directory");
        fs::write(b10x.join("harness.toml"), config).expect("the operator's configuration");
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

    /// `run` with `extra` typed after `--workspace`; its exit status and stderr.
    fn run(&self, extra: &[&str]) -> (Option<i32>, String) {
        let endpoint = model_endpoint();
        let output = Command::new(BINARY)
            .stdin(Stdio::null())
            .args([
                "run",
                "--base-url",
                &endpoint,
                "--model",
                "b10x-emulated",
                "--api-key-env",
                "B10X_ADV_U3_KEY",
                "--no-session",
                "--yes",
                "--input",
                "write the three files",
                "--workspace",
            ])
            .arg(self.workspace())
            .args(extra)
            .env("B10X_ADV_U3_KEY", "test-key")
            .env("HOME", self.home())
            .env("XDG_CONFIG_HOME", self.home().join("config"))
            .env("XDG_STATE_HOME", self.home().join("state"))
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .output()
            .expect("the binary runs");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }

    /// `tools` with `extra` typed after `--workspace`, and the JSON document it prints.
    fn tools(&self, extra: &[&str]) -> serde_json::Value {
        let output = Command::new(BINARY)
            .stdin(Stdio::null())
            .arg("tools")
            .arg("--workspace")
            .arg(self.workspace())
            .args(extra)
            .env("HOME", self.home())
            .env("XDG_CONFIG_HOME", self.home().join("config"))
            .env("XDG_STATE_HOME", self.home().join("state"))
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
}

/// The operator's `[default]` names a `write-scope`; the documented rule (profiles guide: "A
/// `write-scope` in a profile or a typed `--write-scope` replaces it whole") makes it the scope of
/// a typed `--substrate-embedded` run. The run is observed rewriting `.b10x/harness.toml`, which
/// the configured `**=allowed` admits. `tools --substrate-embedded` on the same machine must then
/// not say `.b10x/**` is denied.
#[test]
fn tools_names_the_configured_scope_a_typed_embedded_run_enforces() {
    let machine = Machine::with_config("[default]\nwrite-scope = [\"**=allowed\"]\n");

    let (status, stderr) = machine.run(&["--substrate-embedded"]);
    assert_eq!(status, Some(0), "the run completes: {stderr}");
    assert_eq!(
        machine.read(".b10x/harness.toml"),
        WRITTEN,
        "precondition: the run applies the configured `**=allowed` and rewrites the repository \
         profile\nstderr: {stderr}"
    );

    let described = machine.tools(&["--substrate-embedded"]);
    assert_eq!(
        described["write_scope"],
        json!([{"paths": "**", "write": "allowed"}]),
        "story:tools-help-matches-its-output — `run --substrate-embedded` on this machine just \
         rewrote `.b10x/harness.toml` under the configured scope, and `tools \
         --substrate-embedded` with the same flags names a different scope:\n{:#}",
        described["write_scope"]
    );
}

/// The operator's `[default]` sets `write = true`: every run without `-p` is a confined run under
/// the default scope. The run with no confinement flag is observed writing `notes.md` and refusing
/// the repository profile. `tools` with the same (no) flags must then not say the run writes
/// nowhere, which is what its empty `write_scope` is documented to mean.
#[test]
fn tools_names_the_scope_a_default_write_profile_run_enforces() {
    let machine = Machine::with_config("[default]\nwrite = true\n");

    let (status, stderr) = machine.run(&[]);
    assert_eq!(status, Some(0), "the run completes: {stderr}");
    assert_eq!(
        machine.read("notes.md"),
        WRITTEN,
        "precondition: `[default] write = true` makes the run write\nstderr: {stderr}"
    );
    assert_eq!(
        machine.read(".b10x/harness.toml"),
        REPOSITORY_PROFILE,
        "precondition: the run applies the default scope\nstderr: {stderr}"
    );

    let described = machine.tools(&[]);
    assert_eq!(
        described["write_scope"],
        json!([
            {"paths": ".git/**", "write": "denied"},
            {"paths": ".b10x/**", "write": "denied"},
            {"paths": "**", "write": "allowed"},
        ]),
        "story:tools-help-matches-its-output — `run` with no flags on this machine just wrote \
         `notes.md` under the default scope, and `tools` with the same flags says the run writes \
         nowhere:\n{:#}",
        described["write_scope"]
    );
}
