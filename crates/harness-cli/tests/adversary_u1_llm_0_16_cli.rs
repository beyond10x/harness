//! Adversary cases for `story:harness-builds-on-llm-0-16`, against `23a6eb4`, through the shipped
//! binary. The story's decision: llm's `usage-extension` warning reaches the operator as a stderr
//! line and as one `{"kind":"warning",...}` line under `--json`, and the cost figure is never read
//! as the run's cost: the run's cost stays the one its rate card computes.
//!
//! The provider is a loopback Messages endpoint served by a thread in this test (std only, no
//! script); every secret and endpoint is synthetic (`AGENTS.md` invariant 17), and evidence from it
//! is `provider_emulated` (invariant 18).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");
const MODEL: &str = "b10x-emulated";

/// One Messages text turn: 11 input tokens on `message_start`, 8 output tokens and `end_usage`'s
/// extra fields on `message_delta`.
fn serve(end_usage: &str) -> String {
    let events = [
        (
            "message_start",
            format!(
                r#"{{"type":"message_start","message":{{"id":"msg_b10x_adv","type":"message","role":"assistant","model":"{MODEL}","content":[],"stop_reason":null,"stop_sequence":null,"usage":{{"input_tokens":11,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":1}}}}}}"#
            ),
        ),
        (
            "content_block_start",
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_owned(),
        ),
        (
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"forty-two"}}"#.to_owned(),
        ),
        (
            "content_block_stop",
            r#"{"type":"content_block_stop","index":0}"#.to_owned(),
        ),
        (
            "message_delta",
            format!(
                r#"{{"type":"message_delta","delta":{{"stop_reason":"end_turn","stop_sequence":null}},"usage":{end_usage}}}"#
            ),
        ),
        ("message_stop", r#"{"type":"message_stop"}"#.to_owned()),
    ];
    let mut body = String::new();
    for (name, data) in &events {
        for part in ["event: ", *name, "\ndata: ", data.as_str(), "\n\n"] {
            body.push_str(part);
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let base_url = format!("http://{}/v1", listener.local_addr().expect("bound"));
    std::thread::spawn(move || {
        for connection in listener.incoming() {
            let Ok(connection) = connection else { return };
            answer(connection, &body);
        }
    });
    base_url
}

fn answer(connection: TcpStream, body: &str) {
    let mut reader = BufReader::new(connection);
    let mut length = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut request = vec![0; length];
    if reader.read_exact(&mut request).is_err() {
        return;
    }
    let mut connection = reader.into_inner();
    let _ = write!(
        connection,
        "HTTP/1.0 200 OK\r\ncontent-type: text/event-stream\r\n\r\n{body}"
    );
}

fn rate_card(dir: &Path) -> PathBuf {
    let path = dir.join("rates.json");
    std::fs::write(
        &path,
        format!(
            r#"{{"source": "a synthetic card this test wrote", "as_of": "2026-10-26",
                "models": {{"{MODEL}": {{"input_usd_per_mtok": 1.0,
                                        "cached_input_usd_per_mtok": 0.1,
                                        "output_usd_per_mtok": 2.0}}}}}}"#
        ),
    )
    .expect("write");
    path
}

struct Ran {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run(end_usage: &str, extra: &[&str]) -> Ran {
    let base_url = serve(end_usage);
    let workspace = tempfile::tempdir().expect("a workspace");
    let output = Command::new(BINARY)
        .args([
            "run",
            "--wire",
            "anthropic-messages",
            "--base-url",
            &base_url,
            "--model",
            MODEL,
            "--api-key-env",
            "B10X_HARNESS_TEST_KEY",
            "--no-session",
            "--input",
            "what is six times seven",
        ])
        .args(extra)
        .arg("--workspace")
        .arg(workspace.path())
        .env("B10X_HARNESS_TEST_KEY", "test-key")
        .output()
        .expect("the binary runs");
    Ran {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn events(ran: &Ran) -> Vec<Value> {
    ran.stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|error| panic!("{line}: {error}")))
        .collect()
}

fn of_kind<'e>(events: &'e [Value], kind: &str) -> Vec<&'e Value> {
    events
        .iter()
        .filter(|event| event["kind"] == kind)
        .collect()
}

const COSTED: &str = r#"{"output_tokens":8,"cost":0.0123}"#;

#[test]
fn under_json_the_cost_is_one_usage_extension_warning_line_and_no_cost_event_without_a_card() {
    let ran = run(COSTED, &["--json"]);
    assert_eq!(ran.status, Some(0), "stderr: {}", ran.stderr);
    let events = events(&ran);
    let warnings = of_kind(&events, "warning");
    assert_eq!(
        warnings,
        vec![&serde_json::json!({
            "kind": "warning",
            "code": "usage-extension",
            "message": "message_delta.usage.cost: 0.0123",
        })],
        "{}",
        ran.stdout
    );
    assert_eq!(of_kind(&events, "usage").len(), 1, "{}", ran.stdout);
    assert!(
        of_kind(&events, "cost").is_empty(),
        "a run with no rate card reports no cost, whatever the route said: {}",
        ran.stdout
    );
}

#[test]
fn with_a_card_the_runs_cost_is_the_cards_price_of_the_tokens_not_the_reported_cost() {
    let cards = tempfile::tempdir().expect("a card directory");
    let card = rate_card(cards.path());
    let ran = run(
        COSTED,
        &["--json", "--prices", card.to_str().expect("utf-8 path")],
    );
    assert_eq!(ran.status, Some(0), "stderr: {}", ran.stderr);
    let events = events(&ran);
    let costs: Vec<u64> = of_kind(&events, "cost")
        .iter()
        .filter_map(|event| event["micro_usd"].as_u64())
        .collect();
    // 11 input tokens at $1/Mtok and 8 output at $2/Mtok: 11 + 16 micro-dollars. The route's
    // `cost: 0.0123` would be 12300.
    assert_eq!(costs, vec![27], "{}", ran.stdout);
}

#[test]
fn without_json_the_warning_is_one_stderr_line_and_the_answer_is_on_stdout() {
    let ran = run(COSTED, &[]);
    assert_eq!(ran.status, Some(0), "stderr: {}", ran.stderr);
    let lines: Vec<&str> = ran
        .stderr
        .lines()
        .filter(|line| line.contains("usage-extension"))
        .collect();
    assert_eq!(
        lines,
        vec!["warning [usage-extension] message_delta.usage.cost: 0.0123"],
        "{}",
        ran.stderr
    );
    assert!(ran.stdout.contains("forty-two"), "{}", ran.stdout);
}
