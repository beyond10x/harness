use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::sync::Mutex;
use std::thread;

use serde_json::{Value, json};

use super::*;

/// The facts a Linux deployment with a delegated cgroup root reports, in the shape the wire
/// contract's `capability` document declares.
fn confined() -> Facts {
    serde_json::from_value(json!({
        "driver": "host",
        "driver_version": "0.4.0",
        "facts": {
            "workspace.guarded-io": true,
            "workspace.openat2-beneath": true,
            "workspace.atomic-replace": true,
            "workspace.read-limit-bytes": 262_144,
            "exec.argv-only": true,
            "exec.cgroup-kill": true,
            "exec.cgroup-limits": {"cpu": true, "memory": true, "processes": true},
            "exec.output-limit-bytes": 65_536,
            "exec.signals": ["SIGTERM", "SIGKILL"],
        }
    }))
    .expect("the fixture is a capability document")
}

/// The same daemon on a host with no delegated cgroup root: workspaces served, exec facts absent.
fn unconfined() -> Facts {
    serde_json::from_value(json!({
        "driver": "host",
        "driver_version": "0.4.0",
        "facts": {
            "workspace.guarded-io": true,
            "workspace.openat2-beneath": true,
            "workspace.atomic-replace": true,
        }
    }))
    .expect("the fixture is a capability document")
}

/// `exec.start`'s predicates, copied from the 0.4.0 operations document.
fn exec_start() -> Vec<Predicate> {
    serde_json::from_value(json!([
        {"fact": "exec.argv-only", "op": "eq", "value": true},
        {"fact": "exec.cgroup-limits", "op": "eq",
         "value": {"cpu": true, "memory": true, "processes": true}},
    ]))
    .expect("the fixture is a predicate list")
}

#[test]
fn a_machine_with_a_delegated_cgroup_root_admits_execution_and_one_without_does_not() {
    assert!(confined().confines_execution());
    assert!(!unconfined().confines_execution());

    // And the difference is not about workspaces: substrate serves those either way, so a harness
    // on the second machine gets the write tools and not the `run` tool.
    assert!(confined().holds_workspaces());
    assert!(unconfined().holds_workspaces());
}

#[test]
fn an_absent_fact_never_passes_a_predicate() {
    // The rule the whole publication tier rests on. *The machine did not say* and *the machine said
    // yes* are the two answers a gate must never confuse, and substrate's own position is that
    // missing confinement facts mean the operation is unavailable.
    let unmet = unconfined()
        .admits(&exec_start(), &json!({}))
        .expect_err("refused");
    assert_eq!(unmet.fact, "exec.argv-only");
    assert_eq!(unmet.found, "nothing");
    assert!(
        unmet
            .to_string()
            .contains("Nothing that needs it is published here"),
        "{unmet}"
    );

    confined()
        .admits(&exec_start(), &json!({}))
        .expect("a confined machine admits it");
}

#[test]
fn a_fact_that_is_false_is_as_unmet_as_one_that_is_missing() {
    let mut facts = confined();
    facts
        .facts
        .insert("exec.argv-only".to_owned(), Value::Bool(false));
    let unmet = facts
        .admits(&exec_start(), &json!({}))
        .expect_err("refused");
    assert_eq!(unmet.found, "false");
}

#[test]
fn a_ceiling_is_checked_against_the_number_the_request_actually_asks_for() {
    let ceiling: Vec<Predicate> = serde_json::from_value(json!([
        {"fact": "exec.output-limit-bytes", "input_pointer": "/limit_bytes", "op": "gte"}
    ]))
    .expect("predicates");

    confined()
        .admits(&ceiling, &json!({"limit_bytes": 65_536}))
        .expect("exactly the ceiling is within it");
    let unmet = confined()
        .admits(&ceiling, &json!({"limit_bytes": 65_537}))
        .expect_err("one byte over is not");
    assert!(unmet.wanted.contains("65537"), "{unmet}");
    assert_eq!(unmet.found, "65536");

    // A request that asks for no ceiling cannot exceed one.
    confined()
        .admits(&ceiling, &json!({}))
        .expect("nothing asked for, nothing exceeded");
}

#[test]
fn a_predicate_that_lists_admitted_values_refuses_one_it_does_not_list() {
    let signals: Vec<Predicate> = serde_json::from_value(json!([
        {"fact": "exec.signals", "input_pointer": "/signal", "op": "one_of"}
    ]))
    .expect("predicates");
    confined()
        .admits(&signals, &json!({"signal": "SIGKILL"}))
        .expect("listed");
    let unmet = confined()
        .admits(&signals, &json!({"signal": "SIGHUP"}))
        .expect_err("not listed");
    assert_eq!(unmet.fact, "exec.signals");
}

#[test]
fn a_conditional_predicate_does_not_apply_when_its_condition_is_not_met() {
    // `workspace.file-read` carries one per mode: a directory listing must not be held to the file
    // read's byte ceiling.
    let conditional: Vec<Predicate> = serde_json::from_value(json!([
        {"fact": "workspace.read-limit-bytes", "input_pointer": "/limit_bytes", "op": "gte",
         "when": {"input_pointer": "/mode", "equals": "file"}}
    ]))
    .expect("predicates");

    conditional[0]
        .check(
            &confined(),
            &json!({"mode": "directory", "limit_items": 9_999_999}),
        )
        .expect("a listing is not a read");
    conditional[0]
        .check(
            &confined(),
            &json!({"mode": "file", "limit_bytes": 999_999}),
        )
        .expect_err("and a read is");
}

#[test]
fn an_unknown_fact_survives_being_read_so_a_newer_daemon_stays_readable() {
    // A client that refused a document for having more in it than it knows about would make every
    // daemon upgrade a client outage.
    let facts: Facts = serde_json::from_value(json!({
        "driver": "host",
        "facts": {"exec.argv-only": true, "exec.something-invented-later": {"deep": [1, 2]}}
    }))
    .expect("reads");
    assert!(facts.get("exec.something-invented-later").is_some());
}

// --- the transport ------------------------------------------------------------------------------

/// A one-request Unix-socket server that answers with what the test hands it.
fn serve(body: &'static str, status: u16) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let socket = dir.path().join("substrate.sock");
    let listener = UnixListener::bind(&socket).expect("binds");
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buffer = [0_u8; 2048];
            let _ = stream.read(&mut buffer);
            let response = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (dir, socket)
}

#[test]
fn the_client_reads_a_capability_document_off_a_real_socket() {
    let (_dir, socket) = serve(r#"{"driver":"host","facts":{"exec.argv-only":true}}"#, 200);
    let facts = Client::at(&socket).machine().expect("reads");
    assert_eq!(facts.driver.as_deref(), Some("host"));
    assert_eq!(facts.get("exec.argv-only"), Some(&Value::Bool(true)));
}

#[test]
fn a_daemon_that_refuses_is_reported_with_its_status_and_its_words() {
    let (_dir, socket) = serve(r#"{"error":"exec.sandbox-unavailable"}"#, 503);
    let error = Client::at(&socket).machine().expect_err("refused");
    let message = error.to_string();
    assert!(message.contains("503"), "{message}");
    assert!(
        message.contains("exec.sandbox-unavailable"),
        "the daemon's own words survive: {message}"
    );
}

#[test]
fn no_daemon_at_all_is_a_machine_that_admits_nothing_rather_than_a_failed_launch() {
    // A harness with no substrate is a harness whose confined tools do not exist - which is how
    // this component has run since it was written. A probe that failed the launch would make the
    // read-only harness unlaunchable on a machine that never wanted the other tools.
    let dir = tempfile::tempdir().expect("a temporary directory");
    let client = Client::at(dir.path().join("nothing-listens-here.sock"));

    let facts = client.probe().expect("an absent daemon is not an error");
    assert_eq!(facts, Facts::none());
    assert!(!facts.confines_execution());
    assert!(!facts.holds_workspaces());

    // ...and it is still an error to *ask* directly, so a caller that needs one can find out.
    assert!(matches!(
        client.machine().expect_err("unreachable"),
        SubstrateError::Unreachable { .. }
    ));
}

#[test]
fn a_daemon_that_answers_something_this_build_cannot_read_is_an_error_and_not_an_absence() {
    // A broken deployment is not the same as no deployment, and reporting it as *nothing is
    // confined here* would hide a daemon that needs looking at.
    let (_dir, socket) = serve("not json at all", 200);
    assert!(matches!(
        Client::at(&socket).probe().expect_err("unreadable"),
        SubstrateError::Unreadable { .. }
    ));
}

/// A transport that records what it was asked for, so the request shape is checkable without a
/// socket.
struct Recorder(Mutex<Vec<(String, String)>>);

impl Transport for Recorder {
    fn request(
        &self,
        method: &str,
        path: &str,
        _body: Option<&Value>,
    ) -> Result<(u16, String), SubstrateError> {
        self.0
            .lock()
            .expect("not poisoned")
            .push((method.to_owned(), path.to_owned()));
        Ok((200, r#"{"facts":{}}"#.to_owned()))
    }
}

#[test]
fn the_probe_asks_the_one_route_the_contract_names() {
    let recorder = Recorder(Mutex::new(Vec::new()));
    let client = Client::with(recorder);
    client.probe().expect("answers");
    // Reaching into the transport is not possible once it is boxed, so the assertion is that the
    // call succeeded against a transport that only answers `/v1/machine`-shaped documents. The
    // route itself is pinned by the socket test above.
    assert!(client.machine().is_ok());
}

// --- what a confined workspace admits, and what the catalogue makes of it ----------------------

use harness_tools::{Catalogue, Operations};
use std::sync::Arc;

/// One thing the scripted transport was asked: method, path, and the body if there was one.
type Asked = (String, String, Option<Value>);

/// A transport that answers from a script and records what it was asked.
#[derive(Clone)]
struct Scripted {
    seen: Arc<Mutex<Vec<Asked>>>,
    answers: Arc<Mutex<Vec<(u16, String)>>>,
}

impl Scripted {
    fn new(answers: Vec<(u16, &str)>) -> Self {
        Self {
            seen: Arc::new(Mutex::new(Vec::new())),
            answers: Arc::new(Mutex::new(
                answers
                    .into_iter()
                    .map(|(status, body)| (status, body.to_owned()))
                    .collect(),
            )),
        }
    }
}

impl Transport for Scripted {
    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, String), SubstrateError> {
        self.seen.lock().expect("not poisoned").push((
            method.to_owned(),
            path.to_owned(),
            body.cloned(),
        ));
        let mut answers = self.answers.lock().expect("not poisoned");
        if answers.is_empty() {
            return Ok((200, "{}".to_owned()));
        }
        Ok(answers.remove(0))
    }
}

fn provider(facts: &Facts, script: Scripted, programs: &[&str]) -> ConfinedOperations {
    ConfinedOperations::new(
        Client::with(script),
        facts,
        "ws-1",
        programs.iter().map(|p| (*p).to_owned()).collect(),
    )
}

fn entry_names(catalogue: &Catalogue) -> Vec<&'static str> {
    catalogue.entries().iter().map(|entry| entry.name).collect()
}

#[test]
fn a_machine_that_cannot_confine_a_process_contributes_no_way_to_start_one() {
    // The publication gate, in one assertion. The model is never told about a tool it cannot have.
    let entries = entry_names(&Catalogue::of(provider(
        &unconfined(),
        Scripted::new(vec![]),
        &["cargo"],
    )));
    assert!(entries.contains(&"file_write"), "{entries:?}");
    assert!(!entries.contains(&"run"), "{entries:?}");

    let entries = entry_names(&Catalogue::of(provider(
        &confined(),
        Scripted::new(vec![]),
        &["cargo"],
    )));
    assert!(entries.contains(&"run"), "{entries:?}");
}

#[test]
fn no_backend_at_all_contributes_nothing_that_outlives_a_call() {
    let entries = entry_names(&Catalogue::of(provider(
        &Facts::none(),
        Scripted::new(vec![]),
        &["cargo"],
    )));
    assert_eq!(entries, vec!["file_read", "dir_list", "search"]);
}

/// A backend answering one file of `bytes` bytes.
fn a_file_of(bytes: usize) -> Scripted {
    let data = base64_of(&"x".repeat(bytes));
    Scripted::new(vec![(
        200,
        Box::leak(format!(r#"{{"content":{{"data":"{data}"}}}}"#).into_boxed_str()),
    )])
}

/// The backend answers base64, so a fixture has to speak it.
fn base64_of(text: &str) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = text.as_bytes();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[test]
fn a_confined_read_is_bounded_and_says_so_rather_than_replaying_a_whole_large_file() {
    // A result is replayed on every later turn. A live run read three files in one turn and the
    // next turn's replay grew by 24,630 tokens, which pushed the conversation past its bound.
    let answer = provider(&confined(), a_file_of(200_000), &["cargo"])
        .file_read("big.txt", None)
        .expect("read");

    assert_eq!(answer["bytes"], 200_000, "the whole size is still reported");
    assert_eq!(
        answer["truncated"], true,
        "a partial read never looks whole"
    );
    assert_eq!(
        answer["text"].as_str().expect("text").len(),
        64 * 1024,
        "bounded at the same figure the unconfined provider uses"
    );
}

#[test]
fn a_confined_read_the_caller_bounded_more_tightly_is_answered_at_that_bound() {
    let answer = provider(&confined(), a_file_of(10_000), &["cargo"])
        .file_read("big.txt", Some(100))
        .expect("read");

    assert_eq!(answer["text"].as_str().expect("text").len(), 100);
    assert_eq!(answer["truncated"], true);
}

#[test]
fn a_confined_read_inside_the_bound_is_answered_whole_and_says_it_is_whole() {
    let answer = provider(&confined(), a_file_of(120), &["cargo"])
        .file_read("small.txt", None)
        .expect("read");

    assert_eq!(answer["truncated"], false);
    assert_eq!(answer["text"].as_str().expect("text").len(), 120);
}

#[test]
fn a_run_with_no_declared_programs_is_not_offered_even_where_it_could_be() {
    let entries = entry_names(&Catalogue::of(provider(
        &confined(),
        Scripted::new(vec![]),
        &[],
    )));
    assert!(!entries.contains(&"run"), "{entries:?}");
}

#[test]
fn a_program_outside_the_declared_set_is_refused_locally_and_the_set_is_listed() {
    let script = Scripted::new(vec![]);
    let confined_provider = provider(&confined(), script.clone(), &["cargo"]);
    let refused = confined_provider
        .run(&["sh".to_owned(), "-c".to_owned(), "rm -rf /".to_owned()])
        .expect_err("refused");
    assert!(refused.contains("`sh` is not a program"), "{refused}");
    assert!(refused.contains("cargo"), "the set is listed: {refused}");
    assert!(
        script.seen.lock().expect("not poisoned").is_empty(),
        "and nothing was sent: the refusal is local"
    );
}

#[test]
fn a_declared_program_is_sent_as_an_argv_and_never_as_a_command_line() {
    let script = Scripted::new(vec![
        (200, r#"{"result":{"exec_id":"e-1"}}"#),
        (200, r#"{"result":{"exit_status":0,"stdout":"ok"}}"#),
    ]);
    let confined_provider = provider(&confined(), script.clone(), &["cargo"]);
    confined_provider
        .run(&["cargo".to_owned(), "test".to_owned()])
        .expect("ran");

    let seen = script.seen.lock().expect("not poisoned");
    assert_eq!(
        (seen[0].0.as_str(), seen[0].1.as_str()),
        ("POST", "/v1/execs")
    );
    let sent = seen[0].2.as_ref().expect("a body");
    assert_eq!(sent["input"]["argv"], json!(["cargo", "test"]));
    assert!(
        sent["input"].get("command").is_none(),
        "there is no command line anywhere in it: {sent}"
    );
}

#[test]
fn an_edit_that_matched_nothing_or_several_places_is_refused_rather_than_guessed_at() {
    let content = |text: &str| {
        format!(
            r#"{{"result":{{"content":{{"encoding":"base64","data":"{}"}}}}}}"#,
            crate::base64::encode(text.as_bytes())
        )
    };

    let none = Scripted::new(vec![(
        200,
        Box::leak(content("fn main() {}").into_boxed_str()),
    )]);
    let refused = provider(&confined(), none, &[])
        .file_edit("a.rs", "absent", "x")
        .expect_err("refused");
    assert!(refused.contains("nothing was changed"), "{refused}");

    let several = Scripted::new(vec![(
        200,
        Box::leak(content("a\na\na\n").into_boxed_str()),
    )]);
    let refused = provider(&confined(), several, &[])
        .file_edit("a.rs", "a", "b")
        .expect_err("refused");
    assert!(refused.contains("3 times"), "{refused}");
    assert!(refused.contains("more surrounding text"), "{refused}");
}

#[test]
fn an_edit_that_names_one_place_writes_the_whole_file_back_with_that_one_change() {
    let body = format!(
        r#"{{"result":{{"content":{{"encoding":"base64","data":"{}"}}}}}}"#,
        crate::base64::encode(b"one\ntwo\nthree\n")
    );
    let script = Scripted::new(vec![
        (200, Box::leak(body.into_boxed_str())),
        (200, r#"{"result":{"ok":true}}"#),
    ]);
    provider(&confined(), script.clone(), &[])
        .file_edit("a.txt", "two", "2")
        .expect("edited");

    let seen = script.seen.lock().expect("not poisoned");
    assert_eq!(seen[0].0, "GET");
    assert_eq!(seen[1].0, "PUT");
    let sent = &seen[1].2.as_ref().expect("a body")["input"]["content"];
    assert_eq!(sent["encoding"], "base64");
    assert_eq!(
        String::from_utf8(
            crate::base64::decode(sent["data"].as_str().expect("data")).expect("decodes")
        )
        .expect("text"),
        "one\n2\nthree\n"
    );
}

#[test]
fn a_confined_workspace_lists_and_searches_through_nothing_and_says_so() {
    // `Backend` carries no listing route, and reading the host filesystem to fake one would step
    // around the containment this provider exists for. A run that needs a listing gets it from the
    // reading provider beside this one, which is what `harness_tools::Split` composes.
    let confined_provider = provider(&confined(), Scripted::new(vec![]), &[]);
    for refused in [
        confined_provider.dir_list("."),
        confined_provider.search("fn", ".", None),
    ] {
        assert!(
            refused
                .expect_err("refused")
                .contains("is not offered by this workspace")
        );
    }
}

#[test]
fn an_exec_identity_is_the_shape_substrate_admits() {
    // `admit` requires `^ex_[A-Za-z0-9_]+$`. This was `exec-<pid>-<argv joined by dashes>` - wrong
    // prefix, and a program path is full of `/` and `.` - so **every exec was refused before it
    // started**, for the whole life of the embedded driver, and quietly: a refused tool call looks
    // like a failed tool call to the model.
    //
    // A live run asked to fix a failing suite had all three of its `run` calls refused, edited the
    // file anyway, and reported the suite passing. The file was right and nothing had executed.
    let first = super::embedded::exec_identity(4242, 0);
    let second = super::embedded::exec_identity(4242, 1);

    for id in [&first, &second] {
        assert!(id.starts_with("ex_"), "{id}");
        assert!(
            id.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
            "{id}"
        );
    }
    assert_ne!(
        first, second,
        "substrate keys an execution's output and lifetime on this, so two calls sharing one \
         would read each other's"
    );
}

#[test]
fn a_declared_rust_toolchain_never_mounts_the_operators_cargo_credential() {
    // `~/.cargo` holds `credentials.toml` - a registry publishing token - beside the package
    // cache. It was mounted whole for one commit, which handed every confined run the operator's
    // credential. Nothing about a build needs it, and a confinement that leaks one is not a
    // confinement.
    let Ok(toolchain) = super::Toolchain::rust(
        std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .as_deref(),
    ) else {
        // No toolchain on this machine; the rule below is still the rule.
        return;
    };
    for root in toolchain.roots() {
        assert!(
            !root.host_path.ends_with("/.cargo"),
            "`{}` would carry credentials.toml into the sandbox",
            root.host_path
        );
    }
    // And cargo's home is somewhere it may actually write: it takes a `.package-cache` lock there
    // before doing anything, and against a read-only mount it blocks forever with no output.
    assert_eq!(
        toolchain.env().get("CARGO_HOME").map(String::as_str),
        Some("/workspace/.cargo")
    );
}
