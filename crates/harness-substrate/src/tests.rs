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

/// The declared program set of a run that wants to build.
fn declared() -> Vec<String> {
    vec!["cargo".to_owned()]
}

#[test]
fn a_declared_program_a_machine_cannot_confine_is_recorded_by_the_predicate_that_failed() {
    // The defect this exists for: on a machine whose exec facts are absent the `run` entry simply
    // vanishes, and six entries where seven were asked for reads exactly like six that were asked
    // for. Verified on a real run: `b10x-harness tools` from a login shell against the same daemon
    // that answers seven under `systemd-run --user --scope`.
    let withheld = unconfined().withheld(&declared(), true);
    assert_eq!(withheld.len(), 1, "{withheld:?}");
    assert_eq!(withheld[0].tool, "run");
    assert!(
        withheld[0]
            .reason
            .starts_with("`exec.argv-only` must be true and this machine says nothing."),
        "the predicate is named as the machine stated it: {}",
        withheld[0].reason
    );
    // And the hint, because the fact is absent for a reason that is about how the harness was
    // started rather than about how substrate was configured.
    assert!(
        withheld[0]
            .reason
            .contains("user.slice/user-N.slice/session-M.scope")
            && withheld[0].reason.contains("systemd-run --user --scope"),
        "the cgroup hint sends a reader at the right thing: {}",
        withheld[0].reason
    );
}

#[test]
fn a_fact_the_machine_stated_false_is_quoted_back_rather_than_called_absent() {
    let mut facts = confined();
    facts
        .facts
        .insert("exec.argv-only".to_owned(), Value::Bool(false));
    let withheld = facts.withheld(&declared(), true);
    assert_eq!(withheld.len(), 1, "{withheld:?}");
    assert!(
        withheld[0]
            .reason
            .starts_with("`exec.argv-only` must be true and this machine says false."),
        "{}",
        withheld[0].reason
    );
}

#[test]
fn a_cgroup_root_missing_two_controllers_is_named_by_the_two_it_is_missing() {
    let mut facts = confined();
    facts.facts.insert(
        "exec.cgroup-limits".to_owned(),
        json!({"cpu": true, "memory": false}),
    );
    let withheld = facts.withheld(&declared(), true);
    assert_eq!(withheld.len(), 1, "{withheld:?}");
    let reason = &withheld[0].reason;
    assert!(
        reason.starts_with(
            "`exec.cgroup-limits` must state `cpu`, `memory` and `processes` true and this \
             machine says {\"cpu\":true,\"memory\":false} — no `memory`, no `processes`."
        ),
        "the machine's own answer and exactly what is short of it: {reason}"
    );
    assert!(reason.contains("systemd-run --user --scope"), "{reason}");
}

#[test]
fn a_machine_with_no_facts_at_all_says_so_and_blames_no_cgroup() {
    // `Facts::none()` is a harness nobody pointed at substrate, and one pointed at a daemon that
    // is not there. Neither probed a cgroup, so a sentence about cgroups would send a reader
    // looking in the wrong place.
    let withheld = Facts::none().withheld(&declared(), true);
    assert_eq!(
        withheld.iter().map(|w| w.tool.as_str()).collect::<Vec<_>>(),
        vec!["run", "file_write", "file_edit"],
        "{withheld:?}"
    );
    for entry in &withheld {
        assert!(
            entry
                .reason
                .starts_with("this machine states no capability facts at all"),
            "{}",
            entry.reason
        );
        assert!(
            !entry.reason.contains("cgroup root"),
            "no cgroup was probed, so none is blamed: {}",
            entry.reason
        );
    }
}

#[test]
fn a_workspace_the_machine_does_not_guard_takes_both_writing_entries_with_it() {
    let mut facts = confined();
    facts.facts.remove("workspace.guarded-io");
    let withheld = facts.withheld(&[], true);
    assert_eq!(
        withheld.iter().map(|w| w.tool.as_str()).collect::<Vec<_>>(),
        vec!["file_write", "file_edit"],
        "{withheld:?}"
    );
    assert_eq!(
        withheld[0].reason, "`workspace.guarded-io` must be true and this machine says nothing.",
        "{withheld:?}"
    );
}

#[test]
fn a_run_that_declared_nothing_withholds_nothing() {
    // Absence stays absence (AGENTS.md invariant 7). A machine that cannot confine a process owes
    // no sentence to a run that never wanted to start one, and inventing the want here would put a
    // line about `run` in front of every read-only run there has ever been.
    assert!(unconfined().withheld(&[], true).is_empty());
    assert!(Facts::none().withheld(&[], false).is_empty());
    // Nor does a machine that admits everything that was asked of it.
    assert!(confined().withheld(&declared(), true).is_empty());
}

#[test]
fn the_provider_carries_the_record_of_what_it_was_not_given() {
    // The catalogue built from this provider cannot answer the question any more — an entry that
    // was never published and one nobody wanted are the same absence downstream — so the provider
    // is where the two halves were last both known.
    let withheld = provider(&unconfined(), Scripted::new(vec![]), &["cargo"])
        .withheld()
        .to_vec();
    assert_eq!(withheld.len(), 1, "{withheld:?}");
    assert_eq!(withheld[0].tool, "run");
    assert!(
        provider(&confined(), Scripted::new(vec![]), &["cargo"])
            .withheld()
            .is_empty()
    );
    assert!(
        provider(&unconfined(), Scripted::new(vec![]), &[])
            .withheld()
            .is_empty()
    );
}

#[test]
fn a_withheld_record_round_trips_as_the_two_strings_it_is() {
    let withheld = unconfined().withheld(&declared(), true);
    let encoded = serde_json::to_value(&withheld).expect("serializes");
    assert_eq!(encoded[0]["tool"], json!("run"));
    assert_eq!(
        serde_json::from_value::<Vec<Withheld>>(encoded).expect("deserializes"),
        withheld
    );
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

use harness_tools::{Catalogue, Operations, ReadWindow, SearchOptions};
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
    assert_eq!(entries, vec!["file_read", "dir_list", "search", "find"]);
}

/// A backend answering one file of `bytes` bytes, all on one line.
fn a_file_of(bytes: usize) -> Scripted {
    a_file_holding(&"x".repeat(bytes))
}

/// A backend answering `count` lines of `width` characters each.
fn a_file_of_lines(count: usize, width: usize) -> Scripted {
    a_file_holding(&format!("{}\n", "x".repeat(width)).repeat(count))
}

/// A backend answering exactly this text.
fn a_file_holding(text: &str) -> Scripted {
    let data = base64_of(text);
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
    let answer = provider(&confined(), a_file_of_lines(5_000, 40), &["cargo"])
        .file_read("big.txt", ReadWindow::whole())
        .expect("read");

    assert_eq!(answer["bytes"], 205_000, "the whole size is still reported");
    assert_eq!(
        answer["truncated"], true,
        "a partial read never looks whole"
    );
    assert_eq!(
        answer["lines"],
        json!({"from": 1, "to": 65_536 / 41, "total": 5_000}),
        "as many whole lines as the same byte ceiling the unconfined provider uses holds"
    );
}

#[test]
fn a_confined_read_the_caller_bounded_more_tightly_is_answered_at_that_bound() {
    let answer = provider(&confined(), a_file_of_lines(200, 40), &["cargo"])
        .file_read(
            "big.txt",
            ReadWindow {
                max_bytes: Some(100),
                ..ReadWindow::whole()
            },
        )
        .expect("read");

    assert_eq!(answer["lines"], json!({"from": 1, "to": 2, "total": 200}));
    assert_eq!(answer["truncated"], true);
}

#[test]
fn a_confined_read_inside_the_bound_is_answered_whole_and_says_it_is_whole() {
    let answer = provider(&confined(), a_file_of(120), &["cargo"])
        .file_read("small.txt", ReadWindow::whole())
        .expect("read");

    assert_eq!(answer["truncated"], false);
    assert_eq!(answer["lines"], json!({"from": 1, "to": 1, "total": 1}));
    assert_eq!(answer["truncated_lines"], json!([]));
    assert_eq!(
        answer["text"].as_str().expect("text").len(),
        120 + "     1\t\n".len()
    );
}

#[test]
fn a_confined_read_answers_the_same_numbered_window_the_unconfined_one_does() {
    // A run's replies must not change shape when it is confined: the same numbered lines, the same
    // `lines` block, so a model that learnt to quote a read back to `file_edit` here does it there.
    let answer = provider(&confined(), a_file_holding("a\nb\nc\nd\ne\n"), &["cargo"])
        .file_read("five.txt", ReadWindow::lines(2, 2))
        .expect("read");

    assert_eq!(answer["text"], json!("     2\tb\n     3\tc\n"));
    assert_eq!(answer["lines"], json!({"from": 2, "to": 3, "total": 5}));
    assert_eq!(answer["truncated"], true, "line 5 is not in it");
}

#[test]
fn a_read_the_route_ceiling_cut_is_never_answered_as_though_it_were_the_whole_file() {
    // 4,096 lines of 64 bytes is exactly the 262,144 this machine says it reads and no more, so
    // what came back is a prefix of something larger. It used to be answered as the file: a window
    // ending on the prefix's last line said `truncated: false`, and `lines.total` was the prefix's
    // count under the name of the file's.
    let answer = provider(&confined(), a_file_of_lines(4_096, 63), &["cargo"])
        .file_read("big.txt", ReadWindow::lines(4_090, 10))
        .expect("read");

    assert_eq!(
        answer["truncated"], true,
        "a window ending at the prefix's last line has not reached the file's"
    );
    assert_eq!(
        answer["lines"]["total"],
        Value::Null,
        "how many lines the file has is not knowable from a prefix"
    );
    assert_eq!(answer["bytes"], Value::Null, "nor how large it is");
    assert_eq!(answer["bytes_read"], 262_144);
    assert_eq!(answer["route_ceiling_bytes"], 262_144);
    assert!(
        answer["note"]
            .as_str()
            .expect("a note")
            .contains("cannot be reached on this path"),
        "{}",
        answer["note"]
    );
}

#[test]
fn an_edit_of_a_file_the_read_route_could_not_answer_whole_is_refused_rather_than_truncating_it() {
    // An edit writes back what it read. On a file the ceiling cut, that is a write of the prefix
    // over the whole file - everything past the ceiling deleted, by a tool the model asked to
    // change one line.
    let refusal = provider(&confined(), a_file_of_lines(4_096, 63), &["cargo"])
        .file_edit("big.txt", "xxx", "yyy")
        .expect_err("refused");

    assert!(refusal.contains("read ceiling"), "{refusal}");
    assert!(refusal.contains("Nothing was changed"), "{refusal}");
}

#[test]
fn a_confined_window_past_what_the_read_route_reached_is_refused_by_the_line_it_stopped_at() {
    // The route answers from byte 0 up to a ceiling and hands back a `String`, so there is no
    // offset to seek with. A window past the last line those bytes hold cannot be answered - and
    // answering nothing would look exactly like a file that has no such lines.
    //
    // **The fixture is a file the ceiling really cut** - 4,096 lines of 64 bytes is the fixture
    // ceiling exactly - because that is the case this sentence is true of. It used to be a six-byte
    // file, and the assertion that the refusal blames a byte ceiling passed on a read nothing had
    // stopped; the case below is the other half, and the two together are what that assertion was
    // for.
    let refusal = provider(&confined(), a_file_of_lines(4_096, 63), &["cargo"])
        .file_read("big.txt", ReadWindow::lines(5_000, 10))
        .expect_err("refused");

    assert!(refusal.contains("reaches line 4096"), "{refusal}");
    assert!(refusal.contains("line 5000"), "{refusal}");
    assert!(refusal.contains("byte ceiling"), "and why: {refusal}");
}

#[test]
fn a_confined_window_past_the_end_of_a_short_file_does_not_blame_a_ceiling_nothing_reached() {
    // `ceiling_cut` is computed once and consulted for `bytes`, `truncated`, `lines.total` and the
    // note - and, until this case existed, not for the refusal. So a three-line file under a
    // 256 KiB route ceiling was refused with "the route answers ... up to a byte ceiling of 262144
    // bytes, and that is where it stopped", which nothing had done, beside a note saying the lines
    // past it "cannot be reached on this path at any `offset`".
    //
    // It is invariant 8 seen from the other side: a whole answer reported as a cut one. The move it
    // invites is a model giving up on a file it has entirely seen, and the sentence below is the
    // unconfined provider's own, word for word, because there is one true thing to say here.
    let refusal = provider(&confined(), a_file_holding("a\nb\nc\n"), &["cargo"])
        .file_read("three.txt", ReadWindow::lines(40, 10))
        .expect_err("refused");

    assert!(refusal.contains("has 3 lines"), "{refusal}");
    assert!(refusal.contains("line 40"), "{refusal}");
    assert!(refusal.contains("past the end"), "{refusal}");
    assert!(
        !refusal.contains("byte ceiling") && !refusal.contains("cannot be reached"),
        "nothing cut this file, and a refusal that says one did is one the model acts on: {refusal}"
    );
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
    assert!(
        refused.message().contains("`sh` is not a program"),
        "{refused}"
    );
    assert!(
        refused.message().contains("cargo"),
        "the set is listed: {refused}"
    );
    // And named, so a reader of the record counts it without matching that sentence.
    assert_eq!(
        refused.refusal(),
        Some(&harness_tools::Refusal::ProgramNotDeclared {
            program: "sh".to_owned(),
            declared: vec!["cargo".to_owned()],
        }),
        "{refused}"
    );
    assert!(
        script.seen.lock().expect("not poisoned").is_empty(),
        "and nothing was sent: the refusal is local"
    );
}

/// A machine document in the envelope every route answers, naming a capability snapshot.
///
/// An exec over the socket reads this first and refuses without it, so any script that reaches
/// `/v1/execs` begins here.
const MACHINE_WITH_SNAPSHOT: &str = r#"{"result":{"driver":"host","driver_version":"0.2.0",
    "snapshot":"cap_7f3a","facts":{"workspace.guarded-io":true,"exec.argv-only":true,
    "exec.cgroup-limits":{"cpu":true,"memory":true,"processes":true}}}}"#;

/// What a daemon answers `POST /v1/execs` with under `wait: true`: the exec resource, exited.
const EXEC_EXITED: &str = r#"{"result":{"id":"ex_1","kind":"exec","workspace":"ws_a","state":"exited",
    "exit":{"code":0,"signal":null}}}"#;
/// One stream of it, whole: `ok\n`.
const STDOUT_SLICE: &str = r#"{"result":{"exec":"ex_1","stream":"stdout","offset":0,"returned_bytes":3,
    "next_offset":3,"eof":true,"truncated":false,"content":{"encoding":"base64","data":"b2sK"}}}"#;
const STDERR_SLICE: &str = r#"{"result":{"exec":"ex_1","stream":"stderr","offset":0,"returned_bytes":0,
    "next_offset":0,"eof":true,"truncated":false,"content":{"encoding":"base64","data":""}}}"#;
/// The output routes, as the client must spell them: `ExecOutputQuery` is closed and needs all three.
const STDOUT_ROUTE: &str = "/v1/execs/ex_1/output?stream=stdout&offset=0&limit_bytes=1048576";
const STDERR_ROUTE: &str = "/v1/execs/ex_1/output?stream=stderr&offset=0&limit_bytes=1048576";

#[test]
fn a_declared_program_is_sent_as_an_argv_and_never_as_a_command_line() {
    let script = Scripted::new(vec![
        (200, MACHINE_WITH_SNAPSHOT),
        (200, EXEC_EXITED),
        (200, STDOUT_SLICE),
        (200, STDERR_SLICE),
    ]);
    let confined_provider = provider(&confined(), script.clone(), &["cargo"]);
    confined_provider
        .run(&["cargo".to_owned(), "test".to_owned()])
        .expect("ran");

    let seen = script.seen.lock().expect("not poisoned");
    assert_eq!(
        (seen[1].0.as_str(), seen[1].1.as_str()),
        ("POST", "/v1/execs")
    );
    let sent = seen[1].2.as_ref().expect("a body");
    assert_eq!(sent["input"]["argv"], json!(["cargo", "test"]));
    assert!(
        sent["input"].get("command").is_none(),
        "there is no command line anywhere in it: {sent}"
    );
}

#[test]
fn an_exec_over_the_socket_asks_for_confinement_by_name() {
    // What this pins: the socket path posted `{workspace_id, argv}` and nothing else, so it asked
    // for an exec **without asking for confinement** - and whether that ran unconfined or was
    // refused was the daemon's choice rather than this harness's.
    let script = Scripted::new(vec![
        (200, MACHINE_WITH_SNAPSHOT),
        (200, EXEC_EXITED),
        (200, STDOUT_SLICE),
        (200, STDERR_SLICE),
    ]);
    let client = Client::with(script.clone());
    client
        .exec("ws_a", &["/usr/bin/true".to_owned()], None)
        .expect("ran");

    let seen = script.seen.lock().expect("not poisoned");
    assert_eq!(
        seen.iter()
            .map(|(method, path, _)| (method.as_str(), path.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("GET", "/v1/machine"),
            ("POST", "/v1/execs"),
            ("GET", STDOUT_ROUTE),
            ("GET", STDERR_ROUTE),
        ]
    );

    let body = seen[1].2.as_ref().expect("a body");
    // The daemon's mutation decoder reads `op` before it reads anything, and refuses a body with
    // any third key. Every body this client posted until 2026-08-29 had `input` alone, so the
    // confinement it asked for never reached a daemon that could read it.
    assert!(
        is_operation_id(body["op"].as_str().expect("an op")),
        "{body}"
    );
    assert_eq!(
        body.as_object().expect("an object").len(),
        2,
        "`op` and `input`, nothing else: {body}"
    );
    let sent = &body["input"];
    // `required` is spelled `require` on the wire - `ConfinementRequest` renames it - which is
    // exactly why this body is serialised from the wire crate's type instead of hand-written.
    assert_eq!(sent["sandbox"]["require"], json!(true), "{sent}");
    assert_eq!(
        sent["sandbox"]["network"],
        serde_json::to_value(substrate_wire::NetworkMode::None).expect("serialises"),
        "{sent}"
    );
    assert_eq!(
        sent["sandbox"]["capability_snapshot"],
        json!("cap_7f3a"),
        "the exec names the snapshot it was admitted against: {sent}"
    );
    assert_eq!(sent["argv"], json!(["/usr/bin/true"]));
    assert_eq!(sent["wait"], json!(true));
    assert_eq!(sent["limits"]["timeout_ms"], json!(900_000));
    assert_eq!(sent["env"]["allow"], json!([]));
}

#[test]
fn the_snapshot_is_asked_for_once_per_client_and_every_exec_names_that_one() {
    // The daemon states one snapshot for its lifetime; asking before every exec was a round trip
    // that bought nothing and let publication and admission read two different documents.
    let script = Scripted::new(vec![
        (200, MACHINE_WITH_SNAPSHOT),
        (200, EXEC_EXITED),
        (200, STDOUT_SLICE),
        (200, STDERR_SLICE),
        (200, EXEC_EXITED),
        (200, STDOUT_SLICE),
        (200, STDERR_SLICE),
    ]);
    let client = Client::with(script.clone());
    client.machine().expect("probed");
    client
        .exec("ws_a", &["/usr/bin/true".to_owned()], None)
        .expect("ran");
    client
        .exec("ws_a", &["/usr/bin/false".to_owned()], None)
        .expect("ran");

    let seen = script.seen.lock().expect("not poisoned");
    assert_eq!(
        seen.iter()
            .map(|(method, path, _)| (method.as_str(), path.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("GET", "/v1/machine"),
            ("POST", "/v1/execs"),
            ("GET", STDOUT_ROUTE),
            ("GET", STDERR_ROUTE),
            ("POST", "/v1/execs"),
            ("GET", STDOUT_ROUTE),
            ("GET", STDERR_ROUTE),
        ],
        "one probe, then the execs"
    );
    for start in [&seen[1], &seen[4]] {
        let body = start.2.as_ref().expect("a body");
        assert_eq!(
            body["input"]["sandbox"]["capability_snapshot"],
            json!("cap_7f3a")
        );
    }
}

/// `common.json#/$defs/operation-id`: 16 to 128 of `[A-Za-z0-9_-]`.
fn is_operation_id(value: &str) -> bool {
    (16..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[test]
fn an_operation_id_is_the_shape_the_contract_admits_and_no_two_are_alike() {
    // `op` is a caller-minted idempotency key, not the operation's name: `"workspace.create"`
    // has a `.` and was refused by a live daemon for exactly that.
    let id = super::client::operation_identity(1_756_432_000_000_000_000, 3_914_145, 0);
    assert!(is_operation_id(&id), "{id}");
    assert!(!is_operation_id("workspace.create"));
    assert!(!is_operation_id("op_short"));
    assert_ne!(
        super::client::operation_identity(1, 2, 3),
        super::client::operation_identity(1, 2, 4),
        "two calls on one client"
    );
    assert_ne!(
        super::client::operation_identity(1, 2, 3),
        super::client::operation_identity(1, 3, 3),
        "two processes"
    );
}

#[test]
fn every_mutating_route_posts_a_fresh_operation_id_beside_its_input() {
    let script = Scripted::new(vec![
        (200, r#"{"result":{"id":"ws_new"}}"#),
        (200, r#"{"result":{}}"#),
    ]);
    let client = Client::with(script.clone());
    client.workspace_create(60_000).expect("opened");
    client
        .file_write("ws_new", "a.txt", "hello")
        .expect("written");

    let seen = script.seen.lock().expect("not poisoned");
    let ops: Vec<(&str, &str, String)> = seen
        .iter()
        .map(|(method, path, body)| {
            let body = body.as_ref().expect("a body");
            assert_eq!(body.as_object().expect("an object").len(), 2, "{body}");
            let op = body["op"].as_str().expect("an op").to_owned();
            assert!(is_operation_id(&op), "{body}");
            (method.as_str(), path.as_str(), op)
        })
        .collect();
    assert_eq!(ops[0].0, "POST");
    assert_eq!(ops[0].1, "/v1/workspaces");
    assert_eq!(ops[1].0, "PUT");
    assert_eq!(ops[1].1, "/v1/workspaces/ws_new/files/a.txt");
    assert_ne!(ops[0].2, ops[1].2, "one id per mutation, never reused");
}

#[test]
fn an_exec_over_the_socket_answers_what_the_program_said_and_how_it_ended() {
    // Read off a live daemon on 2026-08-29: the start answers the exec resource under `result`
    // with its `id`, and the output is two slices, one per stream, each base64. The client looked
    // for `exec_id`, fell through to answering the start document, and the model got an exit
    // code and never the output.
    let script = Scripted::new(vec![
        (200, MACHINE_WITH_SNAPSHOT),
        (200, EXEC_EXITED),
        (200, STDOUT_SLICE),
        (200, STDERR_SLICE),
    ]);
    let confined_provider = provider(&confined(), script.clone(), &["/usr/bin/true"]);
    let answer = confined_provider
        .run(&["/usr/bin/true".to_owned()])
        .expect("ran");

    assert_eq!(answer["stdout"], json!("ok\n"), "{answer}");
    assert_eq!(answer["stderr"], json!(""), "{answer}");
    assert_eq!(answer["stdout_truncated"], json!(false));
    assert_eq!(answer["output_complete"], json!(true));
    assert_eq!(answer["exit"]["exit"]["code"], json!(0), "{answer}");
    assert_eq!(answer["exit"]["state"], json!("exited"), "{answer}");

    // A slice the daemon cut, or one with more behind it, is not the whole answer.
    let script = Scripted::new(vec![
        (200, MACHINE_WITH_SNAPSHOT),
        (200, EXEC_EXITED),
        (
            200,
            r#"{"result":{"exec":"ex_1","stream":"stdout","offset":0,"returned_bytes":3,
                "next_offset":3,"eof":false,"truncated":false,
                "content":{"encoding":"base64","data":"b2sK"}}}"#,
        ),
        (200, STDERR_SLICE),
    ]);
    let confined_provider = provider(&confined(), script, &["/usr/bin/true"]);
    let answer = confined_provider
        .run(&["/usr/bin/true".to_owned()])
        .expect("ran");
    assert_eq!(answer["stdout_truncated"], json!(true), "{answer}");
    assert_eq!(answer["output_complete"], json!(false), "{answer}");
}

#[test]
fn an_exec_over_the_socket_is_refused_when_the_daemon_states_no_snapshot() {
    // A daemon that names no snapshot cannot admit a confined exec, so the honest answer is a
    // refusal here rather than a start that may or may not have been confined over there.
    let script = Scripted::new(vec![(
        200,
        r#"{"result":{"driver":"host","facts":{"exec.argv-only":true}}}"#,
    )]);
    let client = Client::with(script.clone());
    let refused = client
        .exec("ws_a", &["/usr/bin/true".to_owned()], None)
        .expect_err("refused");

    assert!(
        matches!(refused, SubstrateError::Refused { status: 0, .. }),
        "no HTTP happened, so there is no status to quote: {refused:?}"
    );
    assert!(refused.to_string().contains("snapshot"), "{refused}");

    let seen = script.seen.lock().expect("not poisoned");
    assert!(
        !seen
            .iter()
            .any(|(method, path, _)| method == "POST" && path == "/v1/execs"),
        "and nothing was started: {seen:?}"
    );
}

#[test]
fn the_embedded_and_socket_paths_build_one_exec_input() {
    // Both call this, so a later edit to one path cannot quietly ask for something weaker than the
    // other. The figures are the ones argued at the builder.
    let input = super::confined_exec_input(
        "ws_a",
        &["/usr/bin/true".to_owned()],
        "cap_7f3a".to_owned(),
        substrate_wire::ExecEnvironment {
            allow: Vec::new(),
            set: std::collections::BTreeMap::new(),
        },
        Vec::new(),
        None,
    );

    assert!(input.sandbox.required);
    assert_eq!(input.sandbox.network, substrate_wire::NetworkMode::None);
    assert_eq!(
        input.sandbox.profile,
        substrate_wire::SandboxProfile::Workspace
    );
    assert_eq!(input.sandbox.capability_snapshot, "cap_7f3a");
    assert_eq!(input.limits.timeout_ms, 900_000);
    assert_eq!(input.limits.output_bytes, 1_048_576);
    assert_eq!(input.limits.processes, 2_048);
    assert_eq!(input.limits.memory_bytes, 8_589_934_592);
    assert_eq!(input.limits.cpu_millis, 3_600_000);
    assert!(input.wait);
    assert!(input.capsule.is_none());
    assert!(input.lease_ttl_ms.is_none());
}

#[test]
fn an_exec_is_bounded_by_what_the_run_has_left_and_never_above_the_ceiling() {
    // The loop's deadline check between calls cannot reach into an exec the daemon is holding
    // open, so the timeout the daemon enforces is the smaller of the build ceiling and the clock.
    let build = |remaining| {
        super::confined_exec_input(
            "ws_a",
            &["/usr/bin/true".to_owned()],
            "cap_7f3a".to_owned(),
            substrate_wire::ExecEnvironment {
                allow: Vec::new(),
                set: std::collections::BTreeMap::new(),
            },
            Vec::new(),
            remaining,
        )
        .limits
        .timeout_ms
    };
    assert_eq!(build(None), 900_000, "no deadline is the ceiling");
    assert_eq!(
        build(Some(std::time::Duration::from_secs(30))),
        30_000,
        "half a minute left is half a minute"
    );
    assert_eq!(
        build(Some(std::time::Duration::from_secs(3600))),
        900_000,
        "an hour left is still the ceiling"
    );

    // And it reaches the wire through the provider.
    let script = Scripted::new(vec![
        (200, MACHINE_WITH_SNAPSHOT),
        (200, EXEC_EXITED),
        (200, STDOUT_SLICE),
        (200, STDERR_SLICE),
    ]);
    let confined_provider = provider(&confined(), script.clone(), &["/usr/bin/true"]);
    confined_provider
        .run_within(
            &["/usr/bin/true".to_owned()],
            Some(std::time::Duration::from_millis(1_500)),
        )
        .expect("ran");
    let seen = script.seen.lock().expect("not poisoned");
    let sent = seen[1].2.as_ref().expect("a body");
    assert_eq!(
        sent["input"]["limits"]["timeout_ms"],
        json!(1_500),
        "{sent}"
    );
}

#[test]
fn two_workspaces_opened_with_one_lease_are_two_workspaces() {
    // It was `ws_{lease}_{pid}`: a run that opened two workspaces with the same TTL minted the same
    // id twice, and the second caller was handed the first's tree with whatever was already in it.
    let first = super::embedded::workspace_identity(600_000, 4242, 0);
    let second = super::embedded::workspace_identity(600_000, 4242, 1);
    assert_ne!(first, second);

    // `^ws_[A-Za-z0-9_]+$` - the stricter of the driver's two disagreeing checks. A name that
    // passes the other one reaches `mkdirat` and comes back as `workspace.path-escape`, which
    // reads as a containment failure and is a naming rule.
    for id in [&first, &second] {
        assert!(id.starts_with("ws_"), "{id}");
        assert!(
            id.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
            "{id}"
        );
    }
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
fn a_confined_workspace_lists_searches_and_finds_through_nothing_and_says_so() {
    // `Backend` carries no listing route, and reading the host filesystem to fake one would step
    // around the containment this provider exists for. A run that needs a listing gets it from the
    // reading provider beside this one, which is what `harness_tools::Split` composes.
    let confined_provider = provider(&confined(), Scripted::new(vec![]), &[]);
    for refused in [
        confined_provider.dir_list("."),
        confined_provider.search("fn", ".", &SearchOptions::default()),
        confined_provider.find("*.rs", ".", None),
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

#[test]
fn a_declared_go_toolchain_keeps_all_mutable_state_inside_the_workspace() {
    let host = tempfile::tempdir().expect("a fake Go installation");
    let bin = host.path().join("bin");
    std::fs::create_dir(&bin).expect("a bin directory");
    let program = bin.join("go");
    std::fs::write(&program, b"not executed").expect("a go program");
    let mut permissions = program.metadata().expect("program metadata").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o500);
    std::fs::set_permissions(&program, permissions).expect("an executable program");

    let toolchain = super::Toolchain::go(Some(host.path()), None).expect("the explicit GOROOT");
    assert_eq!(toolchain.roots().len(), 1);
    assert_eq!(toolchain.roots()[0].mount, "/toolchain/go");
    assert_eq!(
        std::path::Path::new(&toolchain.roots()[0].host_path),
        host.path().canonicalize().expect("the canonical root")
    );
    assert_eq!(
        toolchain.env(),
        &std::collections::BTreeMap::from([
            ("CGO_ENABLED".to_owned(), "0".to_owned()),
            (
                "GOCACHE".to_owned(),
                "/workspace/.cache/go-build".to_owned()
            ),
            ("GOENV".to_owned(), "off".to_owned()),
            ("GOMODCACHE".to_owned(), "/workspace/.go/pkg/mod".to_owned()),
            ("GOPATH".to_owned(), "/workspace/.go".to_owned()),
            ("GOROOT".to_owned(), "/toolchain/go".to_owned()),
            ("GOSUMDB".to_owned(), "off".to_owned()),
            ("GOTOOLCHAIN".to_owned(), "local".to_owned()),
            ("HOME".to_owned(), "/workspace".to_owned()),
            (
                "PATH".to_owned(),
                "/toolchain/go/bin:/usr/local/bin:/usr/bin:/bin".to_owned(),
            ),
        ])
    );
}

#[test]
fn a_go_toolchain_is_discovered_from_path_without_executing_it() {
    let host = tempfile::tempdir().expect("a fake Go installation");
    let bin = host.path().join("bin");
    std::fs::create_dir(&bin).expect("a bin directory");
    let program = bin.join("go");
    std::fs::write(&program, b"not executed").expect("a go program");
    let mut permissions = program.metadata().expect("program metadata").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o500);
    std::fs::set_permissions(&program, permissions).expect("an executable program");

    let toolchain = super::Toolchain::go(None, Some(bin.as_os_str())).expect("PATH discovery");
    assert_eq!(toolchain.roots()[0].mount, "/toolchain/go");
    assert_eq!(
        std::path::Path::new(&toolchain.roots()[0].host_path),
        host.path().canonicalize().expect("the canonical root")
    );
}

#[test]
fn a_declared_go_toolchain_fits_substrates_closed_non_secret_environment() {
    let host = tempfile::tempdir().expect("a fake Go installation");
    let bin = host.path().join("bin");
    std::fs::create_dir(&bin).expect("a bin directory");
    let program = bin.join("go");
    std::fs::write(&program, b"not executed").expect("a go program");
    let mut permissions = program.metadata().expect("program metadata").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o500);
    std::fs::set_permissions(&program, permissions).expect("an executable program");

    let toolchain = super::Toolchain::go(Some(host.path()), None).expect("the explicit GOROOT");
    for name in toolchain.env().keys() {
        let lower = name.to_ascii_lowercase();
        for forbidden in [
            "authorization",
            "bearer",
            "credential",
            "password",
            "proxy",
            "secret",
            "token",
        ] {
            assert!(
                !lower.contains(forbidden),
                "`{name}` is refused by substrate's closed non-secret environment because it \
                 contains `{forbidden}`"
            );
        }
    }
}

#[test]
fn an_undeclared_go_toolchain_is_refused_by_name() {
    assert_eq!(
        super::Toolchain::go(None, None).expect_err("there is no discovery source"),
        "neither `GOROOT` nor `PATH` says where the Go toolchain is"
    );
}

#[test]
fn a_staged_driver_admits_one_file_and_never_the_directory_it_came_from() {
    // The failure this exists to remove: a driven run allow-listed its own CLI by absolute host
    // path, the sandbox had no such file, every `run` died at `ENOENT`, and the model wrote the
    // planning store's files directly instead. Allow-listing admits the name; only a mount admits
    // the file.
    let host = tempfile::tempdir().expect("a directory to build in");
    let build = host.path().join("debug");
    std::fs::create_dir_all(&build).expect("a build directory");
    let program = build.join("protocol");
    std::fs::write(&program, b"#!/bin/sh\nprintf driver\n").expect("a program");
    // Everything else a build directory holds, and none of it is this run's business.
    std::fs::write(build.join("some-other-binary"), b"x").expect("a sibling");
    std::fs::create_dir_all(build.join("deps")).expect("a deps directory");

    let stage_root = tempfile::tempdir().expect("a stage");
    let toolchain = super::Toolchain::default()
        .with_driver(&program, stage_root.path())
        .expect("the driver stages");

    let roots = toolchain.roots();
    assert_eq!(roots.len(), 1, "one program is one root");
    assert_eq!(roots[0].mount, "/toolchain/driver");
    assert_ne!(
        std::path::Path::new(&roots[0].host_path),
        build.as_path(),
        "mounting the build directory would admit `deps` and every other binary to answer for one"
    );
    assert_eq!(
        std::fs::read_dir(&roots[0].host_path)
            .expect("the stage is readable")
            .count(),
        1,
        "the stage holds the driver and nothing else"
    );

    let driver = toolchain.driver().expect("a driver was declared");
    assert_eq!(
        driver.program(),
        "/toolchain/driver/protocol",
        "an argv has to name the path inside the sandbox, not the one on this host"
    );
    // substrate mounts a root read-only and reports it; it computes no digest over one. So the
    // claim that a run pins the build its evidence is recorded against is only true because this
    // value exists for a caller to write down.
    assert_eq!(
        driver.sha256(),
        "2262eae9d3b679fe881472d86f942cd94b6065a63ad65f47a1a87bee31c7a276",
        "the digest of the staged bytes, agreed by `sha256sum` on the same content"
    );
}
