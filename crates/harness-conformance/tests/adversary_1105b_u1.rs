//! Adversary cases for `story:harness-spec-runs-on-the-newest-ess` (wave 2026-11-05b, unit 1,
//! pass 1).
//!
//! The unit moved ESS from 0.77.0 to 0.86.0 and rewrote the six guards on `next` of
//! `CompleteTool`, `Deny` and `ResumeDenied` as `{all: [defined(next), next == write|read]}` so
//! the twelve wrong-state scenarios of those commands in `AwaitingModel`, `AwaitingSummary`,
//! `Failed` and `Stopped` are generated again. These cases hold that claim to programs:
//!
//! * the restored scenarios fail against a target that answers those commands in the wrong state,
//!   or reports the wrong state, through the production adapter (`src/boundary.rs`, compiled in
//!   unchanged) with only its answer perturbed;
//! * every `expect_no_error` step the 0.86.0 suite carries is checked against the native loop;
//! * the specification mutated in a scratch copy is refused by the conformance binary: without
//!   `defined(next)` as incomplete coverage, with `write` and `read` swapped as a failed native run;
//! * the inert target passes no scenario;
//! * the ESS pins name one release everywhere they are written.

#[path = "../src/boundary.rs"]
#[allow(dead_code)]
mod boundary;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use ess_conformance::{
    AdmittedSuite, Runner,
    report::Status,
    runner::{Clock, Ids, RunnerConfig},
    target::{
        ConformanceTarget, EventObservationRequest, ExternalOutcomeControl, ImplementationIdentity,
        ObservedEvent, RedeliveryRequest, ScenarioContext, SemanticCommandRequest,
        SemanticCommandResult, SemanticViewRequest, SemanticViewResult, TargetError,
    },
};
use ess_primitives::{node::Node, time::Timestamp};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The three commands whose guards the unit rewrote, with the outcome each names when `next` is
/// absent and nothing is unfolded: the branch a wrong-state target would take.
const GUARDED: [(&str, &str); 3] = [
    ("harness.loop.CompleteTool", "returned"),
    ("harness.loop.Deny", "denied"),
    ("harness.loop.ResumeDenied", "resumed-denied"),
];

/// The four states in which ESS 0.86.0 refused the wrong-state scenarios before the guard change.
const RESTORED_STATES: [&str; 4] = ["AwaitingModel", "AwaitingSummary", "Failed", "Stopped"];

fn restored() -> BTreeSet<String> {
    RESTORED_STATES
        .iter()
        .flat_map(|state| {
            GUARDED.iter().map(move |(command, _)| {
                format!("harness.loop.Loop/state/{state}/refuses/{command}")
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The restored scenarios against a target that gets the wrong state wrong.
// ---------------------------------------------------------------------------------------------

fn integer_tokens(value: Value) -> Value {
    match value {
        Value::Number(n) if n.is_f64() && n.as_f64().unwrap().fract() == 0.0 => {
            Value::Number(format!("{:.0}", n.as_f64().unwrap()).parse().unwrap())
        }
        Value::Array(v) => Value::Array(v.into_iter().map(integer_tokens).collect()),
        Value::Object(v) => {
            Value::Object(v.into_iter().map(|(k, v)| (k, integer_tokens(v))).collect())
        }
        v => v,
    }
}

fn unavailable(error: impl std::fmt::Display) -> TargetError {
    TargetError::unavailable("native boundary", error.to_string())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mutant {
    /// The production adapter, unchanged.
    None,
    /// `CompleteTool`, `Deny` and `ResumeDenied` are taken in any state: the refusal the adapter
    /// observed is answered as the default branch instead.
    Accepts,
    /// The refusal is given, naming the state the command is for rather than the one the loop is in.
    WrongStateField,
}

struct Target {
    mutant: Mutant,
    loops: boundary::Loops,
}

impl ConformanceTarget for Target {
    fn identity(&self) -> std::result::Result<ImplementationIdentity, TargetError> {
        Ok(ImplementationIdentity::new("harness-adversary-1105b", "0"))
    }
    fn begin_scenario(&self, ctx: &ScenarioContext) -> std::result::Result<(), TargetError> {
        self.loops.begin(&ctx.correlation.to_string());
        Ok(())
    }
    fn end_scenario(&self, ctx: &ScenarioContext) -> std::result::Result<(), TargetError> {
        self.loops.end(&ctx.correlation.to_string());
        Ok(())
    }
    fn execute_command(
        &self,
        request: SemanticCommandRequest,
    ) -> std::result::Result<SemanticCommandResult, TargetError> {
        let input = integer_tokens(serde_json::to_value(&request.input).map_err(unavailable)?);
        let command = request.command.to_string();
        let mut answer = self
            .loops
            .execute(&request.correlation.to_string(), &command, &input)
            .map_err(unavailable)?;
        if let Some((_, default)) = GUARDED.iter().find(|(name, _)| *name == command)
            && answer.outcome == "not-awaiting"
        {
            match self.mutant {
                Mutant::None => {}
                Mutant::Accepts => {
                    (*default).clone_into(&mut answer.outcome);
                    answer.error = None;
                    answer.response = Some(json!({"turn": 1}));
                }
                Mutant::WrongStateField => {
                    let awaited = if command == "harness.loop.CompleteTool" {
                        "AwaitingTool"
                    } else if command == "harness.loop.Deny" {
                        "AwaitingApproval"
                    } else {
                        "Suspended"
                    };
                    if let Some((_, fields)) = answer.error.as_mut() {
                        *fields = json!({"state": awaited});
                    }
                }
            }
        }
        answer.into_result(&command).map_err(unavailable)
    }
    fn query_view(
        &self,
        request: SemanticViewRequest,
    ) -> std::result::Result<SemanticViewResult, TargetError> {
        let rows = self
            .loops
            .rows(&request.correlation.to_string())
            .into_iter()
            .map(serde_json::from_value::<BTreeMap<String, Node>>)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(unavailable)?;
        Ok(SemanticViewResult::of(rows))
    }
    fn observe_events(
        &self,
        _: EventObservationRequest,
    ) -> std::result::Result<Vec<ObservedEvent>, TargetError> {
        Err(TargetError::unsupported("events", "none"))
    }
    fn configure_external_outcome(
        &self,
        _: ExternalOutcomeControl,
    ) -> std::result::Result<(), TargetError> {
        Err(TargetError::unsupported("external outcome", "none"))
    }
    fn redeliver_event(&self, _: RedeliveryRequest) -> std::result::Result<(), TargetError> {
        Err(TargetError::unsupported("redelivery", "none"))
    }
}

struct WallClock;
impl Clock for WallClock {
    fn now(&mut self) -> Timestamp {
        Timestamp::from_epoch_millis(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
                .try_into()
                .unwrap(),
        )
    }
}

fn suite_json() -> Value {
    serde_json::from_str(&fs::read_to_string(root().join("conformance/suite.json")).unwrap())
        .unwrap()
}

/// Each scenario of the checked-in suite run against the production adapter under `mutant`: its
/// id, status, and the `about` and status of every check it ran.
fn run(mutant: Mutant) -> BTreeMap<String, (Status, Vec<(String, Status)>)> {
    let admitted = AdmittedSuite::from_json(
        &fs::read_to_string(root().join("conformance/suite.json")).unwrap(),
    )
    .unwrap();
    let target = Target {
        mutant,
        loops: boundary::Loops::default(),
    };
    let run = Runner::new(
        RunnerConfig::default(),
        WallClock,
        Ids::for_suite(admitted.suite()),
    )
    .run_admitted(&admitted, &target);
    run.scenarios
        .iter()
        .map(|scenario| {
            let id = serde_json::to_value(&scenario.scenario).unwrap();
            (
                id.as_str().unwrap().to_owned(),
                (
                    scenario.status,
                    scenario
                        .checks
                        .iter()
                        .map(|check| (check.about.clone(), check.status))
                        .collect(),
                ),
            )
        })
        .collect()
}

#[test]
fn the_restored_wrong_state_scenarios_pass_against_the_native_loop() {
    let results = run(Mutant::None);
    for id in restored() {
        let (status, _) = results
            .get(&id)
            .unwrap_or_else(|| panic!("{id} is not in the run"));
        assert_eq!(*status, Status::Passed, "{id}");
    }
}

#[test]
fn a_target_taking_guarded_commands_in_the_wrong_state_fails_every_restored_scenario() {
    let results = run(Mutant::Accepts);
    let survived: Vec<String> = restored()
        .into_iter()
        .filter(|id| results.get(id).is_none_or(|(s, _)| *s == Status::Passed))
        .collect();
    assert_eq!(
        survived,
        Vec::<String>::new(),
        "a restored wrong-state scenario passed against a target that took the command"
    );
}

#[test]
fn a_target_naming_the_awaited_state_in_its_refusal_fails_every_restored_scenario() {
    let results = run(Mutant::WrongStateField);
    let survived: Vec<String> = restored()
        .into_iter()
        .filter(|id| results.get(id).is_none_or(|(s, _)| *s == Status::Passed))
        .collect();
    assert_eq!(
        survived,
        Vec::<String>::new(),
        "a restored wrong-state scenario passed against a refusal naming the wrong state"
    );
}

#[test]
fn every_expect_no_error_step_is_checked_against_the_native_loop() {
    let suite = suite_json();
    let declared: usize = suite["scenarios"]
        .as_object()
        .unwrap()
        .values()
        .map(|scenario| {
            scenario["steps"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|step| step["step"] == "expect_no_error")
                .count()
        })
        .sum();
    assert_eq!(
        declared, 196,
        "the 0.86.0 suite carries 196 expect_no_error steps"
    );
    let results = run(Mutant::None);
    assert_eq!(results.len(), 183);
    let mut checked = 0;
    for (id, (status, checks)) in &results {
        assert_eq!(*status, Status::Passed, "{id}");
        for (about, status) in checks {
            if about == "no error" {
                assert_eq!(*status, Status::Passed, "{id}: {about}");
                checked += 1;
            }
        }
    }
    assert_eq!(checked, declared, "an expect_no_error step was not checked");
}

// ---------------------------------------------------------------------------------------------
// The specification mutated in a scratch copy, through the conformance binary.
// ---------------------------------------------------------------------------------------------

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// A scratch copy of `spec/` and `conformance/` under the build's temporary directory, with
/// `edit` applied to `spec/domains/loop.yaml`.
fn mutated_root(name: &str, edit: impl FnOnce(String) -> String) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("adv-1105b-u1-{name}"));
    let _ = fs::remove_dir_all(&dir);
    copy_tree(&root().join("spec"), &dir.join("spec"));
    copy_tree(&root().join("conformance"), &dir.join("conformance"));
    let path = dir.join("spec/domains/loop.yaml");
    let before = fs::read_to_string(&path).unwrap();
    let after = edit(before.clone());
    assert_ne!(before, after, "{name}: the mutation changed nothing");
    fs::write(&path, after).unwrap();
    dir
}

/// Replaces the guard of outcome `outcome` (the `when_subject:` line after its name) with `guard`.
fn guard(text: &str, outcome: &str, guard: &str) -> String {
    let name = format!("      - name: {outcome}");
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let at = lines
        .iter()
        .position(|line| *line == name)
        .unwrap_or_else(|| panic!("no outcome {outcome}"))
        + 1;
    assert!(
        lines[at].starts_with("        when_subject: {predicate: "),
        "{outcome} has no subject guard: {}",
        lines[at]
    );
    lines[at] = format!("        when_subject: {{predicate: {guard}}}");
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn conformance(root: &Path, extra: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_b10x-harness-conformance"))
        .arg("--root")
        .arg(root)
        .args(extra)
        .output()
        .unwrap()
}

/// The prefix the three commands' guarded outcomes share.
const PREFIXES: [&str; 3] = ["returned", "denied", "resumed-denied"];

#[test]
fn the_guard_helper_rewrites_exactly_the_named_guard() {
    let text = fs::read_to_string(root().join("spec/domains/loop.yaml")).unwrap();
    let edited = guard(&text, "returned-then-ask", "next == write");
    assert_eq!(
        edited.lines().count(),
        text.lines().count(),
        "a guard rewrite must not join or split lines"
    );
    assert!(edited.contains(
        "      - name: returned-then-ask\n        when_subject: {predicate: next == write}\n        moves: harness.loop.Loop.ask-next\n"
    ));
    assert_eq!(
        edited.matches("defined(next)").count(),
        text.matches("defined(next)").count() - 1
    );
}

#[test]
fn a_guard_without_defined_next_is_refused_as_incomplete_coverage() {
    for prefix in PREFIXES {
        let outcome = format!("{prefix}-then-ask");
        let dir = mutated_root(&format!("undefined-{prefix}"), |text| {
            guard(&text, &outcome, "next == write")
        });
        let out = conformance(&dir, &["--write-projections"]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{outcome}: accepted\n{stderr}");
        assert!(
            stderr.contains("ESS coverage inventory incomplete"),
            "{outcome}: {stderr}"
        );
    }
}

#[test]
fn write_and_read_swapped_in_a_guard_pair_fail_the_native_run() {
    for prefix in PREFIXES {
        let ask = format!("{prefix}-then-ask");
        let call = format!("{prefix}-then-call");
        let dir = mutated_root(&format!("swapped-{prefix}"), |text| {
            let text = guard(&text, &ask, "{all: [defined(next), next == read]}");
            guard(&text, &call, "{all: [defined(next), next == write]}")
        });
        let out = conformance(&dir, &["--write-projections"]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{prefix}: swapped guards accepted");
        assert!(
            stderr.contains("ESS native conformance failed"),
            "{prefix}: refused for another reason: {}",
            stderr.lines().last().unwrap_or_default()
        );
    }
}

#[test]
fn one_guard_reading_the_other_tool_is_refused() {
    for prefix in PREFIXES {
        let call = format!("{prefix}-then-call");
        let dir = mutated_root(&format!("shadowed-{prefix}"), |text| {
            guard(&text, &call, "{all: [defined(next), next == write]}")
        });
        let out = conformance(&dir, &["--write-projections"]);
        assert!(
            !out.status.success(),
            "{prefix}: a shadowed branch accepted"
        );
    }
}

#[test]
fn the_inert_target_passes_no_scenario() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("adv-1105b-u1-noop");
    let _ = fs::remove_dir_all(&dir);
    copy_tree(&root().join("spec"), &dir.join("spec"));
    copy_tree(&root().join("conformance"), &dir.join("conformance"));
    let out = conformance(&dir, &["--audit-noop"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("Harness ESS: 183 total, 0 passed"),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

// ---------------------------------------------------------------------------------------------
// One release, wherever the pin is written.
// ---------------------------------------------------------------------------------------------

#[test]
fn the_ess_pins_name_one_release() {
    let manifest: Value =
        serde_yaml_ng::from_str(&fs::read_to_string(root().join("spec/ess-inputs.yaml")).unwrap())
            .unwrap();
    let version = manifest["requires"]
        .as_str()
        .unwrap()
        .strip_prefix("ess ")
        .unwrap()
        .to_owned();

    let toml = fs::read_to_string(root().join("crates/harness-conformance/Cargo.toml")).unwrap();
    let revs: BTreeSet<&str> = toml
        .lines()
        .filter(|line| line.starts_with("ess-"))
        .map(|line| {
            let start = line.find("rev = \"").unwrap() + 7;
            &line[start..start + 40]
        })
        .collect();
    assert_eq!(revs.len(), 1, "the ess-* crates pin {revs:?}");
    let rev = *revs.iter().next().unwrap();

    let lock = fs::read_to_string(root().join("Cargo.lock")).unwrap();
    let mut locked = BTreeMap::new();
    for package in lock.split("[[package]]") {
        let field = |name: &str| {
            package
                .lines()
                .find_map(|line| line.strip_prefix(&format!("{name} = \"")))
                .map(|rest| rest.trim_end_matches('"').to_owned())
        };
        if let Some(source) = field("source")
            && source.contains("github.com/beyond10x/ess")
        {
            locked.insert(
                field("name").unwrap(),
                (field("version").unwrap(), source.clone()),
            );
        }
    }
    assert_eq!(locked.len(), 8, "{locked:?}");
    for (name, (locked_version, source)) in &locked {
        assert_eq!(locked_version, &version, "{name}");
        assert!(
            source.ends_with(&format!("?rev={rev}#{rev}")),
            "{name}: {source}"
        );
    }

    let main = fs::read_to_string(root().join("crates/harness-conformance/src/main.rs")).unwrap();
    assert!(main.contains(&format!("manifest[\"requires\"] != \"ess {version}\"")));
    let agents = fs::read_to_string(root().join("AGENTS.md")).unwrap();
    assert!(agents.contains(&format!("`spec/ess-inputs.yaml` pins ESS {version}.")));
    let readme = fs::read_to_string(root().join("spec/README.md")).unwrap();
    assert!(readme.contains(&format!("The maintained toolchain is ESS **{version}**")));
    let coverage = fs::read_to_string(root().join("spec/COVERAGE.md")).unwrap();
    assert!(coverage.contains(&format!("**ESS {version} workarounds.**")));
}
