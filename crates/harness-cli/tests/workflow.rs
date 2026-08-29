//! `b10x-harness workflow`, through the shipped binary, over both emulators.
//!
//! What the unit tests cannot reach: a document read from disk, a plan validated, one process
//! walking it, one conversation per section, and a session file per `(scope, attempt)` on the
//! machine afterwards. Evidence from here is `provider_emulated` (`AGENTS.md` invariant 18).
//!
//! Every walking test runs against **both** wires, because a workflow is not a feature of one:
//! `answer` is a tool the loop owns and both emulators serve the same scenarios.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

/// Which emulator a case runs against, and the flag that points the binary at it.
#[derive(Debug, Clone, Copy)]
enum Wire {
    Responses,
    Messages,
}

/// Both of them. A case that ran on one would prove the walk works over that wire's projection.
const WIRES: [Wire; 2] = [Wire::Responses, Wire::Messages];

impl Wire {
    fn fixture(self, scenario: &str) -> Fixture {
        self.recording(scenario, None)
    }

    /// The same emulator, writing one line per request it served.
    ///
    /// How a test proves a turn **did not happen**: an absence in the flow's own record could be a
    /// walk that skipped a step or a renderer that dropped an event, and only the far end of the
    /// socket can say nothing was asked.
    fn recording(self, scenario: &str, record: Option<&Path>) -> Fixture {
        match self {
            Self::Responses => {
                Fixture::of("harness-responses", "fake_responses.py", scenario, record)
            }
            Self::Messages => Fixture::of("harness-messages", "fake_messages.py", scenario, record),
        }
    }

    fn flags(self) -> &'static [&'static str] {
        match self {
            Self::Responses => &[],
            Self::Messages => &["--wire", "anthropic-messages"],
        }
    }
}

struct Fixture {
    child: Child,
    base_url: String,
}

impl Fixture {
    fn of(crate_name: &str, script: &str, scenario: &str, record: Option<&Path>) -> Self {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(crate_name)
            .join("tests")
            .join("fixtures")
            .join(script);
        let mut command = Command::new("python3");
        command.arg(&script).arg("--scenario").arg(scenario);
        if let Some(record) = record {
            command.arg("--record").arg(record);
        }
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the fixture starts; python3 must be available");
        let mut line = String::new();
        BufReader::new(child.stdout.as_mut().expect("piped stdout"))
            .read_line(&mut line)
            .expect("the fixture announces its address");
        let ready: Value = serde_json::from_str(&line).expect("the announcement is JSON");
        Self {
            base_url: ready["base_url"].as_str().expect("a base url").to_owned(),
            child,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Output {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

fn raw(arguments: &[&str]) -> Output {
    let output = Command::new(BINARY)
        .args(arguments)
        .env("B10X_HARNESS_TEST_KEY", "test-key")
        .output()
        .expect("the binary runs");
    Output {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// A workspace with one readable file, so the run's tools have a real tree to see.
fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut file = fs::File::create(dir.path().join("README.md")).expect("create");
    file.write_all(b"hello harness\n").expect("write");
    dir
}

/// A workflow document, written by the test rather than found anywhere.
fn flow_file(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).expect("write the flow");
    path
}

/// Two sections and three steps, with one name crossing the boundary between them.
///
/// `shape` promises `specification_id`; `build` needs `shape`, so its first turn must carry what
/// `shape` handed over and none of `shape`'s conversation.
const TWO_SECTIONS: &str = "\
id: two-sections
root:
  id: root
  nodes:
    - id: shape
      gives: [specification_id]
      nodes:
        - id: specify
          run:
            state: specify
            summary: \"State the required behaviour.\"
        - id: decompose
          needs: [specify]
          run:
            state: decompose
            prompt: \"Break it into units that can be verified on their own.\"
    - id: build
      needs: [shape]
      nodes:
        - id: implement
          run:
            state: implement
            summary: \"Make the smallest change that satisfies the units.\"
";

/// Two sections a governor can be asked about, one of which may be retreated into once.
///
/// `build` does **not** need `shape`: a refusal at `shape`'s boundary must leave the rest of the
/// walk running, so a test can count what the emulator was asked and see a section missing from it
/// rather than a whole flow that stopped.
const GOVERNED: &str = "\
id: governed
root:
  id: root
  nodes:
    - id: shape
      repeat: {max: 2}
      gives: [specification_id]
      nodes:
        - id: specify
          run:
            state: specify
            summary: \"State the required behaviour.\"
    - id: build
      nodes:
        - id: implement
          run:
            state: implement
            summary: \"Make the smallest change that satisfies the units.\"
";

/// One section, two steps, the second needing the first — the shortest document that can show a
/// ceiling binding *between* steps rather than inside one.
const TWO_STEPS: &str = "\
id: costly
root:
  id: root
  nodes:
    - id: one
      run:
        state: one
        summary: \"Do the first thing.\"
    - id: two
      needs: [one]
      run:
        state: two
        summary: \"Do the second thing.\"
";

/// One section that promises a name no step ever answers with, with two attempts to spare.
///
/// The emulator answers `specification_id`; this document asks for `approval_id`. So the section
/// comes out clean and still breaks the promise its own document made — which is the one failure a
/// `repeat` bound must not retry.
const PROMISES_MORE_THAN_IT_GIVES: &str = "\
id: promised
root:
  id: root
  nodes:
    - id: shape
      repeat: {max: 3}
      gives: [approval_id]
      nodes:
        - id: specify
          run:
            state: specify
            summary: \"State the required behaviour.\"
        - id: decompose
          needs: [specify]
          run:
            state: decompose
            prompt: \"Break it into units that can be verified on their own.\"
";

/// Two siblings that need each other, which is the cycle the notation refuses by name.
const CYCLE: &str = "\
id: broken
root:
  id: root
  nodes:
    - id: a
      needs: [b]
    - id: b
      needs: [a]
";

/// One `workflow run` against a fixture, writing its sessions into a directory the test owns.
fn walk(
    fixture: &Fixture,
    wire: Wire,
    flow: &Path,
    workspace: &Path,
    sessions: &Path,
    extra: &[&str],
) -> Output {
    let mut arguments = vec![
        "workflow".to_owned(),
        "run".to_owned(),
        "--base-url".to_owned(),
        fixture.base_url.clone(),
        "--model".to_owned(),
        "b10x-emulated".to_owned(),
        "--api-key-env".to_owned(),
        "B10X_HARNESS_TEST_KEY".to_owned(),
        "--workspace".to_owned(),
        workspace.display().to_string(),
        "--session-dir".to_owned(),
        sessions.display().to_string(),
        "--flow".to_owned(),
        flow.display().to_string(),
        "--input".to_owned(),
        "add a CSV export".to_owned(),
    ];
    arguments.extend(wire.flags().iter().map(|flag| (*flag).to_owned()));
    arguments.extend(extra.iter().map(|argument| (*argument).to_owned()));
    raw(&arguments.iter().map(String::as_str).collect::<Vec<_>>())
}

/// Every session in a directory, by identifier.
fn sessions_in(dir: &Path) -> Vec<(String, Value)> {
    let mut found: Vec<(String, Value)> = fs::read_dir(dir)
        .expect("the session directory exists")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|path| {
            let session: Value =
                serde_json::from_str(&fs::read_to_string(&path).expect("readable"))
                    .expect("a session file");
            (
                session["id"].as_str().expect("an identifier").to_owned(),
                session,
            )
        })
        .collect();
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found
}

/// The first thing the run said to a section, which is where a handoff has to appear.
fn first_turn(session: &Value) -> &str {
    session["items"][0]["text"]
        .as_str()
        .expect("a section's first item is what it was asked")
}

/// Every event of a `--json` walk, one per line.
fn events(output: &Output) -> Vec<Value> {
    output
        .stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|error| panic!("{line}: {error}")))
        .collect()
}

fn kinds(events: &[Value]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|event| event["kind"].as_str())
        .collect()
}

/// The paths of every step that started, in order.
fn steps_started(events: &[Value]) -> Vec<&str> {
    events
        .iter()
        .filter(|event| event["kind"] == "step-started")
        .filter_map(|event| event["path"].as_str())
        .collect()
}

/// Every event of one kind, in order.
fn of_kind<'a>(events: &'a [Value], kind: &str) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|event| event["kind"] == kind)
        .collect()
}

/// The paths of everything the walk said never ran.
fn skipped(events: &[Value]) -> Vec<&str> {
    of_kind(events, "node-skipped")
        .into_iter()
        .filter_map(|event| event["path"].as_str())
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The governor: `transition` hooks and the flow-wide budget (design 0003 § 3, § 2).
// ---------------------------------------------------------------------------------------------

/// A hooks file naming one program, written where the test can reach it.
fn hooks_file(dir: &Path, declaration: &Value) -> PathBuf {
    let path = dir.join("hooks.json");
    fs::write(
        &path,
        serde_json::json!({"version": 1, "hooks": [declaration]}).to_string(),
    )
    .expect("write the hooks file");
    path
}

/// A governor that is a real program: a python3 script, spawned as an argv like any other.
///
/// Returns the hooks file that declares it at `transition`, which is the only thing a caller needs.
fn governor(dir: &Path, body: &str) -> PathBuf {
    let script = dir.join("governor.py");
    fs::write(&script, body).expect("write the governor");
    hooks_file(
        dir,
        &serde_json::json!({
            "on": "transition",
            "command": ["python3", script.display().to_string()],
        }),
    )
}

/// The head of every governor here: read the document, refuse anything that is not a transition.
///
/// Exit 3 rather than 2, so a payload this suite did not expect arrives as *the hook could not
/// answer* — which fails the walk closed — instead of as a refusal that looks deliberate.
const READS_A_TRANSITION: &str = r#"import json, sys
document = json.load(sys.stdin)
if document.get("hook") != "transition":
    sys.stderr.write("not a transition document: " + json.dumps(document))
    sys.exit(3)
"#;

/// A synthetic rate card that prices **output only**, naming the emulated model.
///
/// Written by the test rather than found anywhere (`AGENTS.md` invariant 17). Output alone because
/// the two emulators report input differently — the Responses one counts cached tokens inside its
/// input total and the Messages one beside it — so an input rate would give the same walk two
/// different bills and no single ceiling could be the exact remainder this test needs. Both report
/// 8 output tokens per turn, so at $2/Mtok a step costs exactly 16 millionths of a dollar.
fn output_rate_card(dir: &Path) -> PathBuf {
    let path = dir.join("rates.json");
    fs::write(
        &path,
        r#"{"source": "a synthetic card this test wrote", "as_of": "2026-08-29",
            "models": {"b10x-emulated": {"input_usd_per_mtok": 0.0,
                                         "cached_input_usd_per_mtok": 0.0,
                                         "output_usd_per_mtok": 2.0}}}"#,
    )
    .expect("write the rate card");
    path
}

/// What one step costs under [`output_rate_card`], in millionths of a US dollar.
const STEP_COST_MICRO: &str = "16";

/// How many requests the emulator was asked to serve.
fn requests(record: &Path) -> usize {
    fs::read_to_string(record).map_or(0, |text| text.lines().count())
}

/// What the governor below says when it declines a section's result.
const LEAVE_REFUSED: &str = "the specification was never approved";

/// A governor that declines every `leave` of `root.shape`, whatever the section did.
///
/// One place, because two tests read the same refusal — one out of the JSON record and one off a
/// terminal — and a governor written twice is two governors that can drift.
fn refuses_every_leave_of_shape(dir: &Path) -> PathBuf {
    governor(
        dir,
        &format!(
            r#"{READS_A_TRANSITION}
if document["moment"] == "leave" and document["path"] == "root.shape":
    sys.stdout.write(json.dumps({{"reason": "{LEAVE_REFUSED}"}}))
    sys.exit(2)
sys.exit(0)
"#
        ),
    )
}

#[test]
fn a_transition_hook_that_refuses_a_leave_re_enters_the_section_until_its_bound() {
    // How an engine forces a retreat: not with a new verb, but by declining a section's result. The
    // section came out clean both times and the governor said no both times, so the document's own
    // `repeat: {max: 2}` is what decides there is no third attempt.
    for wire in WIRES {
        let fixture = wire.fixture("flow-passes");
        let workspace = workspace();
        let dir = tempfile::tempdir().expect("a temporary directory");
        let flow = flow_file(dir.path(), "flow.yaml", GOVERNED);
        let sessions = tempfile::tempdir().expect("a temporary directory");
        let hooks = refuses_every_leave_of_shape(dir.path());

        let output = walk(
            &fixture,
            wire,
            &flow,
            workspace.path(),
            sessions.path(),
            &["--json", "--hooks", hooks.to_str().expect("utf-8 path")],
        );

        assert_eq!(
            output.status,
            Some(2),
            "{wire:?}: a section nobody accepted is a flow that did not come out clean: {}",
            output.stderr
        );
        let events = events(&output);
        let refusals = of_kind(&events, "transition-refused");
        assert_eq!(refusals.len(), 2, "{wire:?}: one per attempt: {refusals:?}");
        for refusal in &refusals {
            assert_eq!(refusal["moment"], "leave", "{wire:?}: {refusal}");
            assert_eq!(refusal["path"], "root.shape", "{wire:?}: {refusal}");
            assert_eq!(
                refusal["reason"], LEAVE_REFUSED,
                "{wire:?}: the governor's own words: {refusal}"
            );
        }
        assert_eq!(refusals[0]["attempt"], 1, "{wire:?}");
        assert_eq!(refusals[1]["attempt"], 2, "{wire:?}");
        // The section really ran twice, and the walk carried on to the sibling either way.
        assert_eq!(
            steps_started(&events),
            vec![
                "root.shape.specify",
                "root.shape.specify",
                "root.build.implement"
            ],
            "{wire:?}"
        );
        let left = of_kind(&events, "group-left")
            .into_iter()
            .find(|event| event["path"] == "root.shape")
            .expect("the section was left");
        assert_eq!(left["failed"], true, "{wire:?}: {left}");
        assert_eq!(
            left["exhausted"], true,
            "{wire:?}: it kept being refused until the document stopped letting it try: {left}"
        );
        assert_eq!(left["attempts"], 2, "{wire:?}: {left}");
        // Every consultation is in the record under the point it happened at — the boundary ones
        // beside the loop's own, in one stream and one shape.
        let asked: Vec<&Value> = of_kind(&events, "hook-ran")
            .into_iter()
            .filter(|event| event["point"] == "transition")
            .collect();
        assert_eq!(
            asked
                .iter()
                .filter(|event| event["decision"]["kind"] == "block")
                .count(),
            2,
            "{wire:?}: two refusals, and every other boundary proceeded: {asked:?}"
        );
        assert!(
            asked.iter().all(|event| event["call_id"].is_null()),
            "{wire:?}: a boundary is not a call: {asked:?}"
        );

        // And nothing the refused section produced reached the one that ran after it. `shape`
        // promised `specification_id` and answered with it both times; a section nobody accepted
        // hands nothing on, or the rest of the walk would be building on a value the same record
        // calls failed.
        let filed = sessions_in(sessions.path());
        let (id, build) = filed
            .iter()
            .find(|(id, _)| id.ends_with(".root.build.1"))
            .unwrap_or_else(|| {
                panic!(
                    "{wire:?}: the sibling filed a session: {:?}",
                    filed.iter().map(|(id, _)| id).collect::<Vec<_>>()
                )
            });
        assert!(
            !first_turn(build).contains("Earlier sections established"),
            "{wire:?}: {id} was handed what a refused section produced: {}",
            first_turn(build)
        );
    }
}

#[test]
fn a_refused_leave_reads_on_a_terminal_as_the_retreat_it_caused() {
    // The same walk without `--json`, where the refusal has to be readable as what it did:
    // `group-repeating` says which attempt failed and not why, so the reason is carried across.
    let wire = Wire::Responses;
    let fixture = wire.fixture("flow-passes");
    let workspace = workspace();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let flow = flow_file(dir.path(), "flow.yaml", GOVERNED);
    let sessions = tempfile::tempdir().expect("a temporary directory");
    let hooks = refuses_every_leave_of_shape(dir.path());

    let output = walk(
        &fixture,
        wire,
        &flow,
        workspace.path(),
        sessions.path(),
        &["--hooks", hooks.to_str().expect("utf-8 path")],
    );
    assert_eq!(output.status, Some(2), "stderr: {}", output.stderr);
    assert!(
        output
            .stderr
            .contains(&format!("retreat ↺ root.shape (2 of 2): {LEAVE_REFUSED}")),
        "the retreat names what caused it: {}",
        output.stderr
    );
}

#[test]
fn a_transition_hook_that_refuses_an_enter_skips_the_section_by_name() {
    // A section nobody allowed to start is skipped **as failed**, every step inside it named. The
    // proof that no model was asked is at the far end of the socket: the emulator served one
    // request for a document that holds two steps.
    for wire in WIRES {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let record = dir.path().join("requests.jsonl");
        let fixture = wire.recording("flow-passes", Some(&record));
        let workspace = workspace();
        let flow = flow_file(dir.path(), "flow.yaml", GOVERNED);
        let sessions = tempfile::tempdir().expect("a temporary directory");
        let hooks = governor(
            dir.path(),
            &format!(
                r#"{READS_A_TRANSITION}
if document["moment"] == "enter" and document["path"] == "root.shape":
    sys.stdout.write(json.dumps({{"reason": "that section is not open yet"}}))
    sys.exit(2)
sys.exit(0)
"#
            ),
        );

        let output = walk(
            &fixture,
            wire,
            &flow,
            workspace.path(),
            sessions.path(),
            &["--json", "--hooks", hooks.to_str().expect("utf-8 path")],
        );

        assert_eq!(output.status, Some(2), "{wire:?}: {}", output.stderr);
        let events = events(&output);
        let refusals = of_kind(&events, "transition-refused");
        assert_eq!(refusals.len(), 1, "{wire:?}: {refusals:?}");
        assert_eq!(refusals[0]["moment"], "enter", "{wire:?}");
        assert_eq!(refusals[0]["path"], "root.shape", "{wire:?}");
        // Named as skipped, section and step both, because *it never ran* is what a reader of a
        // green-looking record has to be able to see.
        let skipped = skipped(&events);
        assert!(
            skipped.contains(&"root.shape.specify"),
            "{wire:?}: {skipped:?}"
        );
        assert!(
            of_kind(&events, "node-skipped")
                .iter()
                .all(|event| event["because"]
                    .as_str()
                    .is_some_and(|because| because.contains("that section is not open yet"))),
            "{wire:?}: the governor's own words travel with the skip: {:?}",
            of_kind(&events, "node-skipped")
        );
        assert_eq!(
            steps_started(&events),
            vec!["root.build.implement"],
            "{wire:?}: the sibling still ran"
        );
        // The document has two steps and the emulator was asked once.
        assert_eq!(
            events.first().expect("a first event")["steps"],
            2,
            "{wire:?}"
        );
        assert_eq!(
            requests(&record),
            1,
            "{wire:?}: the refused section cost no model turn at all"
        );
        // No session for a section that never opened a conversation.
        let filed = sessions_in(sessions.path());
        let names: Vec<&str> = filed.iter().map(|(id, _)| id.as_str()).collect();
        assert!(
            names.iter().all(|id| !id.contains(".root.shape.")),
            "{wire:?}: {names:?}"
        );
    }
}

#[test]
fn a_hook_that_cannot_answer_a_transition_fails_closed() {
    // Exit 3 is neither a yes nor a no. The root is a group and is gated like one, so a governor
    // that cannot answer stops the walk at the first boundary there is: nothing runs, and the
    // reason names the program that could not say yes.
    for wire in WIRES {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let record = dir.path().join("requests.jsonl");
        let fixture = wire.recording("flow-passes", Some(&record));
        let workspace = workspace();
        let flow = flow_file(dir.path(), "flow.yaml", GOVERNED);
        let sessions = tempfile::tempdir().expect("a temporary directory");
        let hooks = governor(
            dir.path(),
            &format!(
                r#"{READS_A_TRANSITION}
sys.stderr.write("the engine is not reachable")
sys.exit(3)
"#
            ),
        );

        let output = walk(
            &fixture,
            wire,
            &flow,
            workspace.path(),
            sessions.path(),
            &["--json", "--hooks", hooks.to_str().expect("utf-8 path")],
        );

        assert_eq!(output.status, Some(2), "{wire:?}: {}", output.stderr);
        let events = events(&output);
        let refusals = of_kind(&events, "transition-refused");
        assert_eq!(
            refusals.len(),
            1,
            "{wire:?}: the first boundary is the root's own: {refusals:?}"
        );
        assert_eq!(refusals[0]["path"], "root", "{wire:?}");
        assert_eq!(refusals[0]["moment"], "enter", "{wire:?}");
        let reason = refusals[0]["reason"].as_str().expect("a reason");
        assert!(
            reason.contains("python3"),
            "{wire:?}: the program: {reason}"
        );
        assert!(reason.contains("exited 3"), "{wire:?}: {reason}");
        assert!(
            reason.contains("the engine is not reachable"),
            "{wire:?}: and its own words: {reason}"
        );
        // The record says the hook failed, not that it blocked: a crashed governor and a governor
        // that decided are different facts even where the consequence is the same.
        let asked = of_kind(&events, "hook-ran");
        assert_eq!(asked.len(), 1, "{wire:?}: {asked:?}");
        assert_eq!(asked[0]["point"], "transition", "{wire:?}");
        assert_eq!(asked[0]["decision"]["kind"], "failed", "{wire:?}");
        assert!(steps_started(&events).is_empty(), "{wire:?}");
        assert_eq!(
            requests(&record),
            0,
            "{wire:?}: a walk nobody allowed to start contacts nothing"
        );
    }
}

/// One document's keys, sorted — the whole key set, so an extra one fails as loudly as a missing.
fn keys(document: &Value) -> Vec<String> {
    let mut names: Vec<String> = document
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    names.sort();
    names
}

/// Every boundary one governed walk of [`GOVERNED`] was asked about, as the governor read them.
fn boundaries_of(wire: Wire, workspace: &Path, dir: &Path) -> Vec<Value> {
    let fixture = wire.fixture("flow-passes");
    let flow = flow_file(dir, "flow.yaml", GOVERNED);
    let sessions = tempfile::tempdir().expect("a temporary directory");
    let seen = dir.join("boundaries.jsonl");
    let hooks = governor(
        dir,
        &format!(
            r#"{READS_A_TRANSITION}
with open({seen:?}, "a", encoding="utf-8") as handle:
    handle.write(json.dumps(document) + "\n")
sys.exit(0)
"#,
            seen = seen.display().to_string(),
        ),
    );

    let output = walk(
        &fixture,
        wire,
        &flow,
        workspace,
        sessions.path(),
        &["--hooks", hooks.to_str().expect("utf-8 path")],
    );
    assert_eq!(output.status, Some(0), "{wire:?}: {}", output.stderr);
    fs::read_to_string(&seen)
        .expect("the governor was asked at least once")
        .lines()
        .map(|line| serde_json::from_str(line).expect("one JSON document per boundary"))
        .collect()
}

#[test]
fn the_transition_payload_is_exactly_the_documented_document() {
    // Design 0003 § 3, key for key. This is the contract engineering-protocols writes its governor
    // against, so a key that appeared only sometimes would be a payload nobody could program to.
    for wire in WIRES {
        let workspace = workspace();
        let dir = tempfile::tempdir().expect("a temporary directory");
        let crossed = boundaries_of(wire, workspace.path(), dir.path());
        let at = |path: &str, moment: &str| -> Value {
            crossed
                .iter()
                .find(|document| document["path"] == path && document["moment"] == moment)
                .unwrap_or_else(|| panic!("{wire:?}: no {moment} of `{path}` in {crossed:?}"))
                .clone()
        };

        let entering = at("root.shape", "enter");
        assert_eq!(
            keys(&entering),
            [
                "attempt",
                "flow",
                "hook",
                "moment",
                "of",
                "path",
                "workspace"
            ],
            "{wire:?}: an enter carries no verdict and no handoff: {entering}"
        );
        assert_eq!(entering["hook"], "transition", "{wire:?}");
        assert_eq!(entering["flow"], "governed", "{wire:?}: the document's id");
        assert_eq!(entering["path"], "root.shape", "{wire:?}");
        assert_eq!(entering["attempt"], 1, "{wire:?}");
        assert_eq!(
            entering["of"], 2,
            "{wire:?}: the section's own `repeat.max`: {entering}"
        );
        assert_eq!(
            Path::new(entering["workspace"].as_str().expect("a path")),
            workspace
                .path()
                .canonicalize()
                .expect("the workspace is there"),
            "{wire:?}: absolute, so a governor resolving a path lands where the run does"
        );

        let leaving = at("root.shape", "leave");
        assert_eq!(
            keys(&leaving),
            [
                "attempt",
                "failed",
                "flow",
                "handoff",
                "hook",
                "moment",
                "of",
                "path",
                "workspace"
            ],
            "{wire:?}: {leaving}"
        );
        assert_eq!(leaving["attempt"], 1, "{wire:?}");
        assert_eq!(leaving["of"], 2, "{wire:?}");
        assert_eq!(
            leaving["failed"], false,
            "{wire:?}: the attempt came out clean on its own: {leaving}"
        );
        assert_eq!(
            leaving["handoff"],
            serde_json::json!({"specification_id": "SPEC-1"}),
            "{wire:?}: asked after the handoff, so the governor sees what crosses: {leaving}"
        );

        // The root is a group and is asked like one, with the bound it actually has.
        assert_eq!(at("root", "enter")["of"], 1, "{wire:?}");
        assert_eq!(
            at("root", "leave")["handoff"],
            serde_json::json!({}),
            "{wire:?}: a section that promises nothing hands over nothing, and the key is still there"
        );
    }
}

#[test]
fn a_flow_cost_ceiling_bounds_the_walk_not_the_step() {
    // `--max-cost-microunits` is a ceiling on the **walk**. Set to exactly one step's price, the
    // first step runs and the second never starts: the walk has nothing left, and the step says so
    // without buying a turn to find out.
    for wire in WIRES {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let record = dir.path().join("requests.jsonl");
        let fixture = wire.recording("flow-passes", Some(&record));
        let workspace = workspace();
        let flow = flow_file(dir.path(), "flow.yaml", TWO_STEPS);
        let cards = tempfile::tempdir().expect("a temporary directory");
        let card = output_rate_card(cards.path());
        let sessions = tempfile::tempdir().expect("a temporary directory");

        let output = walk(
            &fixture,
            wire,
            &flow,
            workspace.path(),
            sessions.path(),
            &[
                "--json",
                "--prices",
                card.to_str().expect("utf-8 path"),
                "--max-cost-microunits",
                STEP_COST_MICRO,
            ],
        );

        assert_eq!(
            output.status,
            Some(2),
            "{wire:?}: a bound that bound is a step that failed, not a broken run: {}",
            output.stderr
        );
        let events = events(&output);
        let finished = of_kind(&events, "step-finished");
        assert_eq!(finished.len(), 2, "{wire:?}: {finished:?}");
        assert_eq!(finished[0]["path"], "root.one", "{wire:?}");
        assert_eq!(
            finished[0]["failed"], false,
            "{wire:?}: the first step had the whole ceiling: {}",
            finished[0]
        );
        assert_eq!(finished[1]["path"], "root.two", "{wire:?}");
        assert_eq!(finished[1]["failed"], true, "{wire:?}: {}", finished[1]);

        let warned = of_kind(&events, "warning")
            .into_iter()
            .find(|event| event["code"] == "flow-budget")
            .unwrap_or_else(|| panic!("{wire:?}: no flow-budget warning in {:?}", kinds(&events)));
        let message = warned["message"].as_str().expect("a message");
        assert!(
            message.contains("--max-cost-microunits 16"),
            "{wire:?}: the ceiling is named: {message}"
        );
        assert!(
            message.contains("spent 16"),
            "{wire:?}: and what went: {message}"
        );
        assert!(
            message.contains("`root.two`"),
            "{wire:?}: and which step did not start: {message}"
        );

        // The whole claim, at the far end of the socket: one step, one request.
        assert_eq!(
            requests(&record),
            1,
            "{wire:?}: the second step asked no model anything"
        );
        let last = events.last().expect("a terminal event");
        assert_eq!(last["kind"], "flow-finished", "{wire:?}: {last}");
        assert_eq!(last["clean"], false, "{wire:?}: {last}");
        assert_eq!(last["ran"], 2, "{wire:?}: {last}");
        assert_eq!(last["failed"], 1, "{wire:?}: {last}");
    }
}

#[test]
fn a_flow_walks_its_plan_and_files_one_session_per_scope() {
    for wire in WIRES {
        let fixture = wire.fixture("flow-passes");
        let workspace = workspace();
        let dir = tempfile::tempdir().expect("a temporary directory");
        let flow = flow_file(dir.path(), "flow.yaml", TWO_SECTIONS);
        let sessions = tempfile::tempdir().expect("a temporary directory");

        let output = walk(
            &fixture,
            wire,
            &flow,
            workspace.path(),
            sessions.path(),
            &["--json"],
        );

        assert_eq!(output.status, Some(0), "{wire:?}: {}", output.stderr);
        let events = events(&output);
        assert_eq!(
            steps_started(&events),
            vec![
                "root.shape.specify",
                "root.shape.decompose",
                "root.build.implement"
            ],
            "{wire:?}: the plan's own order"
        );
        // The last line of a finished flow, so a driver reading the stream ends holding the verdict.
        let last = events.last().expect("a terminal event");
        assert_eq!(last["kind"], "flow-finished", "{wire:?}: {last}");
        assert_eq!(last["clean"], true, "{wire:?}: {last}");
        assert_eq!(last["ran"], 3, "{wire:?}: {last}");
        // A step's own loop events land inside its brackets, not beside them.
        let kinds = kinds(&events);
        let started = kinds.iter().position(|kind| *kind == "step-started");
        let finished = kinds.iter().position(|kind| *kind == "step-finished");
        let answered = kinds.iter().position(|kind| *kind == "answered");
        assert!(started < answered && answered < finished, "{kinds:?}");
        // And one `session` line per section, as it is filed.
        assert_eq!(
            kinds.iter().filter(|kind| **kind == "session").count(),
            2,
            "{wire:?}: {kinds:?}"
        );

        let filed = sessions_in(sessions.path());
        assert_eq!(filed.len(), 2, "{wire:?}: one per section that ran");
        // Sorted by identifier, so `…root.build.1` comes before `…root.shape.1`.
        let (build_id, build) = &filed[0];
        let (shape_id, shape) = &filed[1];
        // `<flow-run-id>.<path>.<attempt>`, and both sections of one walk share the first part.
        let walk_id = shape_id
            .split_once(".root.")
            .expect("the identifier names its section")
            .0;
        assert_eq!(build_id, &format!("{walk_id}.root.build.1"), "{wire:?}");
        assert_eq!(shape_id, &format!("{walk_id}.root.shape.1"), "{wire:?}");

        assert_eq!(shape["turns"], 2, "{wire:?}: two steps, one conversation");
        assert_eq!(build["turns"], 1, "{wire:?}");
        // What crosses the boundary, and what does not.
        assert!(
            first_turn(build).contains("Earlier sections established:\nspecification_id: SPEC-1"),
            "{wire:?}: {}",
            first_turn(build)
        );
        assert!(
            !serde_json::to_string(&shape["items"])
                .expect("encodable")
                .contains("Earlier sections established"),
            "{wire:?}: the first section had nothing handed to it"
        );
        assert!(
            !serde_json::to_string(&build["items"])
                .expect("encodable")
                .contains("State the required behaviour"),
            "{wire:?}: a sibling's transcript never reaches another scope"
        );
    }
}

#[test]
fn a_step_that_answers_failed_skips_what_needed_it_and_the_flow_exits_2() {
    for wire in WIRES {
        let fixture = wire.fixture("flow-fails-second");
        let workspace = workspace();
        let dir = tempfile::tempdir().expect("a temporary directory");
        let flow = flow_file(dir.path(), "flow.yaml", TWO_SECTIONS);
        let sessions = tempfile::tempdir().expect("a temporary directory");

        let output = walk(
            &fixture,
            wire,
            &flow,
            workspace.path(),
            sessions.path(),
            &["--json"],
        );

        assert_eq!(
            output.status,
            Some(2),
            "{wire:?}: a flow that finished and did not come out clean: {}",
            output.stderr
        );
        let events = events(&output);
        // The second step ran and failed; the third never ran at all.
        assert_eq!(
            steps_started(&events),
            vec!["root.shape.specify", "root.shape.decompose"],
            "{wire:?}"
        );
        let skipped: Vec<&str> = events
            .iter()
            .filter(|event| event["kind"] == "node-skipped")
            .filter_map(|event| event["path"].as_str())
            .collect();
        assert!(
            skipped.contains(&"root.build") && skipped.contains(&"root.build.implement"),
            "{wire:?}: what needed the failed section is named as skipped: {skipped:?}"
        );
        let last = events.last().expect("a terminal event");
        assert_eq!(last["kind"], "flow-finished", "{wire:?}: {last}");
        assert_eq!(last["clean"], false, "{wire:?}: {last}");
        assert_eq!(last["failed"], 1, "{wire:?}: {last}");
    }
}

#[test]
fn a_section_that_did_not_come_out_clean_is_re_entered_and_files_a_session_per_attempt() {
    // The retreat, through the shipped binary. `--max-attempts 2` bounds a document that carries
    // no bound of its own; the second step fails once, the whole section runs again from nothing
    // but what crossed into it, and the walk comes out clean.
    for wire in WIRES {
        let fixture = wire.fixture("flow-fails-second");
        let workspace = workspace();
        let dir = tempfile::tempdir().expect("a temporary directory");
        let flow = flow_file(dir.path(), "flow.yaml", TWO_SECTIONS);
        let sessions = tempfile::tempdir().expect("a temporary directory");

        let output = walk(
            &fixture,
            wire,
            &flow,
            workspace.path(),
            sessions.path(),
            &["--json", "--max-attempts", "2"],
        );

        assert_eq!(output.status, Some(0), "{wire:?}: {}", output.stderr);
        let events = events(&output);
        assert_eq!(
            events
                .iter()
                .filter(|event| event["kind"] == "group-repeating")
                .count(),
            1,
            "{wire:?}: a reader must be able to tell a retreat from a duplicate"
        );
        let last = events.last().expect("a terminal event");
        assert_eq!(last["clean"], true, "{wire:?}: {last}");
        assert_eq!(
            last["retreats"], 1,
            "{wire:?}: the tally keeps the attempt that failed: {last}"
        );

        let filed = sessions_in(sessions.path());
        let names: Vec<&str> = filed.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(
            filed.len(),
            3,
            "{wire:?}: one per attempt of a section: {names:?}"
        );
        assert!(names[0].ends_with(".root.build.1"), "{names:?}");
        assert!(names[1].ends_with(".root.shape.1"), "{names:?}");
        assert!(names[2].ends_with(".root.shape.2"), "{names:?}");
        // A new attempt starts from `available` and nothing else, which is what re-running the
        // whole scope means.
        assert_eq!(
            filed[2].1["items"].as_array().expect("items").len(),
            filed[1].1["items"].as_array().expect("items").len(),
            "{wire:?}: the second attempt did not continue the first one's conversation"
        );
        assert!(
            first_turn(&filed[2].1).contains("attempt 2 of section `root.shape`"),
            "{wire:?}: and it says which attempt it is: {}",
            first_turn(&filed[2].1)
        );
    }
}

#[test]
fn the_projected_adp_workflow_walks_end_to_end() {
    // The document engineering-protocols projects from `adp/default/2`, unedited, read off disk.
    let flow = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("harness-flow")
        .join("fixtures")
        .join("adp-default.projected.yaml");
    for wire in WIRES {
        let fixture = wire.fixture("flow-passes");
        let workspace = workspace();
        let sessions = tempfile::tempdir().expect("a temporary directory");

        let output = walk(
            &fixture,
            wire,
            &flow,
            workspace.path(),
            sessions.path(),
            &["--json"],
        );

        assert_eq!(output.status, Some(0), "{wire:?}: {}", output.stderr);
        let events = events(&output);
        assert_eq!(
            steps_started(&events),
            vec![
                "root.receive",
                "root.specify",
                "root.decompose",
                "root.establish_verifiers",
                "root.implement-to-review.implement",
                "root.implement-to-review.verify",
                "root.implement-to-review.adversarial_verify",
                "root.implement-to-review.review",
            ],
            "{wire:?}: the projection's own order, retreat section included"
        );
        let last = events.last().expect("a terminal event");
        assert_eq!(last["kind"], "flow-finished", "{wire:?}: {last}");
        assert_eq!(last["clean"], true, "{wire:?}: {last}");
        assert_eq!(last["retreats"], 0, "{wire:?}: nothing needed a second go");

        let filed = sessions_in(sessions.path());
        let names: Vec<&str> = filed.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(
            filed.len(),
            2,
            "{wire:?}: one per section that ran, and the root is one of them: {names:?}"
        );
        assert!(
            names.iter().any(|id| id.ends_with(".root.1"))
                && names
                    .iter()
                    .any(|id| id.ends_with(".root.implement-to-review.1")),
            "{wire:?}: {names:?}"
        );
    }
}

#[test]
fn a_group_that_never_answers_what_it_promised_fails_once_and_is_not_re_entered() {
    // `gives` is a contract the *document* wrote down, and a second attempt cannot make it truer:
    // the section came out clean and still did not produce the name it declared. Re-entering it
    // would buy the same answer again at full price. A caller who wants that retreat has the leave
    // gate, where somebody decided it.
    for wire in WIRES {
        let fixture = wire.fixture("flow-passes");
        let workspace = workspace();
        let dir = tempfile::tempdir().expect("a temporary directory");
        let flow = flow_file(dir.path(), "flow.yaml", PROMISES_MORE_THAN_IT_GIVES);
        let sessions = tempfile::tempdir().expect("a temporary directory");

        let output = walk(
            &fixture,
            wire,
            &flow,
            workspace.path(),
            sessions.path(),
            &["--json"],
        );

        assert_eq!(output.status, Some(2), "{wire:?}: {}", output.stderr);
        let events = events(&output);
        assert_eq!(
            steps_started(&events),
            vec!["root.shape.specify", "root.shape.decompose"],
            "{wire:?}: one pass, though the document allowed three"
        );
        let incomplete = of_kind(&events, "handoff-incomplete");
        assert_eq!(incomplete.len(), 1, "{wire:?}: said once: {incomplete:?}");
        assert_eq!(incomplete[0]["path"], "root.shape", "{wire:?}");
        assert_eq!(
            incomplete[0]["missing"],
            serde_json::json!(["approval_id"]),
            "{wire:?}: {}",
            incomplete[0]
        );
        assert!(
            of_kind(&events, "group-repeating").is_empty(),
            "{wire:?}: nothing went round again: {:?}",
            kinds(&events)
        );
        let left = of_kind(&events, "group-left")
            .into_iter()
            .find(|event| event["path"] == "root.shape")
            .expect("the section was left");
        assert_eq!(left["failed"], true, "{wire:?}: {left}");
        assert_eq!(left["attempts"], 1, "{wire:?}: {left}");
        assert_eq!(
            left["exhausted"], false,
            "{wire:?}: it never asked for a second attempt, so it used no bound up: {left}"
        );
    }

    // On a terminal the same fact has to be readable as what it is: *which* name was never given.
    // `handoff ✗ root.shape` alone sends a reader to the document to guess.
    let wire = Wire::Responses;
    let fixture = wire.fixture("flow-passes");
    let workspace = workspace();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let flow = flow_file(dir.path(), "flow.yaml", PROMISES_MORE_THAN_IT_GIVES);
    let sessions = tempfile::tempdir().expect("a temporary directory");

    let output = walk(
        &fixture,
        wire,
        &flow,
        workspace.path(),
        sessions.path(),
        &[],
    );
    assert_eq!(output.status, Some(2), "stderr: {}", output.stderr);
    assert!(
        output
            .stderr
            .contains("handoff ✗ root.shape: never gave approval_id"),
        "the line names the section and the name it never gave: {}",
        output.stderr
    );
}

/// A canary that lives **outside** the workspace. A step that got hold of it would put it in a
/// session file and on a stream, which is what the assertions below look for.
const OUTSIDE_CANARY: &str = "ESCAPED-THE-WORKSPACE-9d1f2a";

#[test]
fn a_context_name_that_leaves_the_workspace_fails_the_step_and_asks_no_model() {
    // A `context` entry is a path out of a *generated* document, and `--workspace` is the sentence
    // saying what this run may see. `workspace.join(name)` drops the base the moment the name is
    // absolute, and `..` walks out of it — so a projection could have named a key file or an
    // environment file and had it read into a model turn, past the confinement the tools are under.
    // Both shapes fail the step by name, before anything is sent.
    let wire = Wire::Responses;
    let outer = tempfile::tempdir().expect("a temporary directory");
    let workspace = outer.path().join("workspace");
    fs::create_dir(&workspace).expect("create the workspace");
    fs::write(workspace.join("README.md"), "hello harness\n").expect("write");
    let secret = outer.path().join("outside.txt");
    fs::write(&secret, format!("{OUTSIDE_CANARY}\n")).expect("write the canary");

    let dir = tempfile::tempdir().expect("a temporary directory");
    let record = dir.path().join("requests.jsonl");
    let fixture = wire.recording("flow-passes", Some(&record));
    let flow = flow_file(
        dir.path(),
        "flow.yaml",
        &format!(
            "id: escaping
root:
  id: root
  nodes:
    - id: climbing
      run:
        state: climbing
        summary: \"Read what you were given.\"
        context: [\"../outside.txt\"]
    - id: absolute
      run:
        state: absolute
        summary: \"Read what you were given.\"
        context: [\"{}\"]
",
            secret.display()
        ),
    );
    let sessions = tempfile::tempdir().expect("a temporary directory");

    let output = walk(
        &fixture,
        wire,
        &flow,
        &workspace,
        sessions.path(),
        &["--json"],
    );

    assert_eq!(output.status, Some(2), "stderr: {}", output.stderr);
    let events = events(&output);
    let finished = of_kind(&events, "step-finished");
    assert_eq!(finished.len(), 2, "{finished:?}");
    assert!(
        finished.iter().all(|event| event["failed"] == true),
        "both steps failed by name: {finished:?}"
    );
    let refused: Vec<&Value> = of_kind(&events, "warning")
        .into_iter()
        .filter(|event| event["code"] == "context-refused")
        .collect();
    assert_eq!(
        refused.len(),
        2,
        "one per step, under a code of its own: {:?}",
        of_kind(&events, "warning")
    );
    let resolved = workspace.canonicalize().expect("the workspace is there");
    for warning in &refused {
        let message = warning["message"].as_str().expect("a message");
        assert!(
            message.contains("outside.txt"),
            "the name it refused: {message}"
        );
        assert!(
            message.contains(&resolved.display().to_string()),
            "and the workspace it is outside of: {message}"
        );
    }
    // The whole claim, at the far end of the socket: two steps, no request.
    assert_eq!(
        requests(&record),
        0,
        "a step whose context this run may not read asks no model anything"
    );

    // And the file's contents are nowhere: not in a session, not on either stream.
    assert!(!output.stdout.contains(OUTSIDE_CANARY), "{}", output.stdout);
    assert!(!output.stderr.contains(OUTSIDE_CANARY), "{}", output.stderr);
    for (id, session) in sessions_in(sessions.path()) {
        assert!(
            !serde_json::to_string(&session)
                .expect("encodable")
                .contains(OUTSIDE_CANARY),
            "{id} holds what the run was refused"
        );
    }
}

#[test]
fn an_interrupted_walk_files_what_ran_and_emits_no_ending() {
    // The `Cancelled` row of design 0003 § 2. Somebody stopped it: that is neither a failed step
    // nor a broken wire, so the walk stops where it stands, what it had bought is filed, and
    // **no `flow-finished` is emitted** — an ending nobody reached would be a record claiming the
    // walk finished. Exit `2`, because a flow that did not come out clean is what happened.
    let wire = Wire::Responses;
    // Paced so the interrupt lands mid-stream. A cancel that only ever arrives between turns proves
    // nothing about the case that matters.
    let fixture = wire.fixture("slow");
    let workspace = workspace();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let flow = flow_file(dir.path(), "flow.yaml", TWO_SECTIONS);
    let sessions = tempfile::tempdir().expect("a temporary directory");

    let mut child = Command::new(BINARY)
        .args([
            "workflow",
            "run",
            "--base-url",
            &fixture.base_url,
            "--model",
            "b10x-emulated",
            "--api-key-env",
            "B10X_HARNESS_TEST_KEY",
            "--workspace",
            workspace.path().to_str().expect("utf-8 path"),
            "--session-dir",
            sessions.path().to_str().expect("utf-8 path"),
            "--flow",
            flow.to_str().expect("utf-8 path"),
            "--input",
            "add a CSV export",
            "--json",
        ])
        .env("B10X_HARNESS_TEST_KEY", "test-key")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the binary runs");

    // Wait until the first step is genuinely under way, so the signal reaches a run that is
    // blocked on the model rather than one still resolving its flags.
    let mut record = BufReader::new(child.stdout.take().expect("piped stdout"));
    let mut stdout = String::new();
    loop {
        let mut line = String::new();
        let read = record.read_line(&mut line).expect("the record is readable");
        assert!(read > 0, "the walk ended before a step started: {stdout}");
        stdout.push_str(&line);
        if line.contains("\"step-started\"") {
            break;
        }
    }

    let signalled = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(signalled.success(), "the interrupt was delivered");

    record
        .read_to_string(&mut stdout)
        .expect("the rest of the record");
    let status = child.wait().expect("the walk ends");
    assert_eq!(
        status.code(),
        Some(2),
        "a walk somebody stopped did not come out clean: {stdout}"
    );

    let events: Vec<Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|error| panic!("{line}: {error}")))
        .collect();
    assert!(
        of_kind(&events, "flow-finished").is_empty(),
        "there was no ending to report: {:?}",
        kinds(&events)
    );
    assert!(
        of_kind(&events, "warning")
            .iter()
            .any(|event| event["code"] == "flow-cancelled"),
        "and the record says why it stops here: {:?}",
        of_kind(&events, "warning")
    );

    // What it had bought is still on disk, under the section that was open when it was stopped.
    let filed = sessions_in(sessions.path());
    let names: Vec<&str> = filed.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(filed.len(), 1, "{names:?}");
    assert!(names[0].ends_with(".root.shape.1"), "{names:?}");
    assert!(
        first_turn(&filed[0].1).contains("State the required behaviour"),
        "with the turn it had already bought: {}",
        first_turn(&filed[0].1)
    );
}

#[test]
fn workflow_plan_prints_the_layers_without_an_endpoint() {
    // No `--base-url`, no credential, no socket: this answers *does it validate and what runs* for
    // free, the same way `tools` answers *what would this run publish*.
    let flow = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("harness-flow")
        .join("fixtures")
        .join("adp-default.projected.yaml");

    let output = raw(&[
        "workflow",
        "plan",
        "--flow",
        flow.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert!(output.stdout.contains("adp/default"), "{}", output.stdout);
    assert!(
        output.stdout.contains("1. receive"),
        "one line per layer: {}",
        output.stdout
    );
    assert!(
        output
            .stdout
            .contains("root.implement-to-review (repeat: max 3)"),
        "the bound beside the group it belongs to: {}",
        output.stdout
    );

    // The same answer as a document, and the flag that rewrites every bound.
    let bounded = raw(&[
        "workflow",
        "plan",
        "--flow",
        flow.to_str().expect("utf-8 path"),
        "--max-attempts",
        "2",
        "--json",
    ]);
    assert_eq!(bounded.status, Some(0), "stderr: {}", bounded.stderr);
    let plan: Value = serde_json::from_str(bounded.stdout.trim()).expect("one JSON document");
    assert_eq!(plan["kind"], "plan");
    assert_eq!(plan["flow"], "adp/default");
    assert_eq!(plan["plan"]["layers"][0][0], "receive");
    assert_eq!(
        plan["plan"]["groups"]["implement-to-review"]["attempts"], 2,
        "the document says 3 and the command line says 2: {plan}"
    );
    assert_eq!(
        plan["plan"]["attempts"], 2,
        "and *every* section means the root as well: {plan}"
    );
}

#[test]
fn max_attempts_overrides_the_roots_own_bound_like_every_other_sections() {
    // The flag's word is `every`, and four documents say so. The root is a group like any other,
    // and the one that holds the steps of a flat document: skipping it meant `--max-attempts` on a
    // projection with no sub-sections bounded nothing at all.
    let dir = tempfile::tempdir().expect("a temporary directory");
    let flow = flow_file(
        dir.path(),
        "flow.yaml",
        "id: rooted\nroot:\n  id: root\n  repeat: {max: 5}\n  nodes:\n    - id: one\n",
    );
    let path = flow.to_str().expect("utf-8 path");

    let bounded = raw(&[
        "workflow",
        "plan",
        "--flow",
        path,
        "--max-attempts",
        "2",
        "--json",
    ]);
    assert_eq!(bounded.status, Some(0), "stderr: {}", bounded.stderr);
    let plan: Value = serde_json::from_str(bounded.stdout.trim()).expect("one JSON document");
    assert_eq!(
        plan["plan"]["attempts"], 2,
        "the document says 5 and the command line says 2: {plan}"
    );

    // And without the flag the document's own bound is what stands.
    let untouched = raw(&["workflow", "plan", "--flow", path, "--json"]);
    assert_eq!(untouched.status, Some(0), "stderr: {}", untouched.stderr);
    let plan: Value = serde_json::from_str(untouched.stdout.trim()).expect("one JSON document");
    assert_eq!(plan["plan"]["attempts"], 5, "{plan}");
}

#[test]
fn a_flow_that_does_not_validate_is_refused_before_any_session() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let workspace = workspace();
    // A cycle, which the notation refuses by naming the nodes it goes through.
    let flow = flow_file(dir.path(), "cycle.yaml", CYCLE);
    let sessions = dir.path().join("sessions");

    let output = raw(&[
        "workflow",
        "run",
        "--base-url",
        "http://127.0.0.1:1/v1",
        "--model",
        "b10x-emulated",
        "--api-key-env",
        "B10X_HARNESS_TEST_KEY",
        "--workspace",
        workspace.path().to_str().expect("utf-8 path"),
        "--session-dir",
        sessions.to_str().expect("utf-8 path"),
        "--flow",
        flow.to_str().expect("utf-8 path"),
        "--input",
        "add a CSV export",
        "--json",
    ]);

    assert_eq!(output.status, Some(1), "stdout: {}", output.stdout);
    let refused: Value =
        serde_json::from_str(output.stdout.trim()).expect("one line saying the run never started");
    assert_eq!(refused["kind"], "refused");
    let reason = refused["reason"].as_str().expect("a reason");
    assert!(reason.contains("holds a cycle"), "{reason}");
    assert!(
        reason.contains("a -> b"),
        "naming what it goes through: {reason}"
    );
    assert!(
        !sessions.exists(),
        "a document that never ran leaves no session directory behind"
    );

    // `plan` refuses the same document with the same words, so a person checks it for free.
    let planned = raw(&[
        "workflow",
        "plan",
        "--flow",
        flow.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(planned.status, Some(1), "stdout: {}", planned.stdout);
    assert!(
        planned.stderr.contains("holds a cycle"),
        "{}",
        planned.stderr
    );
}

#[test]
fn a_wire_failure_aborts_the_flow_and_files_what_ran() {
    // A broken wire is nobody's failed step: the walk aborts with `1` rather than reporting a
    // network blip as a section that did not come out clean. What it bought first is still filed.
    for wire in WIRES {
        let fixture = wire.fixture("flow-dies-mid-step");
        let workspace = workspace();
        let dir = tempfile::tempdir().expect("a temporary directory");
        let flow = flow_file(dir.path(), "flow.yaml", TWO_SECTIONS);
        let sessions = tempfile::tempdir().expect("a temporary directory");

        let output = walk(
            &fixture,
            wire,
            &flow,
            workspace.path(),
            sessions.path(),
            &[],
        );

        assert_eq!(
            output.status,
            Some(1),
            "{wire:?}: aborted, not merely unclean: {}",
            output.stderr
        );
        assert!(
            output.stderr.contains("error:"),
            "{wire:?}: {}",
            output.stderr
        );

        let filed = sessions_in(sessions.path());
        assert_eq!(
            filed.len(),
            1,
            "{wire:?}: the section that was open when the wire broke: {filed:?}"
        );
        assert!(
            filed[0].0.ends_with(".root.shape.1"),
            "{wire:?}: {}",
            filed[0].0
        );
        assert!(
            first_turn(&filed[0].1).contains("State the required behaviour"),
            "{wire:?}: with the turn it had already bought"
        );
    }
}

#[test]
fn resuming_a_flow_is_refused_by_name_because_a_flow_has_a_session_per_section() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let workspace = workspace();
    let flow = flow_file(dir.path(), "flow.yaml", TWO_SECTIONS);

    let output = raw(&[
        "workflow",
        "run",
        "--base-url",
        "http://127.0.0.1:1/v1",
        "--model",
        "b10x-emulated",
        "--api-key-env",
        "B10X_HARNESS_TEST_KEY",
        "--workspace",
        workspace.path().to_str().expect("utf-8 path"),
        "--session-dir",
        dir.path().to_str().expect("utf-8 path"),
        "--flow",
        flow.to_str().expect("utf-8 path"),
        "--input",
        "add a CSV export",
        "--resume",
        "latest",
        "--json",
    ]);

    assert_eq!(output.status, Some(1), "stdout: {}", output.stdout);
    let refused: Value =
        serde_json::from_str(output.stdout.trim()).expect("one line saying the run never started");
    assert_eq!(refused["kind"], "refused");
    let reason = refused["reason"].as_str().expect("a reason");
    assert!(reason.contains("`--resume`"), "named: {reason}");
    assert!(
        reason.contains("one per section"),
        "and why a flow has no single conversation to continue: {reason}"
    );
}

#[test]
fn a_document_this_build_does_not_read_is_refused_by_its_extension() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let flow = flow_file(dir.path(), "flow.toml", "id = 'nope'\n");
    let output = raw(&[
        "workflow",
        "plan",
        "--flow",
        flow.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(output.status, Some(1), "stdout: {}", output.stdout);
    assert!(output.stderr.contains("`.toml`"), "{}", output.stderr);
    assert!(output.stderr.contains(".yaml"), "{}", output.stderr);
}

#[test]
fn a_step_payload_that_is_not_an_object_is_refused_before_anything_runs() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let flow = flow_file(
        dir.path(),
        "flow.yaml",
        "id: f\nroot:\n  id: root\n  nodes:\n    - id: one\n      run: \"do the thing\"\n",
    );
    let output = raw(&[
        "workflow",
        "plan",
        "--flow",
        flow.to_str().expect("utf-8 path"),
        "--json",
    ]);
    assert_eq!(output.status, Some(1), "stdout: {}", output.stdout);
    let refused: Value =
        serde_json::from_str(output.stdout.trim()).expect("one line saying it was refused");
    let reason = refused["reason"].as_str().expect("a reason");
    assert!(reason.contains("`root.one`"), "{reason}");
    assert!(reason.contains("a string"), "{reason}");
}
