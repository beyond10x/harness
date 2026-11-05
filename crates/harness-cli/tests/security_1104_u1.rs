//! Security review pass 1 on `story:typed-substrate-run-gets-the-default-write-scope`.
//!
//! Integrity checks of the invariant the story states: every run that may write through substrate
//! starts from the default write scope (`.git/**=denied .b10x/**=denied **=allowed`) or the
//! operator's configured `write-scope`, unless the operator typed `--write-scope`. Each case drives
//! the shipped binary against a model endpoint served from this test process, which asks for the
//! writes the case names; the files on disk afterwards are the observation. Every case runs on a
//! machine of its own (`HOME` and the XDG directories in one temporary directory).

use std::fs;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::json;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

const REPOSITORY_PROFILE: &str = "# the repository's own profile, applied by the next run\n";
const WRITTEN: &str = "written by the model\n";

/// A Responses endpoint on a loopback port that asks for `targets` to be written, then answers.
fn model_endpoint(targets: Vec<String>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind the model endpoint");
    let address = listener.local_addr().expect("the endpoint's address");
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let body = request_body(&mut stream);
            let events = if body.contains("function_call_output") {
                answer_events()
            } else {
                write_events(&targets)
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
        "id": "resp_sec_1104_u1",
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

fn write_events(targets: &[String]) -> Vec<serde_json::Value> {
    let call = |index: usize, status: &str, arguments: &str| {
        json!({
            "id": format!("fc_sec_{index}"),
            "type": "function_call",
            "status": status,
            "name": "file_write",
            "call_id": format!("call_sec_{index}"),
            "arguments": arguments,
        })
    };
    let mut events = vec![json!({
        "type": "response.created",
        "response": response("in_progress", &json!([]), &serde_json::Value::Null),
    })];
    let mut output = Vec::new();
    for (index, path) in targets.iter().enumerate() {
        let arguments = json!({"path": path, "text": WRITTEN}).to_string();
        let done = call(index, "completed", &arguments);
        events.push(json!({
            "type": "response.output_item.added",
            "output_index": index,
            "item": call(index, "in_progress", ""),
        }));
        events.push(json!({
            "type": "response.function_call_arguments.delta",
            "item_id": format!("fc_sec_{index}"),
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
        "id": "msg_sec",
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
                "id": "msg_sec",
                "type": "message",
                "status": "in_progress",
                "role": "assistant",
                "content": [],
            },
        }),
        json!({
            "type": "response.output_text.delta",
            "item_id": "msg_sec",
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

/// One machine: an empty home and a workspace holding a repository profile.
struct Machine {
    root: tempfile::TempDir,
}

impl Machine {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("sec-1104-u1-")
            .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
            .expect("a temporary directory");
        let workspace = root.path().join("work");
        fs::create_dir_all(workspace.join(".b10x")).expect("a .b10x directory");
        fs::create_dir_all(workspace.join("src")).expect("a src directory");
        fs::write(
            workspace.join(".b10x").join("harness.toml"),
            REPOSITORY_PROFILE,
        )
        .expect("a repository profile");
        fs::write(workspace.join("README.md"), "hello\n").expect("a readme");
        fs::create_dir_all(root.path().join("home").join("config").join("b10x"))
            .expect("a config directory");
        Self { root }
    }

    fn workspace(&self) -> PathBuf {
        self.root.path().join("work")
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn config_dir(&self) -> PathBuf {
        self.home().join("config").join("b10x")
    }

    fn profile(&self) -> String {
        fs::read_to_string(self.workspace().join(".b10x").join("harness.toml")).unwrap_or_default()
    }

    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.workspace().join(path)).unwrap_or_default()
    }

    fn command(&self) -> Command {
        let mut command = Command::new(BINARY);
        command
            .env("HOME", self.home())
            .env("XDG_CONFIG_HOME", self.home().join("config"))
            .env("XDG_STATE_HOME", self.home().join("state"))
            .env("B10X_SEC_KEY", "test-key")
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY");
        command
    }

    /// `verb` (`run` or `chat`) on the embedded driver with no `--write-scope`, the model asking
    /// for `targets` to be written.
    fn writing(&self, verb: &str, targets: &[&str]) -> Output {
        let endpoint = model_endpoint(targets.iter().map(|t| (*t).to_owned()).collect());
        let mut command = self.command();
        command.args([
            verb,
            "--base-url",
            &endpoint,
            "--model",
            "b10x-emulated",
            "--api-key-env",
            "B10X_SEC_KEY",
            "--no-session",
            "--yes",
            "--substrate-embedded",
        ]);
        command.arg("--workspace").arg(self.workspace());
        if verb == "run" {
            command
                .stdin(Stdio::null())
                .args(["--input", "write the files"]);
            command.output().expect("the binary runs")
        } else {
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child = command.spawn().expect("the binary starts");
            child
                .stdin
                .take()
                .expect("stdin")
                .write_all(b"write the files\n")
                .expect("the question is written");
            child.wait_with_output().expect("the binary runs")
        }
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[cfg(unix)]
fn link(target: &str, at: &Path) {
    std::os::unix::fs::symlink(target, at).expect("a symlink");
}

/// The default `.b10x/**=denied` is matched lexically by Harness. Every other spelling of the
/// repository profile a model can write must land on the same refusal.
#[test]
fn a_typed_embedded_run_refuses_every_spelling_of_the_repository_profile() {
    let machine = Machine::new();
    let output = machine.writing(
        "run",
        &[
            "./.b10x/harness.toml",
            ".b10x/./harness.toml",
            "src/../.b10x/harness.toml",
            ".b10x//harness.toml",
            ".b10x/harness.toml/",
            "notes.md",
        ],
    );
    let stderr = stderr(&output);
    assert_eq!(output.status.code(), Some(0), "the run completes: {stderr}");
    assert_eq!(
        machine.read("notes.md"),
        WRITTEN,
        "the ordinary write is admitted, so the run reached its tools: {stderr}"
    );
    assert_eq!(
        machine.profile(),
        REPOSITORY_PROFILE,
        "story:typed-substrate-run-gets-the-default-write-scope — another spelling of \
         `.b10x/harness.toml` walked past the default scope: {stderr}"
    );
}

/// A link inside the workspace is a second spelling of the profile. Whichever layer refuses it,
/// Harness's scope or substrate's guarded IO, the profile must not change.
#[cfg(unix)]
#[test]
fn a_typed_embedded_run_refuses_the_repository_profile_through_a_link() {
    let machine = Machine::new();
    link(".b10x", &machine.workspace().join("cfg"));
    link(
        ".b10x/harness.toml",
        &machine.workspace().join("alias.toml"),
    );
    let output = machine.writing("run", &["cfg/harness.toml", "alias.toml", "notes.md"]);
    let stderr = stderr(&output);
    assert_eq!(output.status.code(), Some(0), "the run completes: {stderr}");
    assert_eq!(machine.read("notes.md"), WRITTEN, "{stderr}");
    assert_eq!(
        machine.profile(),
        REPOSITORY_PROFILE,
        "story:typed-substrate-run-gets-the-default-write-scope — a link inside the workspace \
         carried a write into `.b10x/`: {stderr}"
    );
}

/// `chat` resolves its options through the same profile step as `run`, so a typed confined
/// conversation is held to the same default.
#[test]
fn a_typed_embedded_chat_refuses_the_repository_profile() {
    let machine = Machine::new();
    let output = machine.writing("chat", &[".b10x/harness.toml", "notes.md"]);
    let stderr = stderr(&output);
    assert_eq!(
        machine.read("notes.md"),
        WRITTEN,
        "the chat reached its tools (status {:?}): {stderr}",
        output.status.code()
    );
    assert_eq!(
        machine.profile(),
        REPOSITORY_PROFILE,
        "story:typed-substrate-run-gets-the-default-write-scope — a typed `--substrate-embedded` \
         chat rewrote the repository profile: {stderr}"
    );
}

/// A trusted repository profile may not choose the scope a typed confined run applies: a
/// `write-scope` in it refuses the run rather than replacing the default.
#[test]
fn a_trusted_repository_profile_cannot_choose_the_write_scope() {
    let machine = Machine::new();
    fs::write(
        machine.workspace().join(".b10x").join("harness.toml"),
        "[default]\nwrite-scope = [\"**=allowed\"]\n",
    )
    .expect("a repository profile with a scope");
    let trust = machine
        .command()
        .stdin(Stdio::null())
        .args(["profiles", "trust"])
        .arg(machine.workspace())
        .output()
        .expect("trust runs");
    assert_eq!(trust.status.code(), Some(0), "trust: {}", stderr(&trust));
    let output = machine.writing("run", &[".b10x/harness.toml", "notes.md"]);
    let stderr = stderr(&output);
    assert_ne!(
        output.status.code(),
        Some(0),
        "a repository `write-scope` is refused, not applied: {stderr}"
    );
    assert_eq!(
        machine.read("notes.md"),
        "",
        "the refused run wrote nothing: {stderr}"
    );
    assert_eq!(
        machine.profile(),
        "[default]\nwrite-scope = [\"**=allowed\"]\n",
        "story:typed-substrate-run-gets-the-default-write-scope — a repository profile chose the \
         scope of a typed confined run: {stderr}"
    );
}

/// `profiles explain` names the configured `write-scope` a typed confined run starts from when a
/// profile without `write` carries one, because that is what `fill_write_scope` applies.
#[test]
fn profiles_explain_names_a_configured_scope_for_a_typed_confined_run() {
    let machine = Machine::new();
    fs::write(
        machine.config_dir().join("harness.toml"),
        "[default]\nwrite-scope = [\"notes.md=allowed\", \"**=denied\"]\n",
    )
    .expect("a config");
    let output = machine
        .command()
        .stdin(Stdio::null())
        .current_dir(machine.workspace())
        .args(["profiles", "explain"])
        .output()
        .expect("explain runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(0),
        "explain: {}",
        stderr(&output)
    );
    let line = stdout
        .lines()
        .find(|line| line.contains("--substrate-embedded") && line.contains("--write-scope"))
        .unwrap_or_default();
    assert!(
        line.contains("--write-scope notes.md=allowed --write-scope **=denied")
            && !line.contains(".b10x/**=denied"),
        "story:typed-substrate-run-gets-the-default-write-scope — explain must name the \
         configured scope a typed confined run applies, not the default:\n{stdout}"
    );
}

/// `tools` answers "what can this run do?" for the same flags `run` takes. A typed
/// `--substrate-embedded` listing with no `--write-scope` must describe the scope a run with those
/// flags applies, which is now the default; so it must equal the listing with the default typed.
#[test]
fn tools_describes_the_scope_a_typed_confined_run_applies() {
    let machine = Machine::new();
    let listing = |scope: &[&str]| {
        let output = machine
            .command()
            .stdin(Stdio::null())
            .args(["tools", "--substrate-embedded", "--workspace"])
            .arg(machine.workspace())
            .args(scope)
            .output()
            .expect("tools runs");
        assert_eq!(output.status.code(), Some(0), "tools: {}", stderr(&output));
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let untyped = listing(&[]);
    let typed = listing(&[
        "--write-scope",
        ".git/**=denied",
        "--write-scope",
        ".b10x/**=denied",
        "--write-scope",
        "**=allowed",
    ]);
    assert_eq!(
        untyped, typed,
        "story:typed-substrate-run-gets-the-default-write-scope — `tools --substrate-embedded` \
         without `--write-scope` describes a different scope from the one `run` applies with the \
         same flags"
    );
}
