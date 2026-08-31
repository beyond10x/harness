//! The shipped binary, against a real endpoint, over a real workspace.
//!
//! This is the composition the unit tests cannot reach: argument parsing, credential resolution,
//! the HTTP client, the loop, the read-only tools and the renderer, all at once. If this passes,
//! `b10x-harness run` works.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

struct Fixture {
    child: Child,
    base_url: String,
}

impl Fixture {
    fn start(scenario: &str) -> Self {
        Self::of("harness-responses", "fake_responses.py", scenario)
    }

    /// The same fixture, pointed at the second wire's emulator.
    fn messages(scenario: &str) -> Self {
        Self::of("harness-messages", "fake_messages.py", scenario)
    }

    fn of(crate_name: &str, script: &str, scenario: &str) -> Self {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(crate_name)
            .join("tests")
            .join("fixtures")
            .join(script);
        let mut child = Command::new("python3")
            .arg(&script)
            .arg("--scenario")
            .arg(scenario)
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

/// A workspace with one readable file.
fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut file = fs::File::create(dir.path().join("README.md")).expect("create");
    file.write_all(b"hello harness\n").expect("write");
    dir
}

struct Output {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run(arguments: &[&str], workspace: &Path) -> Output {
    let output = Command::new(BINARY)
        .args(arguments)
        .arg("--workspace")
        .arg(workspace)
        .output()
        .expect("the binary runs");
    Output {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// A run against the fixture that leaves nothing on the machine.
///
/// `--no-session` by default: every one of these would otherwise write a transcript into the
/// operator's own `$XDG_STATE_HOME`, and a test suite that files real sessions on the machine it
/// runs on is one nobody can run twice. The session tests below name a directory instead.
fn run_against(fixture: &Fixture, extra: &[&str], workspace: &Path) -> Output {
    let mut arguments = vec![
        "run",
        "--base-url",
        &fixture.base_url,
        "--model",
        "b10x-emulated",
        "--api-key-env",
        "B10X_HARNESS_TEST_KEY",
        "--no-session",
        "--input",
        "read the readme and tell me what it says",
    ];
    arguments.extend_from_slice(extra);
    let output = Command::new(BINARY)
        .args(&arguments)
        .arg("--workspace")
        .arg(workspace)
        .env("B10X_HARNESS_TEST_KEY", "test-key")
        .output()
        .expect("the binary runs");
    Output {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

#[test]
fn the_binary_answers_and_puts_only_the_answer_on_stdout() {
    let fixture = Fixture::start("text");
    let workspace = workspace();
    let output = run_against(&fixture, &[], workspace.path());

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert_eq!(output.stdout.trim(), "provider emulation passed");
    assert!(
        output.stderr.contains("file_read, dir_list, search, find"),
        "progress names what the model was offered, which by default is the catalogue itself: {}",
        output.stderr
    );
}

#[test]
fn the_binary_reads_a_real_file_by_calling_the_tool_directly() {
    // The default surface. The model names `file_read` and passes the entry's own arguments;
    // nothing is discovered first and no verb carries it.
    let fixture = Fixture::start("flat-tool");
    let workspace = workspace();
    let output = run_against(&fixture, &[], workspace.path());

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert!(
        output.stderr.contains("→ file_read"),
        "the call is reported under the entry's own name: {}",
        output.stderr
    );
    assert!(output.stderr.contains("← ok"), "{}", output.stderr);
    assert_eq!(output.stdout.trim(), "The file says: hello harness");
}

#[test]
fn the_binary_reads_a_real_file_through_a_real_tool_call() {
    // The verbs surface, which metaharness serves over MCP and an evaluation arm asks for by
    // name. Same catalogue, same file, one flag different.
    let fixture = Fixture::start("tool");
    let workspace = workspace();
    let output = run_against(&fixture, &["--surface", "verbs"], workspace.path());

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert!(
        output.stderr.contains("→ tool_invoke"),
        "the call is reported: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("← ok"),
        "the result is reported: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("usage 42 in"),
        "reported usage is surfaced: {}",
        output.stderr
    );
}

#[test]
fn json_mode_emits_one_event_per_line() {
    let fixture = Fixture::start("flat-tool");
    let workspace = workspace();
    let output = run_against(&fixture, &["--json"], workspace.path());

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    let events: Vec<Value> = output
        .stdout
        .lines()
        .map(|line| serde_json::from_str(line).expect("each line is one event"))
        .collect();
    let kinds: Vec<&str> = events
        .iter()
        .filter_map(|event| event["kind"].as_str())
        .collect();
    assert_eq!(kinds.first(), Some(&"started"));
    assert_eq!(kinds.last(), Some(&"finished"));
    assert!(kinds.contains(&"tool-requested"), "{kinds:?}");
    assert!(kinds.contains(&"tool-completed"), "{kinds:?}");
    assert!(kinds.contains(&"usage"), "{kinds:?}");
    assert_eq!(
        events.last().expect("a terminal event")["stop"]["kind"],
        serde_json::json!("completed")
    );
}

#[test]
fn a_run_that_stops_without_an_answer_exits_distinctly_from_a_failure() {
    let fixture = Fixture::start("incomplete");
    let workspace = workspace();
    let output = run_against(&fixture, &[], workspace.path());

    assert_eq!(
        output.status,
        Some(2),
        "a named stop is neither success nor failure: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("ProviderIncomplete"),
        "{}",
        output.stderr
    );
}

#[test]
fn a_rejected_credential_fails_with_a_message_naming_the_cause() {
    let fixture = Fixture::start("unauthorized");
    let workspace = workspace();
    let output = run_against(&fixture, &[], workspace.path());

    assert_eq!(output.status, Some(1), "stderr: {}", output.stderr);
    assert!(
        output.stderr.to_lowercase().contains("unauthorized"),
        "{}",
        output.stderr
    );
}

#[test]
fn naming_no_credential_source_reaches_the_endpoint_unauthenticated() {
    // The declaration a gateway on this machine needs, and the one `--credentials none` becomes
    // when metaharness launches this loop. It is not a refusal: the request goes out with no
    // `authorization` header and the far end decides. Here nothing is listening, so what comes
    // back is a transport failure — proof the run got as far as the socket.
    let workspace = workspace();
    let output = run(
        &[
            "run",
            "--base-url",
            "http://127.0.0.1:1/v1",
            "--model",
            "m",
            "--no-session",
            "--input",
            "hi",
        ],
        workspace.path(),
    );
    assert_eq!(output.status, Some(1));
    assert!(
        output.stderr.contains("posting to"),
        "it tried, rather than refusing itself: {}",
        output.stderr
    );
    assert!(!output.stderr.contains("exactly one"), "{}", output.stderr);
}

#[test]
fn the_tools_subcommand_describes_the_toolset_without_an_endpoint() {
    let workspace = workspace();
    let output = run(&["tools"], workspace.path());

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    let described: Value = serde_json::from_str(&output.stdout).expect("valid JSON");
    let names: Vec<&str> = described["tools"]
        .as_array()
        .expect("a tool array")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(
        names,
        vec!["file_read", "dir_list", "search", "find"],
        "by default the model is offered the catalogue entries themselves"
    );
    let entries: Vec<&str> = described["catalogue"]["tools"]
        .as_array()
        .expect("a catalogue")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(entries, vec!["file_read", "dir_list", "search", "find"]);
    assert!(
        described["tools"]
            .as_array()
            .expect("a tool array")
            .iter()
            .all(|tool| tool["approval"] == serde_json::json!("not-required")),
        "the shipped toolset is read-only: {}",
        output.stdout
    );
}

#[test]
fn the_verbs_surface_publishes_three_tools_over_the_same_catalogue() {
    // The other surface, unchanged and still fully served. What differs is the publication, not
    // what the run may do: the catalogue behind the three verbs is the same four entries.
    let workspace = workspace();
    let output = run(&["tools", "--surface", "verbs"], workspace.path());

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    let described: Value = serde_json::from_str(&output.stdout).expect("valid JSON");
    let names: Vec<&str> = described["tools"]
        .as_array()
        .expect("a tool array")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(names, vec!["tool_search", "tool_describe", "tool_invoke"]);
    let entries: Vec<&str> = described["catalogue"]["tools"]
        .as_array()
        .expect("a catalogue")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(entries, vec!["file_read", "dir_list", "search", "find"]);
}

#[test]
fn the_binary_drives_the_second_wire_and_calls_a_real_tool_through_it() {
    // Phase 3's exit criterion at the outermost layer: the shipped binary, one flag different,
    // against the other endpoint. Everything between the flag and the answer — the loop, the
    // catalogue, the tools, the renderer — is the same code, which is the whole claim the second
    // wire was built to test.
    let fixture = Fixture::messages("tool");
    let workspace = workspace();
    let output = run_against(
        &fixture,
        &[
            "--wire",
            "anthropic-messages",
            "--surface",
            "verbs",
            "--yes",
        ],
        workspace.path(),
    );

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert_eq!(output.stdout.trim(), "The file says: hello harness");
}

#[test]
fn a_thinking_round_trip_completes_through_the_shipped_binary() {
    // The second wire's opaque item, all the way out and back through the real binary: the
    // `reasoning` scenario answers turn one with a `thinking` block and a tool call, and only
    // completes turn two if the block was replayed byte for byte at the head of the assistant
    // message. A wire that dropped it or reordered it ends here instead.
    let fixture = Fixture::messages("reasoning");
    let workspace = workspace();
    let output = run_against(
        &fixture,
        &[
            "--wire",
            "anthropic-messages",
            "--surface",
            "verbs",
            "--yes",
            "--json",
        ],
        workspace.path(),
    );
    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);

    let usage: Vec<Value> = output
        .stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|event| event["kind"] == "usage")
        .collect();
    assert!(
        !usage.is_empty(),
        "no usage reached the record: {}",
        output.stdout
    );
    // 42 fresh plus 7 read from cache. This wire reports the two disjointly and the projection
    // sums them, because the neutral `input_tokens` is the whole and cached is a part of it — a
    // run that reported 42 here would price every cached turn low.
    assert_eq!(usage[0]["input_tokens"], 49, "{}", usage[0]);
    assert_eq!(
        usage[0]["cached_input_tokens"],
        Value::from(7),
        "{}",
        usage[0]
    );
}

/// A tree substrate's embedded driver can adopt: the named directory, under a root it can own.
///
/// The directory *is* the workspace — `--workspace`'s parent becomes substrate's root — so the
/// temporary root has to outlive the call, which is why both come back.
fn adoptable_workspace(name: &str) -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().expect("a temporary directory");
    let workspace = root.path().join(name);
    fs::create_dir(&workspace).expect("create the workspace directory");
    (root, workspace)
}

#[test]
fn tools_over_an_adopted_embedded_workspace_publishes_the_writing_entries() {
    let (_root, workspace) = adoptable_workspace("ws_probe");
    let output = run(&["tools", "--substrate-embedded"], &workspace);

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    let described: Value = serde_json::from_str(&output.stdout).expect("valid JSON");
    let entries: Vec<&str> = described["catalogue"]["tools"]
        .as_array()
        .expect("a catalogue")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert!(entries.contains(&"file_write"), "{entries:?}");
    assert!(entries.contains(&"file_edit"), "{entries:?}");
    // No delegated cgroup and no declared program, so this machine confines no process and the
    // catalogue says so by not holding the entry.
    assert!(!entries.contains(&"run"), "{entries:?}");

    let names: Vec<&str> = described["tools"]
        .as_array()
        .expect("a tool array")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert!(names.contains(&"file_write"), "{names:?}");
    assert_eq!(
        names.len(),
        entries.len(),
        "under the default surface the published list and the catalogue are one list: {names:?}"
    );
}

#[test]
fn an_embedded_workspace_with_the_wrong_name_refuses_the_run_by_name() {
    // **`not_a_ws` used to be the wrong name and is now a right one.** Since substrate 0.2.2 a
    // workspace is named by its own directory rather than by the `ws_` id scheme, so what is left
    // to refuse is a name that is not one path component — here a dot, which would let the guarded
    // filesystem be asked for something other than a child of its root.
    let (_root, workspace) = adoptable_workspace("bad.name");
    let output = run(&["tools", "--substrate-embedded"], &workspace);

    assert_eq!(output.status, Some(1), "stdout: {}", output.stdout);
    assert!(
        output.stderr.contains("one path component"),
        "{}",
        output.stderr
    );
    assert!(output.stderr.contains("bad.name"), "{}", output.stderr);
    // The failure this replaces: a silent read-only catalogue, which the operator asked to write
    // into and the model then reported as done without writing anything.
    assert!(
        serde_json::from_str::<Value>(&output.stdout).is_err(),
        "nothing was published: {}",
        output.stdout
    );
}

/// The workspace-name rule every page states admits exactly the names this binary adopts.
///
/// Both pages once told the operator the directory "must therefore be named `ws_something` —
/// substrate's guarded filesystem will not represent any other name". `0c31438` shipped workspace
/// adoption and left the sentence standing; substrate `0.2.2`'s `validate_root_name` asks for one
/// path component of ASCII alphanumerics, `_` and `-` that is not empty, `.` or `..` and does not
/// begin with `-`, and the `ws_` prefix it used to demand was the id scheme rather than the
/// containment.
///
/// # Why this reads the sentence instead of looking for it
///
/// Asserting that a page does not say `ws_` and does say "one path component" is satisfied by a
/// page that names the wrong characters. Two mutants proved it: both copies changed to
/// "alphanumerics, `_` and `.`" left the suite green while telling the operator that `my.project`
/// is fine — it is refused — and that `my-project` is not — it is adopted; and the whole rule left
/// as plain "alphanumerics" left `café` refused by a binary whose page says it is a legal name.
///
/// So the **alphabet the sentence names** is decoded out of it — whether it says `ASCII`, and which
/// characters it lists beside the alphanumerics — and used to classify a set of probe names, each
/// of which is separately handed to the binary. Page and behaviour must agree on every one. A page
/// whose rule this cannot find at all fails rather than passing: a rule stated in some other words
/// is one this case has to be re-derived from, not one it may skip.
///
/// # And on every page, not on the two that share a struct
///
/// `RunOptions` is flattened by `run`, `chat` and `workflow run`; `ToolsOptions` carries a second
/// copy of the same paragraph for `tools`. Four pages, two sources — so a case that asserted on
/// `run` and `chat` left the whole `ToolsOptions` copy unpinned, and reverting it alone put
/// `ws_something` back on `tools --help` with everything green. The pages are enumerated from the
/// pinned argv document instead: every command that records a `--substrate-embedded` row.
///
/// The binary's own refusal is held to the same standard, because it is the sentence an operator
/// reads *after* being refused, and a rule they cannot act on is the failure this story was opened
/// about rather than one it fixed.
#[test]
fn the_workspace_rule_each_page_states_admits_exactly_the_names_this_binary_adopts() {
    // Names chosen to separate the clauses: a hyphen, an underscore, a digit, a dot, three scripts
    // whose letters are not ASCII, and a name that would read as an option.
    const PROBES: [&str; 8] = [
        "my-project",
        "my_project",
        "project9",
        "my.project",
        "café",
        "Projekt-Übung",
        "日本語",
        "-rf",
    ];

    let mut adopts: std::collections::BTreeMap<&str, bool> = std::collections::BTreeMap::new();
    let mut a_refusal = String::new();
    for name in PROBES {
        let (_root, workspace) = adoptable_workspace(name);
        let output = run(&["tools", "--substrate-embedded"], &workspace);
        let adopted = output.status == Some(0);
        if !adopted {
            a_refusal = output.stderr.clone();
        }
        adopts.insert(name, adopted);
    }
    assert!(
        adopts.values().any(|held| *held) && adopts.values().any(|held| !*held),
        "the probes have to fall on both sides of the rule or they measure nothing: {adopts:?}"
    );

    let mut pages: Vec<Vec<String>> = commands_rendering(&["--substrate-embedded"])
        .into_iter()
        .map(|path| {
            let mut words: Vec<String> = path.split_whitespace().map(str::to_owned).collect();
            words.push("--help".to_owned());
            words
        })
        .collect();
    assert!(
        pages.len() >= 2,
        "the pinned document records `--substrate-embedded` on {} command(s); this case is about \
         every page that renders it",
        pages.len()
    );
    pages.sort();

    let mut wrong: Vec<String> = Vec::new();
    let mut stated: Vec<(String, String)> = pages
        .iter()
        .map(|page| {
            let words: Vec<&str> = page.iter().map(String::as_str).collect();
            let help = raw(&words);
            assert_eq!(
                help.status,
                Some(0),
                "`{}`: {}",
                page.join(" "),
                help.stderr
            );
            (page.join(" "), flowed(&help.stdout))
        })
        .collect();
    stated.push(("the refusal itself".to_owned(), flowed(&a_refusal)));

    for (named, text) in &stated {
        if text.contains("ws_") {
            wrong.push(format!(
                "  `{named}` still requires a `ws_` workspace name, which this binary dropped at \
                 substrate 0.2.2"
            ));
            continue;
        }
        let Some(rule) = alphabet_stated_by(text) else {
            wrong.push(format!(
                "  `{named}` states no rule this case can read — it has to say `one path component \
                 of …, and may not …` — so it cannot be compared with what the binary does"
            ));
            continue;
        };
        for (name, adopted) in &adopts {
            let says = rule.admits(name);
            if says != *adopted {
                wrong.push(format!(
                    "  `{named}` says `{name}` is {}, and this binary {} it",
                    if says { "a legal name" } else { "illegal" },
                    if *adopted { "adopts" } else { "refuses" }
                ));
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "the workspace-name rule an operator is shown and the one this binary keeps:\n{}",
        wrong.join("\n")
    );
}

/// The character rule a sentence states, decoded from the sentence's own words.
struct StatedRule {
    /// Whether it says the alphanumerics are `ASCII`, which is what the code checks.
    ascii_only: bool,
    /// The single characters it lists beside them.
    beside: std::collections::BTreeSet<char>,
    /// Whether it refuses a name that would read as an option.
    leading_dash_refused: bool,
}

impl StatedRule {
    /// Whether the rule, as stated, admits this name.
    fn admits(&self, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        if self.leading_dash_refused && name.starts_with('-') {
            return false;
        }
        name.chars().all(|character| {
            (character.is_alphanumeric() && (!self.ascii_only || character.is_ascii()))
                || self.beside.contains(&character)
        })
    }
}

/// The alphabet clause of a stated rule: everything between `path component of` and the
/// `, and may not` that begins the shape rule.
///
/// Scoped to that clause on purpose. `` `-` `` appears again in "may not … begin with `-`", and a
/// reader that took every backticked character in the sentence would count the hyphen as admitted
/// however the alphabet clause was rewritten — which is the mutant this exists to kill.
fn alphabet_stated_by(text: &str) -> Option<StatedRule> {
    let clause = text
        .split_once("path component of")?
        .1
        .split_once(", and may not")?
        .0;
    Some(StatedRule {
        ascii_only: clause.contains("ASCII"),
        beside: clause
            .split('`')
            .skip(1)
            .step_by(2)
            .filter_map(|token| {
                let mut characters = token.chars();
                let single = characters.next()?;
                characters.next().is_none().then_some(single)
            })
            .collect(),
        leading_dash_refused: text.contains("begin with `-`"),
    })
}

/// clap wraps long help to the terminal's width, so a phrase is looked for in flowed text.
fn flowed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every command the pinned argv document records all of these flags on.
///
/// Read from the pin rather than listed here, so a flag that arrives on a fifth command brings its
/// page into this case without anybody remembering to add it.
fn commands_rendering(flags: &[&str]) -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("contracts")
        .join("cli")
        .join("b10x-harness")
        .join(b10x_harness_cli::contract::ARGV_CONTRACT_VERSION)
        .join("argv.json");
    let document: Value = serde_json::from_str(
        &fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("reading `{}`: {error}", path.display())),
    )
    .expect("the pinned document is JSON");
    let mut found: Vec<String> = document["arguments"]
        .as_object()
        .expect("an object")
        .iter()
        .filter(|(_, rows)| {
            let listed = rows.as_array().expect("a list of arguments");
            flags
                .iter()
                .all(|flag| listed.iter().any(|row| row["long"] == *flag))
        })
        .map(|(command, _)| command.clone())
        .collect();
    found.sort();
    found
}

#[test]
fn a_named_socket_with_no_daemon_refuses_rather_than_going_read_only() {
    let (root, workspace) = adoptable_workspace("ws_probe");
    let socket = root.path().join("nothing.sock");
    let output = run(
        &["tools", "--substrate", socket.to_str().expect("utf-8 path")],
        &workspace,
    );

    assert_eq!(output.status, Some(1), "stdout: {}", output.stdout);
    assert!(output.stderr.contains("nothing.sock"), "{}", output.stderr);
}

/// The binary with exactly the arguments given — no `--workspace` appended.
///
/// `run` and `tools` take one and `sessions` does not, and a helper that always appended it could
/// only ever test half the command line.
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

/// One `run` against the fixture, writing its session into a directory the test owns.
fn run_with_session(
    fixture: &Fixture,
    extra: &[&str],
    workspace: &Path,
    sessions: &Path,
) -> Output {
    let mut arguments = vec![
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
        "--input".to_owned(),
        "read the readme and tell me what it says".to_owned(),
    ];
    arguments.extend(extra.iter().map(|argument| (*argument).to_owned()));
    raw(&arguments.iter().map(String::as_str).collect::<Vec<_>>())
}

/// The one session in a directory, parsed.
fn only_session(sessions: &Path) -> Value {
    let mut files: Vec<PathBuf> = fs::read_dir(sessions)
        .expect("the session directory exists")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    assert_eq!(files.len(), 1, "one session was written: {files:?}");
    let path = files.pop().expect("one file");
    serde_json::from_str(&fs::read_to_string(path).expect("readable")).expect("a session file")
}

#[test]
fn a_run_files_its_conversation_and_a_later_one_resumes_it() {
    // The twenty-turn run a blip threw away, and the follow-up question that used to cost the
    // whole conversation again. The second run replays the first one's items before its own input,
    // so the model keeps what it already worked out.
    let fixture = Fixture::start("text");
    let workspace = workspace();
    let sessions = tempfile::tempdir().expect("a temporary directory");

    let first = run_with_session(&fixture, &[], workspace.path(), sessions.path());
    assert_eq!(first.status, Some(0), "stderr: {}", first.stderr);
    assert!(
        first.stderr.contains("session ") && first.stderr.contains("saved to"),
        "the identifier is printed, or nobody can resume it: {}",
        first.stderr
    );
    let session = only_session(sessions.path());
    assert_eq!(session["turns"], 1);
    assert_eq!(session["version"], 1);
    assert_eq!(session["wire"], "openai-responses");
    let after_one = session["items"].as_array().expect("items").len();
    assert!(after_one >= 2, "the question and the answer: {session}");

    let second = run_with_session(
        &fixture,
        &["--resume", "latest"],
        workspace.path(),
        sessions.path(),
    );
    assert_eq!(second.status, Some(0), "stderr: {}", second.stderr);
    let session = only_session(sessions.path());
    assert_eq!(session["turns"], 2, "folded into the same session");
    assert!(
        session["items"].as_array().expect("items").len() > after_one,
        "the second run's turn joined the first one's: {session}"
    );
    assert_eq!(
        session["items"][0]["text"], "read the readme and tell me what it says",
        "and the first question is still the first item: {session}"
    );

    let listed = raw(&[
        "sessions",
        "--session-dir",
        sessions.path().to_str().expect("utf-8 path"),
    ]);
    assert_eq!(listed.status, Some(0), "stderr: {}", listed.stderr);
    assert!(
        listed
            .stdout
            .contains(session["id"].as_str().expect("an identifier")),
        "the listing names it: {}",
        listed.stdout
    );
    assert!(listed.stdout.contains("b10x-emulated"), "{}", listed.stdout);
}

#[test]
fn a_run_that_never_got_an_answer_still_files_what_it_had() {
    // `LoopError` carries no items, so a shell reading only the outcome had nothing to save. The
    // conversation the loop hands back is saved exactly as a finished one is.
    let fixture = Fixture::start("unauthorized");
    let workspace = workspace();
    let sessions = tempfile::tempdir().expect("a temporary directory");

    let output = run_with_session(&fixture, &[], workspace.path(), sessions.path());
    assert_eq!(output.status, Some(1), "stdout: {}", output.stdout);
    let session = only_session(sessions.path());
    assert_eq!(
        session["items"][0]["text"], "read the readme and tell me what it says",
        "what the run had when it failed: {session}"
    );
}

/// A synthetic rate card naming the emulated model, in a directory of its own.
///
/// Written by the test rather than found anywhere (`AGENTS.md` invariant 17). Its own directory
/// because a `.json` file beside the sessions is a file `only_session` would try to parse as one.
/// Without a card the loop prices nothing, and the cost half of what a failed run hands back would
/// be `None` in every assertion below for a reason that has nothing to do with the failure.
fn rate_card(dir: &Path) -> PathBuf {
    let path = dir.join("rates.json");
    fs::write(
        &path,
        r#"{"source": "a synthetic card this test wrote", "as_of": "2026-08-29",
            "models": {"b10x-emulated": {"input_usd_per_mtok": 1.0,
                                         "cached_input_usd_per_mtok": 0.1,
                                         "output_usd_per_mtok": 2.0}}}"#,
    )
    .expect("write");
    path
}

#[test]
fn a_run_that_broke_on_the_wire_files_the_turn_it_had_already_bought() {
    // The `Err` arm's ledger fold, end to end. `fails-after-turn` is the only scenario that answers
    // a whole turn — usage, a cost, one tool call the loop comes back from — and *then* breaks on
    // the wire, so it is the only one where the session file can be wrong about what a failed run
    // spent. Every figure asserted here scrolled past on stderr while the run was alive; after it,
    // this file is the only place left holding them, and one showing a turn and no tokens would
    // say the failure was free.
    let fixture = Fixture::start("fails-after-turn");
    let workspace = workspace();
    let sessions = tempfile::tempdir().expect("a temporary directory");
    let cards = tempfile::tempdir().expect("a temporary directory");
    let card = rate_card(cards.path());

    let output = run_with_session(
        &fixture,
        &["--prices", card.to_str().expect("utf-8 path")],
        workspace.path(),
        sessions.path(),
    );

    // The status `README.md` documents for a run the harness could not finish — not the `2` of a
    // run that stopped for a named reason.
    assert_eq!(output.status, Some(1), "stdout: {}", output.stdout);
    assert!(
        output.stderr.contains("400"),
        "the failure names itself: {}",
        output.stderr
    );
    // Turn one really happened first: the tool was called and answered before the wire refused.
    assert!(output.stderr.contains("→ file_read"), "{}", output.stderr);
    assert!(output.stderr.contains("← ok"), "{}", output.stderr);

    let session = only_session(sessions.path());
    assert_eq!(
        session["turns"], 2,
        "the turn that answered and the one that broke: {session}"
    );
    assert_eq!(
        session["usage"].as_array().expect("usage").len(),
        1,
        "one entry, for the one turn the provider reported for: {session}"
    );
    assert_eq!(session["usage"][0]["input_tokens"], 42, "{session}");
    assert_eq!(session["usage"][0]["output_tokens"], 8, "{session}");
    // 35 fresh input at $1/Mtok, 7 cached at $0.10, 8 output at $2, rounded once for the turn.
    assert_eq!(
        session["cost_micro_usd"], 52,
        "a run that failed is not a run that was free: {session}"
    );
    // And the conversation it had when it broke is filed beside the figures, as it always was.
    let kinds: Vec<&str> = session["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter_map(|item| item["kind"].as_str())
        .collect();
    assert_eq!(
        kinds,
        vec!["user-text", "tool-call", "tool-result"],
        "{session}"
    );
}

#[test]
fn a_run_that_broke_on_the_wire_files_what_it_bought_on_the_second_wire_too() {
    // The same scenario and the same assertions, one flag different. The fold is the shell's and
    // cannot see which wire a run was on; the figures differ only because this route reports its
    // cache read disjointly and the client sums it, so 42 + 7 arrives as 49.
    let fixture = Fixture::messages("fails-after-turn");
    let workspace = workspace();
    let sessions = tempfile::tempdir().expect("a temporary directory");
    let cards = tempfile::tempdir().expect("a temporary directory");
    let card = rate_card(cards.path());

    let output = run_with_session(
        &fixture,
        &[
            "--wire",
            "anthropic-messages",
            "--prices",
            card.to_str().expect("utf-8 path"),
        ],
        workspace.path(),
        sessions.path(),
    );

    assert_eq!(output.status, Some(1), "stdout: {}", output.stdout);
    assert!(output.stderr.contains("← ok"), "{}", output.stderr);

    let session = only_session(sessions.path());
    assert_eq!(session["wire"], "anthropic-messages");
    assert_eq!(session["turns"], 2, "{session}");
    assert_eq!(session["usage"].as_array().expect("usage").len(), 1);
    assert_eq!(session["usage"][0]["input_tokens"], 49, "{session}");
    assert_eq!(session["usage"][0]["cached_input_tokens"], 7, "{session}");
    assert_eq!(session["cost_micro_usd"], 59, "{session}");
}

#[test]
fn the_record_of_a_run_that_broke_after_a_turn_carries_that_turn_and_stops() {
    // What a driver above this process reads. The turn that was bought is in the record with its
    // usage and its cost, the turn that broke is announced and never finishes, and there is no
    // `finished` event at all — a run that ended on the wire must not be readable as one that
    // completed. The failure itself is on stderr beside the exit status, because `refused` is
    // reserved for a run that never started and this one did.
    let fixture = Fixture::start("fails-after-turn");
    let workspace = workspace();
    let cards = tempfile::tempdir().expect("a temporary directory");
    let card = rate_card(cards.path());

    let output = run_against(
        &fixture,
        &["--json", "--prices", card.to_str().expect("utf-8 path")],
        workspace.path(),
    );

    assert_eq!(output.status, Some(1), "stdout: {}", output.stdout);
    let events = events(&output);
    let kinds = kinds(&events);
    assert_eq!(kinds.first(), Some(&"started"));
    assert!(kinds.contains(&"tool-requested"), "{kinds:?}");
    assert!(kinds.contains(&"tool-completed"), "{kinds:?}");
    assert!(kinds.contains(&"usage"), "{kinds:?}");
    assert!(kinds.contains(&"cost"), "{kinds:?}");
    assert_eq!(
        kinds.iter().filter(|kind| **kind == "turn-started").count(),
        2,
        "the turn that answered and the one that broke: {kinds:?}"
    );
    assert_eq!(
        kinds.last(),
        Some(&"turn-started"),
        "the record stops where the wire did: {kinds:?}"
    );
    assert_eq!(
        events.last().expect("a last event")["turn"],
        serde_json::json!(2)
    );
    assert!(
        !kinds.contains(&"finished"),
        "a run that broke must not read as one that ended: {kinds:?}"
    );
    assert!(
        !kinds.contains(&"refused"),
        "`refused` is for a run that never started, and this one did: {kinds:?}"
    );
    assert!(
        output.stderr.contains("400"),
        "the failure is named on stderr: {}",
        output.stderr
    );
}

#[test]
fn a_chat_line_that_broke_on_the_wire_files_what_that_line_had_bought() {
    // `chat` folds the ledger in its own `Err` arm, and it has to: the session it writes into is
    // the whole conversation's, so a total that stopped counting at the line that broke would be
    // wrong about every line before it and not only about this one. One line down the pipe is
    // enough to show it, because the line that breaks ends the chat.
    let fixture = Fixture::start("fails-after-turn");
    let workspace = workspace();
    let sessions = tempfile::tempdir().expect("a temporary directory");
    let cards = tempfile::tempdir().expect("a temporary directory");
    let card = rate_card(cards.path());

    let mut child = Command::new(BINARY)
        .args([
            "chat",
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
            "--prices",
            card.to_str().expect("utf-8 path"),
        ])
        .env("B10X_HARNESS_TEST_KEY", "test-key")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    child
        .stdin
        .as_mut()
        .expect("piped stdin")
        .write_all(b"read the readme and tell me what it says\n")
        .expect("write");
    let output = child.wait_with_output().expect("the chat ends");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(1), "stderr: {stderr}");
    assert!(stderr.contains("400"), "{stderr}");

    let session = only_session(sessions.path());
    assert_eq!(session["turns"], 2, "{session}");
    assert_eq!(session["usage"].as_array().expect("usage").len(), 1);
    assert_eq!(session["cost_micro_usd"], 52, "{session}");
}

#[test]
fn a_failed_run_nobody_could_price_leaves_the_session_unpriced_rather_than_zero() {
    // The other half of the same fold (`AGENTS.md` invariant 7). `unauthorized` breaks on the first
    // request, so the run started a turn the provider never reported usage for. A rate card is in
    // force, so a zero here would be a figure the harness computed rather than one it never had —
    // and `b10x-harness sessions` would report the run as having cost nothing.
    let fixture = Fixture::start("unauthorized");
    let workspace = workspace();
    let sessions = tempfile::tempdir().expect("a temporary directory");
    let cards = tempfile::tempdir().expect("a temporary directory");
    let card = rate_card(cards.path());

    let output = run_with_session(
        &fixture,
        &["--prices", card.to_str().expect("utf-8 path")],
        workspace.path(),
        sessions.path(),
    );

    assert_eq!(output.status, Some(1), "stdout: {}", output.stdout);
    let session = only_session(sessions.path());
    assert_eq!(
        session["turns"], 1,
        "the turn was started and paid for by the attempt: {session}"
    );
    assert!(
        session["usage"].as_array().expect("usage").is_empty(),
        "nothing was reported, and an empty list says so: {session}"
    );
    assert!(
        session["cost_micro_usd"].is_null(),
        "absent, never zero: {session}"
    );
}

#[test]
fn a_session_from_the_other_wire_is_refused_before_anything_is_sent() {
    // An opaque provider item may not cross wires. The loop would refuse it; saying so here says
    // it in this harness's own words, naming the flag that fixes it, before a turn is paid for.
    let fixture = Fixture::start("text");
    let workspace = workspace();
    let sessions = tempfile::tempdir().expect("a temporary directory");
    run_with_session(&fixture, &[], workspace.path(), sessions.path());

    let output = run_with_session(
        &fixture,
        &[
            "--resume",
            "latest",
            "--wire",
            "anthropic-messages",
            "--json",
        ],
        workspace.path(),
        sessions.path(),
    );
    assert_eq!(output.status, Some(1), "stdout: {}", output.stdout);
    assert!(
        output.stderr.contains("openai-responses"),
        "{}",
        output.stderr
    );
    assert!(
        output.stderr.contains("anthropic-messages"),
        "{}",
        output.stderr
    );
    // And a driver reading the record is told the run never started, rather than being left with
    // an exit status and an empty stream.
    let refused: Value = serde_json::from_str(output.stdout.trim()).expect("one JSON line");
    assert_eq!(refused["kind"], "refused");
    assert!(
        refused["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("anthropic-messages")),
        "{refused}"
    );
}

#[test]
fn a_command_line_the_parser_refuses_states_that_the_run_never_started() {
    // Two hours went into this once: a driver launched the binary with a flag that had changed
    // shape, clap exited before any harness code ran, and the driver saw a status it already had
    // a meaning for and no record at all.
    let output = raw(&["run", "--json", "--not-a-flag-this-build-has"]);

    assert_eq!(
        output.status,
        Some(1),
        "1 and not clap's 2: on this command line 2 means a run that happened and stopped"
    );
    let refused: Value = serde_json::from_str(output.stdout.trim()).expect("one JSON line");
    assert_eq!(refused["kind"], "refused");
    assert!(
        refused["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("--not-a-flag-this-build-has")),
        "clap's own words, on one line: {refused}"
    );
    assert!(
        refused["reason"]
            .as_str()
            .is_some_and(|reason| !reason.contains('\n')),
        "one line, so a line-delimited record stays line-delimited: {refused}"
    );
    assert!(
        output.stderr.contains("--not-a-flag-this-build-has"),
        "and a person still gets clap's message: {}",
        output.stderr
    );
}

#[test]
fn an_unenforceable_spend_ceiling_is_one_json_refusal_and_no_session() {
    // `Budget::validate` runs before the loop's `Started` event and before any request. The CLI
    // used to classify that error as a run that had started and failed: stdout was empty under
    // `--no-session`, or held only a `session` line when filing was enabled. Both shapes left a
    // JSONL driver with no terminal saying the run was refused.
    let workspace = workspace();
    let sessions = tempfile::tempdir().expect("a temporary directory");
    let output = run(
        &[
            "run",
            "--base-url",
            "http://127.0.0.1:1/v1",
            "--model",
            "b10x-emulated",
            "--max-cost-microunits",
            "1",
            "--session-dir",
            sessions.path().to_str().expect("utf-8 path"),
            "--json",
            "--input",
            "hi",
        ],
        workspace.path(),
    );

    assert_eq!(output.status, Some(1), "stderr: {}", output.stderr);
    assert_eq!(
        output.stdout.lines().count(),
        1,
        "one terminal and no session line: {}",
        output.stdout
    );
    let refused: Value = serde_json::from_str(output.stdout.trim()).expect("one JSON refusal line");
    assert_eq!(refused["kind"], "refused", "{refused}");
    assert!(
        refused["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("max_cost_microunits")),
        "{refused}"
    );
    assert!(
        output.stderr.contains("budget refused")
            && !output.stderr.contains("posting to http://127.0.0.1:1"),
        "the budget, not the unreachable endpoint, decides before any request: {}",
        output.stderr
    );
    assert_eq!(
        fs::read_dir(sessions.path())
            .expect("the session directory remains readable")
            .count(),
        0,
        "a run that never started files no session"
    );
}

#[test]
fn a_write_is_refused_when_the_run_may_not_ask_anybody() {
    // `--approve deny` is the library's own default approver, chosen explicitly. The call comes
    // back to the model as a failed outcome — it has to know the effect did not happen — and the
    // run finishes rather than ending.
    let fixture = Fixture::start("flat-write");
    let (_root, workspace) = adoptable_workspace("ws_deny");
    let output = run_against(
        &fixture,
        &["--substrate-embedded", "--approve", "deny"],
        &workspace,
    );

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert!(output.stderr.contains("→ file_write"), "{}", output.stderr);
    assert!(
        output.stderr.contains("← failed"),
        "the model is told the write did not happen: {}",
        output.stderr
    );
    assert!(
        !workspace.join("note.md").exists(),
        "and nothing was written"
    );
}

#[test]
fn asking_for_a_person_when_there_is_no_terminal_refuses_the_run_by_name() {
    // `--approve prompt` names a person. Falling back to refusing every call would look like a
    // harness whose tools do not work, so the run refuses instead and says which flag to use.
    let fixture = Fixture::start("flat-write");
    let (_root, workspace) = adoptable_workspace("ws_prompt");
    let output = run_against(
        &fixture,
        &["--substrate-embedded", "--approve", "prompt"],
        &workspace,
    );

    assert_eq!(output.status, Some(1), "stdout: {}", output.stdout);
    assert!(output.stderr.contains("/dev/tty"), "{}", output.stderr);
    assert!(
        output.stderr.contains("--approve deny"),
        "{}",
        output.stderr
    );
}

#[test]
fn raising_the_ceiling_lets_a_write_through_on_the_second_wire_too() {
    // The ceiling is the loop's, and the loop cannot tell which wire it got. One flag different
    // from the run above, and the write lands.
    let fixture = Fixture::messages("flat-write");
    let (_root, workspace) = adoptable_workspace("ws_ceiling");
    let output = run_against(
        &fixture,
        &[
            "--wire",
            "anthropic-messages",
            "--substrate-embedded",
            "--approve-up-to",
            "medium",
        ],
        &workspace,
    );

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert!(output.stderr.contains("← ok"), "{}", output.stderr);
    assert_eq!(
        fs::read_to_string(workspace.join("note.md")).expect("the file was written"),
        "written by the harness\n"
    );
}

#[test]
fn chat_carries_one_turn_into_the_next_over_one_session() {
    // Two questions down a pipe, one conversation. The session is what proves it: the second
    // turn's items sit on top of the first turn's rather than beside them.
    let fixture = Fixture::start("text");
    let workspace = workspace();
    let sessions = tempfile::tempdir().expect("a temporary directory");
    let mut child = Command::new(BINARY)
        .args([
            "chat",
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
        ])
        .env("B10X_HARNESS_TEST_KEY", "test-key")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    child
        .stdin
        .as_mut()
        .expect("piped stdin")
        .write_all(b"what does this workspace do?\nand what else?\nexit\n")
        .expect("write");
    let output = child.wait_with_output().expect("the chat ends");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.matches("provider emulation passed").count(),
        2,
        "one answer per line of input: {stdout}"
    );
    let session = only_session(sessions.path());
    assert_eq!(session["turns"], 2, "{session}");
    let questions: Vec<&str> = session["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter(|item| item["kind"] == "user-text")
        .filter_map(|item| item["text"].as_str())
        .collect();
    assert_eq!(
        questions,
        vec!["what does this workspace do?", "and what else?"],
        "the second turn is a follow-up on the first, not a run of its own: {session}"
    );
}

// ---------------------------------------------------------------------------------------------
// Structured output, delegation and hooks (design 0002).
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

/// A hook that is a real program: a python3 script, spawned as an argv like any other.
fn hook_script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).expect("write the hook");
    path
}

/// The schema `--output-schema` names, as a file.
fn schema_file(dir: &Path, schema: &Value) -> PathBuf {
    let path = dir.join("schema.json");
    fs::write(&path, schema.to_string()).expect("write the schema");
    path
}

/// Every event of a `--json` run, one per line.
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

#[test]
fn a_stop_hook_blocks_one_ending_and_the_run_turns_again_before_it_finishes() {
    // The operator's last word on a run that would end here. The first stop is refused with a
    // reason, that reason becomes one more user item, the model answers again and the second stop
    // is allowed — so the run completes, having taken a turn nobody's flags asked for.
    let fixture = Fixture::start("stop-hook");
    let workspace = workspace();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let marker = dir.path().join("ran-once");
    let script = hook_script(
        dir.path(),
        "stop.py",
        &format!(
            r#"import json, os, sys
document = json.load(sys.stdin)
if document.get("hook") != "stop" or "text" not in document or "workspace" not in document:
    sys.stderr.write("not a stop document: " + json.dumps(document))
    sys.exit(3)
if os.path.exists({marker:?}):
    sys.exit(0)
open({marker:?}, "w").close()
sys.stdout.write(json.dumps({{"reason": "the tests were not run; run them and say what happened"}}))
sys.exit(2)
"#,
            marker = marker.display().to_string(),
        ),
    );
    let hooks = hooks_file(
        dir.path(),
        &serde_json::json!({
            "on": "stop",
            "command": ["python3", script.display().to_string()],
        }),
    );

    let output = run_against(
        &fixture,
        &["--json", "--hooks", hooks.to_str().expect("utf-8 path")],
        workspace.path(),
    );

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    let events = events(&output);
    let decisions: Vec<&Value> = events
        .iter()
        .filter(|event| event["kind"] == "hook-ran")
        .collect();
    assert_eq!(
        decisions.len(),
        2,
        "one refusal and one assent: {:?}",
        kinds(&events)
    );
    assert_eq!(decisions[0]["point"], serde_json::json!("stop"));
    assert_eq!(decisions[0]["decision"]["kind"], serde_json::json!("block"));
    assert!(
        decisions[0]["decision"]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("the tests were not run")),
        "the hook's own words reach the record: {:?}",
        decisions[0]
    );
    assert_eq!(
        decisions[1]["decision"]["kind"],
        serde_json::json!("proceed")
    );
    let text: String = events
        .iter()
        .filter(|event| event["kind"] == "text-delta")
        .filter_map(|event| event["text"].as_str())
        .collect();
    assert!(
        text.contains("second answer, after the hook"),
        "the model answered again, having read the reason: {text}"
    );
    assert_eq!(
        events.last().expect("a terminal event")["stop"]["kind"],
        serde_json::json!("completed")
    );
}

#[test]
fn a_hooks_file_this_build_cannot_read_refuses_the_run_before_anything_is_sent() {
    // A run started with `--hooks` and no hooks is a run whose gate the operator thinks is there.
    // So it is a refusal, of exactly the kind a bad credential is, and it says so in the record.
    let fixture = Fixture::start("text");
    let workspace = workspace();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("hooks.json");
    fs::write(
        &path,
        r#"{"version": 1, "hooks": [{"on": "whenever", "command": ["x"]}]}"#,
    )
    .expect("write");

    let output = run_against(
        &fixture,
        &["--json", "--hooks", path.to_str().expect("utf-8 path")],
        workspace.path(),
    );

    assert_eq!(output.status, Some(1), "stdout: {}", output.stdout);
    let refused: Value =
        serde_json::from_str(output.stdout.trim()).expect("one line saying the run never started");
    assert_eq!(refused["kind"], serde_json::json!("refused"));
    assert!(
        refused["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("not a hook point")),
        "{refused}"
    );
    assert!(output.stderr.contains("hooks.json"), "{}", output.stderr);
}

#[test]
fn an_output_schema_that_is_not_an_object_schema_refuses_the_run_before_anything_is_sent() {
    let fixture = Fixture::start("text");
    let workspace = workspace();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let schema = schema_file(dir.path(), &serde_json::json!({"type": "string"}));

    let output = run_against(
        &fixture,
        &[
            "--json",
            "--output-schema",
            schema.to_str().expect("utf-8 path"),
        ],
        workspace.path(),
    );

    assert_eq!(output.status, Some(1), "stdout: {}", output.stdout);
    let refused: Value =
        serde_json::from_str(output.stdout.trim()).expect("one line saying the run never started");
    assert_eq!(refused["kind"], serde_json::json!("refused"));
    assert!(
        refused["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("JSON Schema for an object")),
        "{refused}"
    );
}

#[test]
fn a_run_asked_for_a_schema_that_answers_in_prose_stops_without_an_answer() {
    // The failure that must not be silent: a consumer that piped stdout to a JSON reader and got
    // prose with exit 0 would read the prose as the answer. One nudge is spent, and then the run
    // stops `unstructured` and exits 2 — the same status every other stop-without-an-answer has.
    let fixture = Fixture::start("answer-prose");
    let workspace = workspace();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let schema = schema_file(
        dir.path(),
        &serde_json::json!({
            "type": "object",
            "properties": {"verdict": {"type": "string"}},
            "required": ["verdict"],
        }),
    );

    let output = run_against(
        &fixture,
        &["--output-schema", schema.to_str().expect("utf-8 path")],
        workspace.path(),
    );

    assert_eq!(output.status, Some(2), "stderr: {}", output.stderr);
    assert_eq!(
        output.stdout, "",
        "nothing at all on stdout: a run with no structured answer has nothing to compose with"
    );
    assert!(output.stderr.contains("Unstructured"), "{}", output.stderr);
    // The prose is still shown — as progress, on stderr, where it cannot be read as the answer.
    assert!(
        output.stderr.contains("The readme says hello harness."),
        "{}",
        output.stderr
    );

    let recorded = run_against(
        &fixture,
        &[
            "--json",
            "--output-schema",
            schema.to_str().expect("utf-8 path"),
        ],
        workspace.path(),
    );
    let events = events(&recorded);
    assert_eq!(
        events.last().expect("a terminal event")["stop"],
        serde_json::json!({"kind": "unstructured", "asked_again": 1})
    );
}

#[test]
fn an_answer_call_is_the_only_thing_on_stdout() {
    let fixture = Fixture::start("answer-call");
    let workspace = workspace();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let schema = schema_file(
        dir.path(),
        &serde_json::json!({
            "type": "object",
            "properties": {
                "verdict": {"type": "string"},
                "file": {"type": "string"},
                "bytes": {"type": "integer"},
            },
            "required": ["verdict"],
        }),
    );

    let output = run_against(
        &fixture,
        &["--output-schema", schema.to_str().expect("utf-8 path")],
        workspace.path(),
    );

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert_eq!(
        output.stdout.lines().count(),
        1,
        "one line, so the command composes: {:?}",
        output.stdout
    );
    let answer: Value =
        serde_json::from_str(output.stdout.trim()).expect("stdout is the answer, as JSON");
    assert_eq!(
        answer,
        serde_json::json!({"verdict": "ok", "file": "README.md", "bytes": 14})
    );
}

#[test]
fn an_answer_call_is_the_only_thing_on_stdout_on_the_second_wire_too() {
    // Structured output is not a feature of one wire: `answer` is a tool the loop owns, published
    // beside the port's own specs, and both emulators serve the same scenario. One flag different.
    let fixture = Fixture::messages("answer-call");
    let workspace = workspace();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let schema = schema_file(
        dir.path(),
        &serde_json::json!({
            "type": "object",
            "properties": {
                "verdict": {"type": "string"},
                "file": {"type": "string"},
                "bytes": {"type": "integer"},
            },
            "required": ["verdict"],
        }),
    );

    let output = run_against(
        &fixture,
        &[
            "--wire",
            "anthropic-messages",
            "--output-schema",
            schema.to_str().expect("utf-8 path"),
        ],
        workspace.path(),
    );

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert_eq!(
        output.stdout.lines().count(),
        1,
        "one line, so the command composes: {:?}",
        output.stdout
    );
    let answer: Value =
        serde_json::from_str(output.stdout.trim()).expect("stdout is the answer, as JSON");
    assert_eq!(
        answer,
        serde_json::json!({"verdict": "ok", "file": "README.md", "bytes": 14})
    );
}

#[test]
fn a_withdrawn_answer_is_never_printed_and_the_second_one_is_printed_once() {
    // The whole reason the renderer holds the value instead of writing it as it arrives. The stop
    // hook refuses the first ending, the loop clears the answer and turns again, and the model
    // answers differently. Printing on arrival put the withdrawn value on stdout and then printed
    // the second beside it — two JSON lines, of which the first was the one the operator refused.
    let fixture = Fixture::start("answer-stop-hook");
    let workspace = workspace();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let schema = schema_file(
        dir.path(),
        &serde_json::json!({
            "type": "object",
            "properties": {"verdict": {"type": "string"}},
            "required": ["verdict"],
        }),
    );
    let marker = dir.path().join("ran-once");
    let script = hook_script(
        dir.path(),
        "stop.py",
        &format!(
            r#"import json, os, sys
json.load(sys.stdin)
if os.path.exists({marker:?}):
    sys.exit(0)
open({marker:?}, "w").close()
sys.stdout.write(json.dumps({{"reason": "that verdict is not the one the tests support"}}))
sys.exit(2)
"#,
            marker = marker.display().to_string(),
        ),
    );
    let hooks = hooks_file(
        dir.path(),
        &serde_json::json!({
            "on": "stop",
            "command": ["python3", script.display().to_string()],
        }),
    );

    let output = run_against(
        &fixture,
        &[
            "--output-schema",
            schema.to_str().expect("utf-8 path"),
            "--hooks",
            hooks.to_str().expect("utf-8 path"),
        ],
        workspace.path(),
    );

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert_eq!(
        output.stdout.lines().count(),
        1,
        "exactly one answer, not the refused one and its replacement: {:?}",
        output.stdout
    );
    let answer: Value =
        serde_json::from_str(output.stdout.trim()).expect("stdout is the answer, as JSON");
    assert_eq!(
        answer,
        serde_json::json!({"verdict": "second, after the hook"}),
        "the value that survived is the one on stdout"
    );
}

#[test]
fn under_json_the_answer_is_the_last_answered_event_and_no_line_of_stdout_is_a_bare_answer() {
    // What the README and `--output-schema` used to promise — *stdout is that JSON and nothing
    // else* — is false the moment `--json` is also given, which is the metaharness's own shape.
    // Stdout is then the record, every line an event, and a driver looking for a bare JSON line
    // finds none. What it takes instead is the last `answered` before a `completed` `finished`.
    let fixture = Fixture::start("answer-call");
    let workspace = workspace();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let schema = schema_file(
        dir.path(),
        &serde_json::json!({
            "type": "object",
            "properties": {
                "verdict": {"type": "string"},
                "file": {"type": "string"},
                "bytes": {"type": "integer"},
            },
            "required": ["verdict"],
        }),
    );

    let output = run_against(
        &fixture,
        &[
            "--json",
            "--output-schema",
            schema.to_str().expect("utf-8 path"),
        ],
        workspace.path(),
    );

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    // `events` panics on a line that is not JSON at all; this says the stronger thing — every
    // line is an *event*, so nothing on the stream is the answer written beside the record.
    let events = events(&output);
    for event in &events {
        assert!(
            event["kind"].as_str().is_some(),
            "every line of the record is an event, not a bare answer: {event}"
        );
    }
    let expected = serde_json::json!({"verdict": "ok", "file": "README.md", "bytes": 14});
    assert!(
        !events.contains(&expected),
        "the answer is not also printed on its own line: {:?}",
        output.stdout
    );

    let answered: Vec<&Value> = events
        .iter()
        .filter(|event| event["kind"] == serde_json::json!("answered"))
        .collect();
    assert_eq!(
        answered.last().expect("the run answered")["value"],
        expected,
        "the last `answered` is the run's answer"
    );
    let finished = events.last().expect("a terminal event");
    assert_eq!(finished["kind"], serde_json::json!("finished"));
    assert_eq!(
        finished["stop"],
        serde_json::json!({"kind": "completed"}),
        "and it is the answer only because the run completed"
    );
}

#[test]
fn a_withdrawn_answer_stays_in_the_json_record_so_only_the_last_one_may_be_read() {
    // Why the rule is *last* and not *first*. The stop hook refuses the first ending, the loop
    // clears the structured answer and turns again, and both `answered` events are in the record
    // — the withdrawn one first. A driver taking the first takes the value the operator refused.
    let fixture = Fixture::start("answer-stop-hook");
    let workspace = workspace();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let schema = schema_file(
        dir.path(),
        &serde_json::json!({
            "type": "object",
            "properties": {"verdict": {"type": "string"}},
            "required": ["verdict"],
        }),
    );
    let marker = dir.path().join("ran-once");
    let script = hook_script(
        dir.path(),
        "stop.py",
        &format!(
            r#"import json, os, sys
json.load(sys.stdin)
if os.path.exists({marker:?}):
    sys.exit(0)
open({marker:?}, "w").close()
sys.stdout.write(json.dumps({{"reason": "that verdict is not the one the tests support"}}))
sys.exit(2)
"#,
            marker = marker.display().to_string(),
        ),
    );
    let hooks = hooks_file(
        dir.path(),
        &serde_json::json!({
            "on": "stop",
            "command": ["python3", script.display().to_string()],
        }),
    );

    let output = run_against(
        &fixture,
        &[
            "--json",
            "--output-schema",
            schema.to_str().expect("utf-8 path"),
            "--hooks",
            hooks.to_str().expect("utf-8 path"),
        ],
        workspace.path(),
    );

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    let events = events(&output);
    let answers: Vec<&Value> = events
        .iter()
        .filter(|event| event["kind"] == serde_json::json!("answered"))
        .map(|event| &event["value"])
        .collect();
    assert_eq!(
        answers,
        vec![
            &serde_json::json!({"verdict": "first"}),
            &serde_json::json!({"verdict": "second, after the hook"}),
        ],
        "the withdrawn answer is in the record too, before the one that survived: {:?}",
        output.stdout
    );
    let finished = events.last().expect("a terminal event");
    assert_eq!(finished["stop"], serde_json::json!({"kind": "completed"}));
}

#[test]
fn an_after_call_note_reaches_the_model_beside_the_result_it_is_about() {
    // A hook that cannot block still has to be able to tell the model something — that the
    // formatter ran, that the tree is dirty. The note travels as `hook_notes` on the tool result
    // itself, which is what the next turn reads, and the session records exactly that.
    let fixture = Fixture::start("flat-tool");
    let workspace = workspace();
    let sessions = tempfile::tempdir().expect("a temporary directory");
    let dir = tempfile::tempdir().expect("a temporary directory");
    let script = hook_script(
        dir.path(),
        "note.py",
        r#"import json, sys
document = json.load(sys.stdin)
if document.get("hook") != "after-call" or "outcome" not in document:
    sys.stderr.write("not an after-call document: " + json.dumps(document))
    sys.exit(3)
sys.stdout.write(json.dumps({"note": "read at revision deadbeef"}))
sys.exit(0)
"#,
    );
    let hooks = hooks_file(
        dir.path(),
        &serde_json::json!({
            "on": "after-call",
            "tools": ["file_read"],
            "command": ["python3", script.display().to_string()],
        }),
    );

    let output = run_with_session(
        &fixture,
        &["--hooks", hooks.to_str().expect("utf-8 path")],
        workspace.path(),
        sessions.path(),
    );
    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);

    let session = only_session(sessions.path());
    let result = session["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|item| item["kind"] == "tool-result")
        .expect("the conversation carries the result the model read");
    assert_eq!(
        result["output"]["hook_notes"],
        serde_json::json!(["read at revision deadbeef"]),
        "beside the result, not instead of it: {result}"
    );
    assert_eq!(
        result["failed"],
        serde_json::json!(false),
        "an `after-call` hook narrows nothing: {result}"
    );
}

#[test]
fn a_hook_never_carries_the_variable_this_run_reads_its_credential_from() {
    // A hook is unconfined — the operator's own program, in this run's environment — and that is
    // exactly why it must not be handed the key. The child inherited the whole environment, so a
    // hook that echoed `$B10X_HARNESS_TEST_KEY` put the credential into the note the model reads
    // and into the session on disk. The name `--api-key-env` gave is removed before the spawn.
    let fixture = Fixture::start("flat-tool");
    let workspace = workspace();
    let sessions = tempfile::tempdir().expect("a temporary directory");
    let dir = tempfile::tempdir().expect("a temporary directory");
    let script = hook_script(
        dir.path(),
        "peek.py",
        r#"import json, os, sys
json.load(sys.stdin)
sys.stdout.write(json.dumps({"note": "saw [" + os.environ.get("B10X_HARNESS_TEST_KEY", "") + "]"}))
"#,
    );
    let hooks = hooks_file(
        dir.path(),
        &serde_json::json!({
            "on": "after-call",
            "command": ["python3", script.display().to_string()],
        }),
    );

    let output = run_with_session(
        &fixture,
        &["--hooks", hooks.to_str().expect("utf-8 path")],
        workspace.path(),
        sessions.path(),
    );
    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);

    let session = only_session(sessions.path());
    let result = session["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|item| item["kind"] == "tool-result")
        .expect("the conversation carries the result the model read");
    assert_eq!(
        result["output"]["hook_notes"],
        serde_json::json!(["saw []"]),
        "the hook ran, and found nothing under that name: {result}"
    );
    assert!(
        !session.to_string().contains("test-key"),
        "and the credential is nowhere in what was written to disk"
    );
}

#[test]
fn a_delegate_reads_a_file_in_its_own_context_and_the_parent_reads_only_the_report() {
    let fixture = Fixture::start("delegate");
    let workspace = workspace();

    let output = run_against(&fixture, &["--delegate", "--json"], workspace.path());

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    let events = events(&output);
    let kinds = kinds(&events);
    assert!(kinds.contains(&"delegate-started"), "{kinds:?}");
    assert!(kinds.contains(&"delegated"), "{kinds:?}");
    assert!(kinds.contains(&"delegate-finished"), "{kinds:?}");
    // The child's own tool call arrives wrapped, and nothing of it arrives bare.
    let wrapped: Vec<&Value> = events
        .iter()
        .filter(|event| event["kind"] == "delegated")
        .collect();
    assert!(
        wrapped
            .iter()
            .any(|event| event["event"]["kind"] == "tool-requested"),
        "the child's own calls are in the record, nested: {wrapped:?}"
    );
    let parent: String = events
        .iter()
        .filter(|event| event["kind"] == "text-delta")
        .filter_map(|event| event["text"].as_str())
        .collect();
    assert!(
        parent.contains("the delegate read it"),
        "the parent answers for itself: {parent}"
    );
}

/// Two `delegate-started` before either `delegate-finished`, over the shipped binary and the
/// shipped wire client.
///
/// # Why this is not the loop's own test again
///
/// `harness-loop` proves the *mechanism* against doubles it wrote itself. What only a run through
/// the binary can prove is that the **real** ports fork: `ResponsesClient::fork` and the tool
/// surface's, over a real socket, with the real argv that chose them. A `fork` that answered `None`
/// in the shipped client would be a run that quietly went back to one child at a time, with every
/// loop test still green.
///
/// # And why the assertion is on the order of two events rather than on a clock
///
/// A fast serial run and a slow parallel one look alike to a timer. Two `delegate-started` with no
/// `delegate-finished` between them cannot happen in a run that delegated in order: the loop emits
/// a child's start immediately before running it to completion. So the bracketing *is* the
/// evidence.
fn two_delegates_of_one_turn_run_side_by_side_on(
    fixture: &Fixture,
    wire: &[&str],
    // The two wires mint call ids in their own vocabularies, and the pairing being pinned here is
    // between a child and *a* call rather than between a child and a spelling.
    ids: [&str; 2],
) {
    let workspace = workspace();

    let mut arguments = vec!["--delegate", "--json"];
    arguments.extend_from_slice(wire);
    let output = run_against(fixture, &arguments, workspace.path());

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    let events = events(&output);
    let bracketing: Vec<&str> = events
        .iter()
        .filter_map(|event| match event["kind"].as_str() {
            Some(kind @ ("delegate-started" | "delegate-finished")) => Some(kind),
            _ => None,
        })
        .collect();
    assert_eq!(
        bracketing,
        vec![
            "delegate-started",
            "delegate-started",
            "delegate-finished",
            "delegate-finished"
        ],
        "both children start before either finishes, which a run that delegated in order cannot \
         produce: {bracketing:?}"
    );

    // Each child was handed its own task and answered from it. The group's whole risk is that a
    // child's work is attributed to the wrong `delegate` call — the children finish in no
    // particular order, and nothing downstream could tell — so what is pinned is the pairing of
    // call id to task on the way in and to the child's own text on the way out.
    let handed: Vec<(&str, &str)> = events
        .iter()
        .filter(|event| event["kind"] == "delegate-started")
        .filter_map(|event| Some((event["call_id"].as_str()?, event["task"].as_str()?)))
        .collect();
    assert_eq!(
        handed,
        vec![
            (ids[0], "DELEGATE-TASK the left half"),
            (ids[1], "DELEGATE-TASK the right half"),
        ],
        "each child got the task of the call it answers: {handed:?}"
    );
    for (call, half) in [(ids[0], "left"), (ids[1], "right")] {
        let said: String = events
            .iter()
            .filter(|event| event["kind"] == "delegated" && event["call_id"] == call)
            .filter(|event| event["event"]["kind"] == "text-delta")
            .filter_map(|event| event["event"]["text"].as_str())
            .collect();
        assert_eq!(
            said,
            format!("child reporting on {half}"),
            "what the child of `{call}` said arrives under that call and no other"
        );
    }
    let parent: String = events
        .iter()
        .filter(|event| event["kind"] == "text-delta")
        .filter_map(|event| event["text"].as_str())
        .collect();
    assert!(
        parent.contains("both delegates reported"),
        "the parent answers for itself: {parent}"
    );
}

#[test]
fn two_delegates_of_one_turn_run_side_by_side_on_the_responses_wire() {
    two_delegates_of_one_turn_run_side_by_side_on(
        &Fixture::start("delegate-pair"),
        &[],
        ["call_b10x_001", "call_b10x_002"],
    );
}

#[test]
fn two_delegates_of_one_turn_run_side_by_side_on_the_messages_wire() {
    two_delegates_of_one_turn_run_side_by_side_on(
        &Fixture::messages("delegate-pair"),
        &["--wire", "anthropic-messages"],
        ["toolu_b10x_001", "toolu_b10x_002"],
    );
}

/// `--delegate-parallel 1` is the behaviour every version before this had: one child at a time.
#[test]
fn a_run_told_to_delegate_one_at_a_time_brackets_each_child_before_starting_the_next() {
    let fixture = Fixture::start("delegate-pair");
    let workspace = workspace();

    let output = run_against(
        &fixture,
        &["--delegate", "--delegate-parallel", "1", "--json"],
        workspace.path(),
    );

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    let events = events(&output);
    let bracketing: Vec<&str> = events
        .iter()
        .filter_map(|event| match event["kind"].as_str() {
            Some(kind @ ("delegate-started" | "delegate-finished")) => Some(kind),
            _ => None,
        })
        .collect();
    assert_eq!(
        bracketing,
        vec![
            "delegate-started",
            "delegate-finished",
            "delegate-started",
            "delegate-finished"
        ],
        "each child is bracketed before the next begins: {bracketing:?}"
    );
}

#[test]
fn a_delegate_turn_ceiling_binds_the_child_and_the_parent_is_told_it_did_not_finish() {
    // `--delegate-turns` is the child's own ceiling and not the parent's remainder, so a child
    // that loops does not spend the run's remaining turns finding out. The same delegate that
    // completes in two turns above comes back **failed**, carrying the bound it hit — a parent
    // that read a half-answer as a whole one is the silent failure invariant 9 forbids.
    let fixture = Fixture::start("delegate");
    let workspace = workspace();

    let output = run_against(
        &fixture,
        &["--delegate", "--delegate-turns", "1", "--json"],
        workspace.path(),
    );

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    let events = events(&output);
    let finished = events
        .iter()
        .find(|event| event["kind"] == "delegate-finished")
        .expect("the delegate is bracketed in the record");
    assert_eq!(
        finished["stop"],
        serde_json::json!({"kind": "max-turns", "limit": 1}),
        "the child stopped at its own ceiling: {finished}"
    );
    assert_eq!(finished["turns"], serde_json::json!(1), "{finished}");
    let completed: Vec<&Value> = events
        .iter()
        .filter(|event| event["kind"] == "tool-completed")
        .collect();
    assert_eq!(
        completed.len(),
        1,
        "the parent made one call, and the child's arrive wrapped: {completed:?}"
    );
    assert_eq!(
        completed[0]["failed"],
        serde_json::json!(true),
        "the parent learns the sub-task did not finish: {completed:?}"
    );
}

#[test]
fn a_before_call_hook_blocks_a_write_the_ceiling_had_already_allowed() {
    // A hook narrows and never widens: the approver said yes at `--approve-up-to medium`, and this
    // is one more refusal after it. The model is told the effect did not happen (invariant 9).
    let fixture = Fixture::start("hooks-block");
    let (_root, workspace) = adoptable_workspace("ws_hooked");
    let dir = tempfile::tempdir().expect("a temporary directory");
    let script = hook_script(
        dir.path(),
        "guard.py",
        r#"import json, sys
document = json.load(sys.stdin)
if document.get("entry") != "file_write":
    sys.stderr.write("this hook was declared for file_write: " + json.dumps(document))
    sys.exit(3)
sys.stdout.write(json.dumps({"reason": "note.md is not a file this run may create"}))
sys.exit(2)
"#,
    );
    let hooks = hooks_file(
        dir.path(),
        &serde_json::json!({
            "on": "before-call",
            "tools": ["file_write"],
            "command": ["python3", script.display().to_string()],
        }),
    );

    let output = run_against(
        &fixture,
        &[
            "--json",
            "--substrate-embedded",
            "--approve-up-to",
            "medium",
            "--hooks",
            hooks.to_str().expect("utf-8 path"),
        ],
        &workspace,
    );

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert!(
        !workspace.join("note.md").exists(),
        "the write did not happen"
    );
    let events = events(&output);
    let blocked = events
        .iter()
        .find(|event| event["kind"] == "hook-ran")
        .expect("the hook is in the record");
    assert_eq!(blocked["point"], serde_json::json!("before-call"));
    assert_eq!(blocked["decision"]["kind"], serde_json::json!("block"));
    let result = events
        .iter()
        .find(|event| event["kind"] == "tool-completed")
        .expect("the call still produces an outcome the model reads");
    assert_eq!(
        result["failed"],
        serde_json::json!(true),
        "a refusal the model must learn about is an outcome, not a silence"
    );
}

/// A machine with no config directory is **refused**, not crashed.
///
/// `RunOptions::model` unwraps on the promise that `apply_profiles` filled the model or refused
/// the run. When neither `HOME` nor `XDG_CONFIG_HOME` is set there is no config file, and the
/// early return for that case walked straight past the refusal — so the promise became a panic
/// and exit **101**, a fourth status on a command line documenting three.
///
/// Spawned rather than unit-tested because the variables are process-global: a test that cleared
/// them in-process would clear them for every test sharing the binary.
#[test]
fn a_machine_with_no_config_directory_is_told_what_to_type_rather_than_panicking() {
    let output = Command::new(BINARY)
        .args(["run", "--base-url", "http://127.0.0.1:1", "--input", "hi"])
        .env_remove("HOME")
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .expect("the binary runs");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "a refusal exits 1; 101 is a panic escaping as an exit status. stderr: {stderr}"
    );
    assert!(
        !stderr.contains("panicked"),
        "the operator gets an instruction, not a backtrace invitation: {stderr}"
    );
    assert!(
        stderr.contains("--model"),
        "and the instruction names the flag that would fix it: {stderr}"
    );
    assert!(
        stderr.contains("XDG_CONFIG_HOME"),
        "pointing at a config file that cannot exist on this machine is worse than saying so: \
         {stderr}"
    );
}

/// `providers show codex` states the write before anything is spent.
///
/// **The condition on which a built-in provider is allowed to renew at all.** A defaulted
/// credential path is paid for by being readable without running anything (`provider.rs` § *The
/// credential is defaulted*); a defaulted credential path that will also be **rewritten** owes the
/// same reader more — which other field of that file gets read, and which server it is presented
/// to. If this ever stops printing, the renewal is no longer accountable and should go with it.
#[test]
fn providers_show_states_the_file_it_will_rewrite_and_the_server_it_will_talk_to() {
    let empty_config = tempfile::tempdir().expect("a temporary directory");
    let shown = Command::new(BINARY)
        .args(["providers", "show", "codex"])
        // An empty config directory, so this is the shipped table and not an operator's override.
        .env("XDG_CONFIG_HOME", empty_config.path())
        .output()
        .expect("the binary runs");
    assert!(shown.status.success(), "{shown:?}");
    let text = String::from_utf8_lossy(&shown.stdout);
    for expected in [
        "https://chatgpt.com/backend-api/codex",
        "openai-responses",
        "/tokens/access_token",
        "https://auth.openai.com/oauth/token",
        "app_EMoamEEZ73f0CkXaXp7hrann",
        "/tokens/refresh_token",
    ] {
        assert!(text.contains(expected), "`{expected}` is not in:\n{text}");
    }
}

/// And the provider that does not renew says so, rather than saying nothing.
///
/// The same silence rule the `Started` event's always-written lists exist for: a reader cannot tell
/// *this provider never writes your credential file* from *this build does not say* unless one of
/// them is stated.
#[test]
fn a_provider_that_never_rewrites_your_credential_file_says_so() {
    let empty_config = tempfile::tempdir().expect("a temporary directory");
    let shown = Command::new(BINARY)
        .args(["providers", "show", "claude"])
        .env("XDG_CONFIG_HOME", empty_config.path())
        .output()
        .expect("the binary runs");
    assert!(shown.status.success(), "{shown:?}");
    let text = String::from_utf8_lossy(&shown.stdout);
    assert!(text.contains("never writes it"), "{text}");
    assert!(
        !text.contains("auth.openai.com"),
        "a provider with no measured renewal must not borrow another's: {text}"
    );
}

/// `providers list` offers the `ChatGPT` route beside the API-key one.
#[test]
fn the_shipped_table_offers_both_openai_routes_under_different_names() {
    let empty_config = tempfile::tempdir().expect("a temporary directory");
    let listed = Command::new(BINARY)
        .args(["providers", "list"])
        .env("XDG_CONFIG_HOME", empty_config.path())
        .output()
        .expect("the binary runs");
    assert!(listed.status.success(), "{listed:?}");
    let text = String::from_utf8_lossy(&listed.stdout);
    for name in ["claude", "openai", "codex"] {
        assert!(text.contains(name), "`{name}` is not in:\n{text}");
    }
}
