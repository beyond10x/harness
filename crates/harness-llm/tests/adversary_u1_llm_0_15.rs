//! Adversary cases for unit U1 of wave 2026-10-26 (`story:harness-builds-on-llm-0-15`), against
//! `a8d9e4e`. Each case drives the public surface, or a document the unit edited, against what
//! the unit's own `CHANGELOG.md` / `README.md` lines say about it.
//!
//! Every secret and every endpoint is synthetic and loopback (`AGENTS.md` invariant 17).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use harness_llm::{Protocol, Source, Target, client};
use harness_wire::{Cancel, Item, Sampling, ToolChoice, TurnRequest, VecSink, WireErrorCode};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(root().join(relative))
        .unwrap_or_else(|error| panic!("{relative}: {error}"))
}

// ---------------------------------------------------------------------------------------------
// The unit's own contract: CHANGELOG.md [Unreleased] quotes the refusal text.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_bedrock_converse_target_is_refused_with_the_text_the_changelog_quotes() {
    // CHANGELOG.md [Unreleased]: "`harness_llm::client` refuses it by name (`this build speaks
    // no bedrock-converse wire`), as it refuses `ChatCompletions`". The unit's own case only
    // asks for the substring `bedrock-converse`, which llm's binding refusal also carries.
    let target = Target {
        protocol: Protocol::BedrockConverse,
        base_url: "http://127.0.0.1:1/v1",
        model: "m",
        context_window: 1000,
        max_output_tokens: None,
    };
    let error = client(&target, None, &Cancel::new())
        .err()
        .expect("bedrock-converse is refused");
    assert_eq!(
        error, "this build speaks no bedrock-converse wire",
        "the refusal CHANGELOG.md quotes is the one an embedder gets"
    );
}

#[test]
fn a_chat_completions_target_is_refused_with_this_crates_own_text() {
    // The control for the case above: the sibling arm the CHANGELOG says Bedrock is refused
    // "as" is reached and answers its own text.
    let target = Target {
        protocol: Protocol::ChatCompletions,
        base_url: "http://127.0.0.1:1/v1",
        model: "m",
        context_window: 1000,
        max_output_tokens: None,
    };
    let error = client(&target, None, &Cancel::new())
        .err()
        .expect("chat-completions is refused");
    assert_eq!(error, "this build speaks no chat-completions wire");
}

// ---------------------------------------------------------------------------------------------
// Contract drift: documents that still describe the old pin as current, or the new pin as old.
// ---------------------------------------------------------------------------------------------

/// Every backticked `llm-*` / `b10x-llm-*` package name in `text`.
fn llm_package_names(text: &str) -> Vec<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .filter(|token| {
            (token.starts_with("llm-") || token.starts_with("b10x-llm-"))
                && token
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
        .map(str::to_owned)
        .collect()
}

#[test]
fn spec_coverage_names_only_llm_packages_the_lockfile_holds_at_the_pinned_tag() {
    // spec/COVERAGE.md "UNMAPPED: provider protocols and transport" and "credential lifecycle"
    // name the crates `crates/harness-llm/src/lib.rs` sits on, and the tag. After the move to
    // 0.15.0 those packages are `llm-*`, and Cargo.lock holds no `b10x-llm-*` (story acceptance).
    let coverage = read("spec/COVERAGE.md");
    let lock = read("Cargo.lock");
    let manifest = read("Cargo.toml");
    let tag = manifest
        .lines()
        .find(|line| line.starts_with("llm-core"))
        .and_then(|line| line.split("tag = \"").nth(1))
        .and_then(|rest| rest.split('"').next())
        .expect("Cargo.toml pins llm-core by tag")
        .to_owned();
    let mut stale = Vec::new();
    for name in llm_package_names(&coverage) {
        if !lock.contains(&format!("name = \"{name}\"")) {
            stale.push(name);
        }
    }
    for line in coverage.lines().filter(|line| line.contains("llm")) {
        if let Some(rest) = line.split("(tag `").nth(1) {
            let named = rest.split('`').next().unwrap_or_default();
            if named != tag {
                stale.push(format!("tag {named} (Cargo.toml pins {tag})"));
            }
        }
    }
    assert!(
        stale.is_empty(),
        "spec/COVERAGE.md cites llm packages or a tag this tree does not build on: {stale:?}"
    );
}

#[test]
fn the_readme_pins_it_dates_to_a_release_are_that_releases_pins() {
    // README.md: "At `<version>` those pins are … llm tag `<tag>` …". Where `<version>` is a
    // released tag, the sentence is checkable against that tag's Cargo.toml; where it is the
    // workspace's own version (the release commit, before its tag exists), against this tree's.
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
    let manifest = read("Cargo.toml");
    let workspace_version = manifest
        .lines()
        .find(|line| line.starts_with("version = \""))
        .and_then(|line| line.split('"').nth(1))
        .expect("the workspace declares a version")
        .to_owned();
    let released = if version == workspace_version {
        manifest
    } else {
        released_manifest(&version)
    };
    let pinned = released
        .lines()
        .find(|line| line.starts_with("llm-core"))
        .and_then(|line| line.split("tag = \"").nth(1))
        .and_then(|rest| rest.split('"').next())
        .expect("the release pins llm-core by tag")
        .to_owned();
    assert_eq!(
        claimed, pinned,
        "README.md says release `{version}` pins llm tag `{claimed}`; that release's Cargo.toml \
         pins `{pinned}`"
    );
}

fn released_manifest(version: &str) -> String {
    let shown = Command::new("git")
        .arg("-C")
        .arg(root())
        .args(["show", &format!("{version}:Cargo.toml")])
        .output()
        .expect("git runs");
    assert!(
        shown.status.success(),
        "the release `{version}` README.md dates its pins to is not a tag here; this check \
         refuses rather than skips: {}",
        String::from_utf8_lossy(&shown.stderr)
    );
    String::from_utf8_lossy(&shown.stdout).into_owned()
}

// ---------------------------------------------------------------------------------------------
// Behaviour llm changed between 0.8.1 and 0.15.0 that the loop's retry rests on, and that no
// harness case covered: a 429 that reports an exhausted quota or spend cap is final (llm 0.8.2).
// ---------------------------------------------------------------------------------------------

/// A loopback endpoint that answers every request with `status` and the JSON `body`, counting
/// requests. It keeps nothing a request carried.
struct Refusing {
    base_url: String,
    requests: Arc<AtomicUsize>,
}

impl Refusing {
    fn start(status: u16, body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let base_url = format!("http://{}/v1", listener.local_addr().expect("bound"));
        let requests = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&requests);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                counted.fetch_add(1, Ordering::SeqCst);
                answer(stream, status, body);
            }
        });
        Self { base_url, requests }
    }

    fn target(&self, protocol: Protocol) -> Target<'_> {
        Target {
            protocol,
            base_url: &self.base_url,
            model: "m",
            context_window: 1000,
            max_output_tokens: None,
        }
    }
}

fn answer(stream: TcpStream, status: u16, body: &str) {
    let mut reader = BufReader::new(stream);
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
    let mut stream = reader.into_inner();
    let _ = write!(
        stream,
        "HTTP/1.0 {status} Status\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
}

fn request() -> TurnRequest {
    TurnRequest {
        model: "m".to_owned(),
        instructions: "Answer.".to_owned(),
        items: vec![Item::UserText {
            text: "hello".to_owned(),
        }],
        tools: Vec::new(),
        max_output_tokens: None,
        sampling: Sampling::default(),
        tool_choice: ToolChoice::Auto,
    }
}

fn one_turn(endpoint: &Refusing, protocol: Protocol) -> harness_wire::WireError {
    let mut port = client(
        &endpoint.target(protocol),
        Some(Source::key("synthetic-key").expect("a source")),
        &Cancel::new(),
    )
    .expect("a client");
    port.turn(&request(), &mut VecSink::new())
        .expect_err("a 429 ends the turn")
}

#[test]
fn a_messages_spend_cap_429_reaches_the_loop_final_and_a_plain_429_retriable() {
    let capped = Refusing::start(
        429,
        r#"{"type":"error","error":{"type":"rate_limit_error","message":"spend cap","details":{"error_code":"enforced_spend_limit_reached"}}}"#,
    );
    let error = one_turn(&capped, Protocol::Messages);
    assert_eq!(
        (error.code, error.retriable),
        (WireErrorCode::Refused, false),
        "llm 0.8.2: a spend-cap 429 is a final refusal, so the loop does not spend three retries \
         on it: {error:?}"
    );
    assert_eq!(capped.requests.load(Ordering::SeqCst), 1);

    let limited = Refusing::start(
        429,
        r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}"#,
    );
    let error = one_turn(&limited, Protocol::Messages);
    assert_eq!(
        (error.code, error.retriable),
        (WireErrorCode::RateLimited, true),
        "every other 429 stays retriable: {error:?}"
    );
}

#[test]
fn a_codex_usage_limit_429_reaches_the_loop_final_and_a_plain_429_retriable() {
    let capped = Refusing::start(
        429,
        r#"{"error":{"type":"usage_limit_reached","message":"usage limit"}}"#,
    );
    let error = one_turn(&capped, Protocol::Responses);
    assert_eq!(
        (error.code, error.retriable),
        (WireErrorCode::Refused, false),
        "llm 0.8.2: a Codex usage-limit 429 is final: {error:?}"
    );
    assert_eq!(capped.requests.load(Ordering::SeqCst), 1);

    let limited = Refusing::start(
        429,
        r#"{"error":{"type":"requests","code":"rate_limit_exceeded","message":"slow down"}}"#,
    );
    let error = one_turn(&limited, Protocol::Responses);
    assert_eq!(
        (error.code, error.retriable),
        (WireErrorCode::RateLimited, true),
        "every other 429 stays retriable: {error:?}"
    );
}
