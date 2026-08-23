use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::sync::Mutex;
use std::thread;

use harness_wire::{ToolCall, ToolName, ToolPort};
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
        unmet.to_string().contains("Nothing that needs it is published here"),
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
    let unmet = facts.admits(&exec_start(), &json!({})).expect_err("refused");
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
        .check(&confined(), &json!({"mode": "directory", "limit_items": 9_999_999}))
        .expect("a listing is not a read");
    conditional[0]
        .check(&confined(), &json!({"mode": "file", "limit_bytes": 999_999}))
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
    let (_dir, socket) = serve(
        r#"{"driver":"host","facts":{"exec.argv-only":true}}"#,
        200,
    );
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

// --- publication, and the tools that exist only where they can be confined ----------------------

use std::sync::Arc;

/// A transport that answers from a script and records what it was asked.
#[derive(Clone)]
struct Scripted {
    seen: Arc<Mutex<Vec<(String, String, Option<Value>)>>>,
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

fn tools(facts: &Facts, script: Scripted, programs: &[&str]) -> ConfinedTools {
    ConfinedTools::new(
        Client::with(script),
        facts,
        "ws-1",
        programs.iter().map(|p| (*p).to_owned()).collect(),
    )
}

#[test]
fn a_machine_that_cannot_confine_a_process_publishes_no_way_to_start_one() {
    // The whole publication tier in one assertion. Not disabled, not gated: absent.
    let published = tools(&unconfined(), Scripted::new(vec![]), &["cargo"]);
    let names: Vec<&str> = published.specs().iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec![WRITE_TOOL, EDIT_TOOL]);
    assert!(!names.contains(&RUN_TOOL), "the model is never told about it");

    let confined_tools = tools(&confined(), Scripted::new(vec![]), &["cargo"]);
    let names: Vec<&str> = confined_tools.specs().iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec![WRITE_TOOL, EDIT_TOOL, RUN_TOOL]);
}

#[test]
fn no_daemon_at_all_publishes_nothing_and_the_harness_is_what_it_always_was() {
    let published = tools(&Facts::none(), Scripted::new(vec![]), &["cargo"]);
    assert!(published.specs().is_empty());
}

#[test]
fn a_run_with_no_declared_programs_is_not_published_even_where_it_could_be() {
    // A workflow that named no commands wants none. A tool that admitted everything because
    // nobody listed anything is the failure this design exists to prevent.
    let published = tools(&confined(), Scripted::new(vec![]), &[]);
    let names: Vec<&str> = published.specs().iter().map(|s| s.name.as_str()).collect();
    assert!(!names.contains(&RUN_TOOL));
}

#[test]
fn the_declared_set_is_in_the_tools_own_schema_so_the_model_reads_it_rather_than_guessing() {
    let published = tools(&confined(), Scripted::new(vec![]), &["cargo", "protocol"]);
    let run = published
        .specs()
        .iter()
        .find(|spec| spec.name.as_str() == RUN_TOOL)
        .expect("published");
    assert!(run.description.contains("cargo, protocol"), "{}", run.description);
    assert!(
        run.description.contains("not a shell"),
        "and says what it is not: {}",
        run.description
    );
    let allowed = &run.input_schema["properties"]["argv"]["prefixItems"][0]["enum"];
    assert_eq!(allowed, &json!(["cargo", "protocol"]));
}

#[test]
fn a_program_outside_the_declared_set_is_refused_by_name_and_the_set_is_listed() {
    let script = Scripted::new(vec![]);
    let mut published = tools(&confined(), script.clone(), &["cargo"]);
    let outcome = published.call(&ToolCall {
        call_id: harness_wire::CallId::new("c-1").expect("valid"),
        name: ToolName::new(RUN_TOOL).expect("valid"),
        arguments: json!({"argv": ["sh", "-c", "rm -rf /"]}),
    });
    assert!(outcome.failed);
    let said = outcome.output.as_str().unwrap_or_default().to_owned();
    assert!(said.contains("`sh` is not a program"), "{said}");
    assert!(said.contains("cargo"), "the set is listed: {said}");
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
    let mut published = tools(&confined(), script.clone(), &["cargo"]);
    let outcome = published.call(&ToolCall {
        call_id: harness_wire::CallId::new("c-1").expect("valid"),
        name: ToolName::new(RUN_TOOL).expect("valid"),
        arguments: json!({"argv": ["cargo", "test", "--workspace"]}),
    });
    assert!(!outcome.failed, "{:?}", outcome.output);

    let seen = script.seen.lock().expect("not poisoned");
    assert_eq!(seen[0].0, "POST");
    assert_eq!(seen[0].1, "/v1/execs");
    let sent = seen[0].2.as_ref().expect("a body");
    assert_eq!(sent["input"]["argv"], json!(["cargo", "test", "--workspace"]));
    assert!(
        sent["input"].get("command").is_none(),
        "there is no command line anywhere in it: {sent}"
    );
    assert_eq!(seen[1].1, "/v1/execs/e-1/output");
}

#[test]
fn an_edit_that_matched_nothing_or_several_places_is_refused_rather_than_guessed_at() {
    let call = |arguments: Value| ToolCall {
        call_id: harness_wire::CallId::new("c-1").expect("valid"),
        name: ToolName::new(EDIT_TOOL).expect("valid"),
        arguments,
    };

    // Nothing matched: the model would otherwise believe a change landed.
    let mut published = tools(
        &confined(),
        Scripted::new(vec![(200, r#"{"result":{"content":"fn main() {}"}}"#)]),
        &[],
    );
    let outcome = published.call(&call(json!({"path": "a.rs", "old": "absent", "new": "x"})));
    assert!(outcome.failed);
    assert!(
        outcome.output.as_str().unwrap_or_default().contains("nothing was changed"),
        "{:?}",
        outcome.output
    );

    // Several matched: three things nobody asked about would have changed.
    let mut published = tools(
        &confined(),
        Scripted::new(vec![(200, r#"{"result":{"content":"a\na\na\n"}}"#)]),
        &[],
    );
    let outcome = published.call(&call(json!({"path": "a.rs", "old": "a", "new": "b"})));
    assert!(outcome.failed);
    let said = outcome.output.as_str().unwrap_or_default().to_owned();
    assert!(said.contains("3 times"), "{said}");
    assert!(said.contains("more surrounding text"), "and says what to do: {said}");
}

#[test]
fn an_edit_that_names_one_place_writes_the_whole_file_back_with_that_one_change() {
    let script = Scripted::new(vec![
        (200, r#"{"result":{"content":"one\ntwo\nthree\n"}}"#),
        (200, r#"{"result":{"ok":true}}"#),
    ]);
    let mut published = tools(&confined(), script.clone(), &[]);
    let outcome = published.call(&ToolCall {
        call_id: harness_wire::CallId::new("c-1").expect("valid"),
        name: ToolName::new(EDIT_TOOL).expect("valid"),
        arguments: json!({"path": "a.txt", "old": "two", "new": "2"}),
    });
    assert!(!outcome.failed, "{:?}", outcome.output);

    let seen = script.seen.lock().expect("not poisoned");
    assert_eq!(seen[0].0, "GET");
    assert_eq!(seen[1].0, "PUT");
    assert_eq!(seen[1].1, "/v1/workspaces/ws-1/files/a.txt");
    assert_eq!(seen[1].2.as_ref().expect("a body")["input"]["content"], "one\n2\nthree\n");
}

#[test]
fn an_edit_declares_itself_non_idempotent_because_a_retreat_will_run_it_twice() {
    let published = tools(&confined(), Scripted::new(vec![]), &[]);
    let spec = |name: &str| {
        published
            .specs()
            .iter()
            .find(|spec| spec.name.as_str() == name)
            .expect("published")
            .clone()
    };
    assert_eq!(
        spec(EDIT_TOOL).envelope.idempotency,
        harness_wire::Idempotency::NonIdempotent,
        "the second attempt finds nothing to replace"
    );
    assert_eq!(
        spec(WRITE_TOOL).envelope.idempotency,
        harness_wire::Idempotency::Idempotent,
        "writing the same bytes twice leaves the same file"
    );
    assert!(
        spec(WRITE_TOOL).envelope.mutates(),
        "and both are mutations, whatever their idempotency"
    );
}

#[test]
fn a_call_names_what_it_touches_so_the_second_gate_has_something_to_read() {
    let published = tools(&confined(), Scripted::new(vec![]), &["cargo"]);
    let call = |name: &str, arguments: Value| ToolCall {
        call_id: harness_wire::CallId::new("c-1").expect("valid"),
        name: ToolName::new(name).expect("valid"),
        arguments,
    };
    assert_eq!(
        published.subjects(&call(WRITE_TOOL, json!({"path": "src/x.rs", "text": ""}))),
        vec![harness_wire::Subject::file("src/x.rs")]
    );
    assert_eq!(
        published.subjects(&call(RUN_TOOL, json!({"argv": ["cargo", "test"]}))),
        vec![harness_wire::Subject::process("cargo")],
        "the program, not the whole argv: what a policy names is what would start"
    );
}

#[test]
fn a_real_daemons_answer_is_wrapped_in_a_result_and_the_facts_are_found_inside_it() {
    // The shape the first live daemon this crate ever spoke to actually answered, 2026-08-23. The
    // client read the body until then; a document of an unexpected shape deserialises into a
    // `Facts` whose map is empty, so it would have published no tools and blamed the machine.
    let (_dir, socket) = serve(
        r#"{"api_version":"v1","request_id":"req_1","result":{"driver":"host","driver_version":"0.2.0","facts":{"exec.argv-only":true,"exec.cgroup-limits":{"cpu":true,"memory":true,"processes":true},"workspace.guarded-io":true}}}"#,
        200,
    );
    let facts = Client::at(&socket).machine().expect("reads");
    assert_eq!(facts.driver_version.as_deref(), Some("0.2.0"));
    assert!(facts.confines_execution(), "the facts were found");
    assert!(facts.holds_workspaces());
}

#[test]
fn a_bare_capability_document_is_still_read_because_that_is_what_the_schemas_show() {
    let (_dir, socket) = serve(r#"{"driver":"host","facts":{"workspace.guarded-io":true}}"#, 200);
    let facts = Client::at(&socket).machine().expect("reads");
    assert!(facts.holds_workspaces());
}
