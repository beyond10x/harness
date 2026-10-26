//! Adversary cases for `story:harness-builds-on-llm-0-16`, against `23a6eb4`. Each case drives
//! the public port, or a document the unit edited, against what the unit and the story say.
//!
//! Every secret and every endpoint is synthetic and loopback (`AGENTS.md` invariant 17); evidence
//! from this endpoint is `provider_emulated` (invariant 18).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;

use harness_llm::{Protocol, Source, Target, client};
use harness_wire::{
    Cancel, Item, Sampling, StreamEvent, ToolChoice, TurnOutcome, TurnRequest, VecSink, WireError,
    WireErrorCode,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(root().join(relative))
        .unwrap_or_else(|error| panic!("{relative}: {error}"))
}

fn pinned_llm_tag(manifest: &str) -> String {
    manifest
        .lines()
        .find(|line| line.starts_with("llm-core"))
        .and_then(|line| line.split("tag = \"").nth(1))
        .and_then(|rest| rest.split('"').next())
        .expect("Cargo.toml pins llm-core by tag")
        .to_owned()
}

fn is_tag(name: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root())
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/tags/{name}"),
        ])
        .output()
        .expect("git runs")
        .status
        .success()
}

fn manifest_at(tag: &str) -> String {
    let shown = Command::new("git")
        .arg("-C")
        .arg(root())
        .args(["show", &format!("{tag}:Cargo.toml")])
        .output()
        .expect("git runs");
    assert!(shown.status.success(), "git show {tag}:Cargo.toml");
    String::from_utf8_lossy(&shown.stdout).into_owned()
}

// ---------------------------------------------------------------------------------------------
// Contract drift: documents that date the new pin to a release that does not carry it.
// ---------------------------------------------------------------------------------------------

#[test]
fn the_readme_does_not_date_llm_0_16_to_a_release_already_tagged_on_0_15() {
    // README.md:25-26 (edited by this unit): "At `0.20.0` those pins are … llm tag `0.16.0`
    // (`33ae9001`)". `0.20.0` is a tag in this repository (the unit's base 684c99f), and its
    // Cargo.toml pins llm `0.15.0`. The existing check
    // `the_readme_pins_it_dates_to_a_release_are_that_releases_pins` reads this tree's manifest
    // whenever the named version equals the workspace version, so it cannot see that the version
    // is already released with a different pin.
    let readme = read("README.md");
    let sentence = readme
        .split("At `")
        .nth(1)
        .expect("README.md dates its pins to a version");
    let version = sentence.split('`').next().expect("a version").to_owned();
    let claimed = sentence
        .split("llm tag `")
        .nth(1)
        .and_then(|rest| rest.split('`').next())
        .expect("README.md names an llm tag")
        .to_owned();
    if is_tag(&version) {
        let released = pinned_llm_tag(&manifest_at(&version));
        assert_eq!(
            claimed, released,
            "README.md says release `{version}` pins llm tag `{claimed}`; the tag `{version}` \
             pins `{released}`"
        );
    } else {
        assert_eq!(claimed, pinned_llm_tag(&read("Cargo.toml")));
    }
}

#[test]
fn status_names_the_llm_tag_cargo_toml_pins_as_the_current_wire() {
    // STATUS.md rows "Source", "Responses wire" and "Messages wire" state the current llm pin
    // ("llm tag `0.15.0` since 2026-10-26", "(2026-10-26, llm `0.15.0`)"). After this unit the
    // tree builds on another tag and those rows describe a graph nobody builds.
    let status = read("STATUS.md");
    let tag = pinned_llm_tag(&read("Cargo.toml"));
    let mut stale = Vec::new();
    for (number, line) in status.lines().enumerate() {
        for marker in ["llm tag `", "llm `"] {
            let parts: Vec<&str> = line.split(marker).collect();
            for pair in parts.windows(2) {
                let (before, rest) = (pair[0], pair[1]);
                let named = rest.split('`').next().unwrap_or_default();
                // A row that quotes a past pin as history ("built on harness 0.15.1 / llm
                // 0.8.1") names a harness version beside it; the current-pin phrases do not.
                let historical = before.trim_end().ends_with('/');
                if named != tag && !historical {
                    stale.push(format!("STATUS.md:{}: {marker}{named}`", number + 1));
                }
            }
        }
    }
    assert!(
        stale.is_empty(),
        "STATUS.md names an llm pin Cargo.toml does not hold ({tag}): {stale:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// Boundaries of the usage extension at the port: where, what shape, and what still refuses.
// ---------------------------------------------------------------------------------------------

const START_USAGE: &str = r#""input_tokens":11,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":1"#;

/// A loopback Messages endpoint answering every request with one text turn whose
/// `message_start.message.usage` carries `start_extra` and whose `message_delta.usage` is
/// `end_usage`.
fn serve(start_extra: &str, end_usage: &str) -> String {
    let start_usage = if start_extra.is_empty() {
        format!("{{{START_USAGE}}}")
    } else {
        format!("{{{START_USAGE},{start_extra}}}")
    };
    let events = [
        (
            "message_start",
            format!(
                r#"{{"type":"message_start","message":{{"id":"msg_b10x_adv","type":"message","role":"assistant","model":"m","content":[],"stop_reason":null,"stop_sequence":null,"usage":{start_usage}}}}}"#
            ),
        ),
        ("ping", r#"{"type":"ping"}"#.to_owned()),
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

fn one_turn(
    start_extra: &str,
    end_usage: &str,
) -> (Result<TurnOutcome, WireError>, Vec<StreamEvent>) {
    let base_url = serve(start_extra, end_usage);
    let target = Target {
        protocol: Protocol::Messages,
        base_url: &base_url,
        model: "m",
        context_window: 1000,
        max_output_tokens: None,
    };
    let mut port = client(
        &target,
        Some(Source::key("synthetic-key").expect("a source")),
        &Cancel::new(),
    )
    .expect("a client");
    let mut sink = VecSink::new();
    let request = TurnRequest {
        model: "m".to_owned(),
        instructions: "Answer.".to_owned(),
        items: vec![Item::UserText {
            text: "What is six times seven?".to_owned(),
        }],
        tools: Vec::new(),
        max_output_tokens: None,
        sampling: Sampling::default(),
        tool_choice: ToolChoice::Auto,
    };
    let outcome = port.turn(&request, &mut sink);
    (outcome, sink.events().to_vec())
}

fn extensions(events: &[StreamEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::Warning { code, message } if code == llm_messages::USAGE_EXTENSION => {
                Some(message.clone())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_cost_on_message_start_alone_completes_with_one_warning_shown_before_the_answer() {
    let (plain, _) = one_turn("", r#"{"output_tokens":8}"#);
    let plain = plain.expect("the plain turn completes");
    let (costed, events) = one_turn(r#""cost":0.5"#, r#"{"output_tokens":8}"#);
    let costed = costed.unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(costed, plain, "the cost reads into nothing");
    assert_eq!(
        extensions(&events),
        vec!["message_start.message.usage.cost: 0.5".to_owned()]
    );
    let warning = events
        .iter()
        .position(|event| matches!(event, StreamEvent::Warning { .. }))
        .expect("a warning");
    let text = events
        .iter()
        .position(|event| matches!(event, StreamEvent::TextDelta { .. }))
        .expect("a text delta");
    assert!(warning < text, "held until the stream answers: {events:?}");
}

#[test]
fn a_cost_on_both_usage_objects_reaches_the_sink_twice_in_stream_order() {
    let (outcome, events) = one_turn(r#""cost":0.25"#, r#"{"output_tokens":8,"cost":0.5}"#);
    outcome.unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(
        extensions(&events),
        vec![
            "message_start.message.usage.cost: 0.25".to_owned(),
            "message_delta.usage.cost: 0.5".to_owned(),
        ]
    );
}

#[test]
fn every_non_null_cost_shape_is_carried_as_its_compact_value_and_null_is_absent() {
    for (cost, carried) in [
        ("-0.5", Some("-0.5")),
        ("0", Some("0")),
        (r#""0.0123""#, Some(r#""0.0123""#)),
        ("true", Some("true")),
        ("[1,2]", Some("[1,2]")),
        (
            r#"{"z":{"b":2,"a":1},"amount":3}"#,
            Some(r#"{"amount":3,"z":{"a":1,"b":2}}"#),
        ),
        ("null", None),
    ] {
        let (outcome, events) = one_turn("", &format!(r#"{{"output_tokens":8,"cost":{cost}}}"#));
        let outcome = outcome.unwrap_or_else(|error| panic!("cost {cost}: {error:?}"));
        assert_eq!(outcome.usage.as_ref().map(|u| u.output_tokens), Some(8));
        let expected: Vec<String> = carried
            .map(|value| format!("message_delta.usage.cost: {value}"))
            .into_iter()
            .collect();
        assert_eq!(extensions(&events), expected, "cost {cost}");
    }
}

#[test]
fn a_usage_key_beside_cost_still_ends_the_turn_after_the_answer_streamed() {
    // llm refuses the first unknown usage key in sorted order (`fields`, `object.keys().find`,
    // serde_json without `preserve_order`), so the live refusal that named
    // `message_delta.usage.cost` is equally what a usage object carrying `cost` *and* a key that
    // sorts after it produced. On 0.16.0 that object still ends the turn, after the text showed.
    let (outcome, events) = one_turn("", r#"{"output_tokens":8,"cost":0.5,"is_byok":false}"#);
    let error = outcome.expect_err("a sibling unknown key is still refused");
    assert_eq!(error.code, WireErrorCode::Unsupported, "{error:?}");
    assert!(
        error.message.contains("message_delta.usage.is_byok"),
        "{error:?}"
    );
    assert!(!error.retriable, "{error:?}");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, StreamEvent::TextDelta { .. })),
        "the answer had already reached the sink: {events:?}"
    );
    assert!(
        extensions(&events).is_empty(),
        "a refused object warns nothing"
    );
}

#[test]
fn a_cost_over_one_kibibyte_ends_the_turn_too_large_after_the_answer_streamed() {
    let big = format!(r#""{}""#, "x".repeat(1100));
    let (outcome, events) = one_turn("", &format!(r#"{{"output_tokens":8,"cost":{big}}}"#));
    let error = outcome.expect_err("refused by its bound");
    assert_eq!(error.code, WireErrorCode::TooLarge, "{error:?}");
    assert!(
        error.message.contains("message_delta.usage.cost"),
        "{error:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, StreamEvent::TextDelta { .. })),
        "{events:?}"
    );
}
