//! Adversary cases for `story:loop-stop-is-a-tagged-union` (wave d221, unit 3).
//!
//! Two directions, both driven by the real specification and the real `LoopStop`:
//!
//! * the type the specification declares is exactly `LoopStop`: every value the Rust type can
//!   produce, written in the ESS wire form, compiles; a value with a field dropped, a foreign field
//!   added, a field renamed or a field of the wrong type is refused;
//! * the suite cannot hide a regression behind the adapter's `tagged()`: a response whose stop
//!   carries an extra, renamed or missing field, an unknown kind, the serde (internally tagged)
//!   form, or an empty `value` on `completed`, fails the scenarios that observe it.
//!
//! The adapter's own module is compiled in here unchanged (`#[path]`), so the run goes through the
//! production `boundary::execute` and only the response it returned is perturbed.

#[path = "../src/boundary.rs"]
#[allow(dead_code)]
mod boundary;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    sync::Mutex,
};

use ess_compiler::{ir::EssIr, source::SourceMap};
use ess_conformance::{
    AdmittedSuite, Runner,
    authored::Cause,
    coverage_build::{self, CoverageSource},
    report::Status,
    runner::{Clock, Ids, RunnerConfig},
    target::{
        ConformanceTarget, EventObservationRequest, ExternalOutcomeControl, ImplementationIdentity,
        ObservedEvent, RedeliveryRequest, ScenarioContext, SemanticCommandRequest,
        SemanticCommandResult, SemanticViewRequest, SemanticViewResult, TargetError,
    },
};
use ess_primitives::{node::Node, time::Timestamp};
use harness_loop::LoopStop;
use serde_json::{Map, Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn spec() -> PathBuf {
    root().join("spec")
}

// ---------------------------------------------------------------------------------------------
// Direction 1: the declared union is exactly LoopStop.
// ---------------------------------------------------------------------------------------------

fn model() -> EssIr {
    let manifest: Value =
        serde_yaml_ng::from_str(&fs::read_to_string(spec().join("ess-inputs.yaml")).unwrap())
            .unwrap();
    let paths: Vec<&str> = manifest["specification"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    let texts: Vec<String> = paths
        .iter()
        .map(|p| fs::read_to_string(spec().join(p)).unwrap())
        .collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let mut sources = SourceMap::new();
    let mut parsed = Vec::new();
    for ((path, text), raw) in paths
        .iter()
        .zip(&texts)
        .zip(ess_domain::spec::RawSpecFile::parse_all(&refs))
    {
        sources.insert(*path, text);
        parsed.push((ess_domain::system::Source::new(*path), raw.unwrap()));
    }
    let model = ess_domain::spec::Specification::assemble(parsed).unwrap();
    ess_compiler::compile(&model, &sources).unwrap()
}

/// One value of every `LoopStop` variant. The match is exhaustive: a new variant does not compile
/// here until it has a sample, so the specification cannot silently lag the type.
fn samples() -> Vec<LoopStop> {
    let all = vec![
        LoopStop::Completed,
        LoopStop::MaxTurns { limit: 3 },
        LoopStop::MaxInputTokens {
            limit: 10,
            reported: 11,
        },
        LoopStop::MaxOutputTokens {
            limit: 5,
            reported: 6,
        },
        LoopStop::MaxCost {
            limit_micro_usd: 100,
            spent_micro_usd: 120,
        },
        LoopStop::BudgetUnobservable {
            name: "max_input_tokens".into(),
            reason: "usage omitted".into(),
        },
        LoopStop::Deadline { limit_ms: 1000 },
        LoopStop::Cancelled {
            reason: "the caller cancelled".into(),
        },
        LoopStop::AwaitingApproval {
            checkpoint_id: "cp-1".into(),
        },
        LoopStop::ProviderIncomplete {
            reason: "max_output_tokens".into(),
        },
        LoopStop::Unstructured { asked_again: 2 },
        LoopStop::Failed {
            cause: "provider-refused".into(),
            message: "refused".into(),
            after_compaction: true,
        },
    ];
    for stop in &all {
        match stop {
            LoopStop::Completed
            | LoopStop::MaxTurns { .. }
            | LoopStop::MaxInputTokens { .. }
            | LoopStop::MaxOutputTokens { .. }
            | LoopStop::MaxCost { .. }
            | LoopStop::BudgetUnobservable { .. }
            | LoopStop::Deadline { .. }
            | LoopStop::Cancelled { .. }
            | LoopStop::AwaitingApproval { .. }
            | LoopStop::ProviderIncomplete { .. }
            | LoopStop::Unstructured { .. }
            | LoopStop::Failed { .. } => {}
        }
    }
    all
}

/// The serde serialization of `stop`, written independently of the adapter in the ESS adjacent
/// form: the tag, and the remaining fields under `value` unless there are none.
fn adjacent(serde: &Value) -> Value {
    let mut fields = serde.as_object().unwrap().clone();
    let kind = fields.remove("kind").unwrap();
    if fields.is_empty() {
        json!({"kind": kind})
    } else {
        json!({"kind": kind, "value": fields})
    }
}

fn completed_run(name: &str, stop: &Value) -> CoverageSource {
    let mut scenario: Value = serde_json::from_str(
        &fs::read_to_string(spec().join("scenarios/run-completed.yaml")).unwrap(),
    )
    .unwrap();
    scenario["scenario"] = json!(name);
    scenario["timeline"][0]["response"]["stop"] = stop.clone();
    CoverageSource::new(
        format!("scenarios/{name}.yaml"),
        serde_json::to_string_pretty(&scenario).unwrap(),
    )
    .unwrap()
}

fn refusals(ir: &EssIr, name: &str, stop: &Value) -> Vec<String> {
    let batch = coverage_build::compile_sources(ir, &[completed_run(name, stop)]).unwrap();
    batch
        .refusals()
        .map(|refusal| {
            let detail = match &refusal.cause {
                Cause::Unreadable { detail } => detail.clone(),
                other => other.to_string(),
            };
            format!("{} {detail}", refusal.code())
        })
        .collect()
}

#[test]
fn every_loop_stop_the_type_can_produce_is_admitted_by_the_union() {
    let ir = model();
    let mut wrong = Vec::new();
    for (i, stop) in samples().iter().enumerate() {
        let wire = adjacent(&serde_json::to_value(stop).unwrap());
        let refused = refusals(&ir, &format!("adv-admit-{i}"), &wire);
        if !refused.is_empty() {
            wrong.push(format!("{wire} refused: {refused:?}"));
        }
    }
    assert_eq!(wrong, Vec::<String>::new());
}

/// Every field name any variant owns, with a value of its own type.
fn foreign_fields() -> BTreeMap<String, Value> {
    let mut all = BTreeMap::new();
    for stop in samples() {
        if let Value::Object(fields) = serde_json::to_value(&stop).unwrap() {
            for (k, v) in fields {
                if k != "kind" {
                    all.insert(k, v);
                }
            }
        }
    }
    all
}

#[test]
fn every_value_no_loop_stop_can_be_is_refused_by_the_union() {
    let ir = model();
    let foreign = foreign_fields();
    let mut admitted = Vec::new();
    let mut n = 0;
    let mut check = |wire: Value, why: String| {
        n += 1;
        if refusals(&ir, &format!("adv-refuse-{n}"), &wire).is_empty() {
            admitted.push(format!("{why}: {wire}"));
        }
    };
    for stop in samples() {
        let serde = serde_json::to_value(&stop).unwrap();
        let mut own: Map<String, Value> = serde.as_object().unwrap().clone();
        let kind = own.remove("kind").unwrap();
        // A field another variant owns (or an unknown one), added to this variant.
        for (name, value) in foreign.iter().chain([(&"bogus".to_owned(), &json!(1))]) {
            if own.contains_key(name) {
                continue;
            }
            let mut fields = own.clone();
            fields.insert(name.clone(), value.clone());
            check(
                json!({"kind": kind, "value": fields}),
                format!("{kind} plus {name}"),
            );
        }
        for name in own.keys() {
            // A required field dropped.
            let mut dropped = own.clone();
            dropped.remove(name);
            let wire = if dropped.is_empty() {
                json!({"kind": kind})
            } else {
                json!({"kind": kind, "value": dropped})
            };
            check(wire, format!("{kind} without {name}"));
            // A field renamed to its camelCase / unrelated spelling.
            let mut renamed = own.clone();
            let value = renamed.remove(name).unwrap();
            renamed.insert(format!("{name}_x"), value.clone());
            check(
                json!({"kind": kind, "value": renamed}),
                format!("{kind} with {name} renamed"),
            );
            // A field of the wrong type.
            let mut retyped = own.clone();
            let other = match value {
                Value::String(_) => json!(7),
                _ => json!("seven"),
            };
            retyped.insert(name.clone(), other);
            check(
                json!({"kind": kind, "value": retyped}),
                format!("{kind} with {name} retyped"),
            );
            // The field optional: written as null.
            let mut nulled = own.clone();
            nulled.insert(name.clone(), Value::Null);
            check(
                json!({"kind": kind, "value": nulled}),
                format!("{kind} with {name} null"),
            );
        }
        // The serde (internally tagged) form, fields beside the tag.
        if !own.is_empty() {
            check(serde.clone(), format!("{kind} internally tagged"));
        }
    }
    check(json!({"kind": "bogus"}), "an unknown kind".into());
    check(
        json!({"kind": "MaxTurns", "value": {"limit": 1}}),
        "a PascalCase kind".into(),
    );
    check(
        json!({"kind": "max_turns", "value": {"limit": 1}}),
        "a snake_case kind".into(),
    );
    check(json!({"value": {"limit": 1}}), "no kind".into());
    check(
        json!({"kind": "completed", "value": {}}),
        "completed with an empty value".into(),
    );
    assert_eq!(admitted, Vec::<String>::new());
}

// ---------------------------------------------------------------------------------------------
// Direction 2: the suite goes red when the stop the adapter returns drifts from the expected one.
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

type Perturb = fn(&mut Value) -> bool;

struct Target {
    perturb: Perturb,
    current: Mutex<String>,
    /// Each scenario the perturbation reached, with the commands whose stop it changed.
    touched: Mutex<BTreeMap<String, BTreeSet<String>>>,
    loops: boundary::Loops,
}
fn unavailable(error: impl std::fmt::Display) -> TargetError {
    TargetError::unavailable("native boundary", error.to_string())
}
impl ConformanceTarget for Target {
    fn identity(&self) -> std::result::Result<ImplementationIdentity, TargetError> {
        Ok(ImplementationIdentity::new("harness-adversary", "0"))
    }
    fn begin_scenario(&self, ctx: &ScenarioContext) -> std::result::Result<(), TargetError> {
        *self.current.lock().unwrap() = serde_json::to_string(&ctx.scenario).unwrap();
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
        if let Some(stop) = answer
            .response
            .as_mut()
            .and_then(|response| response.get_mut("stop"))
            && (self.perturb)(stop)
        {
            self.touched
                .lock()
                .unwrap()
                .entry(self.current.lock().unwrap().clone())
                .or_default()
                .insert(command.clone());
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

/// Runs the checked-in suite (drift-checked against native synthesis by `cargo xtask
/// specification`) against the production boundary with `perturb` applied to every stop; returns
/// (scenarios that observed a perturbed stop, those of them that still passed, total passed).
///
/// A scenario observes a stop when it checks the direct response of a command whose stop was
/// perturbed. Since story:harness-loop-modeled-in-ess a scenario may also reach a state through a
/// command that returns a stop and never read that response: the stop it arranges with is the
/// subject of that command's own scenarios, which must fail, and not of this one.
fn run(perturb: Perturb) -> (BTreeSet<String>, Vec<String>, usize) {
    let admitted = AdmittedSuite::from_json(
        &fs::read_to_string(root().join("conformance/suite.json")).unwrap(),
    )
    .unwrap();
    let target = Target {
        perturb,
        current: Mutex::new(String::new()),
        touched: Mutex::new(BTreeMap::new()),
        loops: boundary::Loops::default(),
    };
    let run = Runner::new(
        RunnerConfig::default(),
        WallClock,
        Ids::for_suite(admitted.suite()),
    )
    .run_admitted(&admitted, &target);
    let perturbed = target.touched.into_inner().unwrap();
    let mut touched = BTreeSet::new();
    let mut survived = Vec::new();
    let mut passed = 0;
    for scenario in &run.scenarios {
        let id = serde_json::to_string(&scenario.scenario).unwrap();
        let observed = perturbed.get(&id).is_some_and(|commands| {
            commands.iter().any(|command| {
                let read = format!("direct response {command}");
                scenario.checks.iter().any(|check| check.about == read)
            })
        });
        if observed {
            touched.insert(id.clone());
        }
        if scenario.status == Status::Passed {
            passed += 1;
            if observed {
                survived.push(id);
            }
        }
    }
    (touched, survived, passed)
}

fn with_value(stop: &mut Value, f: impl FnOnce(&mut Map<String, Value>)) -> bool {
    match stop.get_mut("value").and_then(Value::as_object_mut) {
        Some(fields) => {
            f(fields);
            true
        }
        None => false,
    }
}

#[test]
fn the_unperturbed_production_boundary_passes_every_scenario() {
    let (_, _, passed) = run(|_| false);
    assert_eq!(
        passed, 183,
        "story:harness-loop-modeled-in-ess: 29 scenarios before, plus 136 generated and 18 \
         authored ones for the Loop entity"
    );
}

#[test]
fn a_stop_that_gained_a_field_fails_its_scenarios() {
    let (touched, survived, _) = run(|stop| {
        with_value(stop, |v| {
            v.insert("extra".into(), json!(1));
        })
    });
    assert!(touched.len() >= 8, "touched {touched:?}");
    assert_eq!(
        survived,
        Vec::<String>::new(),
        "story:harness-loop-modeled-in-ess: a scenario that read the perturbed stop passed"
    );
}

#[test]
fn a_stop_with_a_renamed_field_fails_its_scenarios() {
    let (touched, survived, _) = run(|stop| {
        with_value(stop, |v| {
            let (k, x) = v
                .iter()
                .next()
                .map(|(k, x)| (k.clone(), x.clone()))
                .unwrap();
            v.remove(&k);
            v.insert(format!("{k}_renamed"), x);
        })
    });
    assert!(touched.len() >= 8, "touched {touched:?}");
    assert_eq!(
        survived,
        Vec::<String>::new(),
        "story:harness-loop-modeled-in-ess: a scenario that read the perturbed stop passed"
    );
}

#[test]
fn a_stop_that_lost_a_field_fails_its_scenarios() {
    let (touched, survived, _) = run(|stop| {
        with_value(stop, |v| {
            let k = v.keys().next().cloned().unwrap();
            v.remove(&k);
        })
    });
    assert!(touched.len() >= 8, "touched {touched:?}");
    assert_eq!(
        survived,
        Vec::<String>::new(),
        "story:harness-loop-modeled-in-ess: a scenario that read the perturbed stop passed"
    );
}

#[test]
fn a_stop_in_the_serde_form_fails_its_scenarios() {
    let (touched, survived, _) = run(|stop| {
        let Some(Value::Object(fields)) = stop.as_object_mut().unwrap().remove("value") else {
            return false;
        };
        stop.as_object_mut().unwrap().extend(fields);
        true
    });
    assert!(touched.len() >= 8, "touched {touched:?}");
    assert_eq!(
        survived,
        Vec::<String>::new(),
        "story:harness-loop-modeled-in-ess: a scenario that read the perturbed stop passed"
    );
}

#[test]
fn a_stop_with_an_unknown_kind_fails_its_scenarios() {
    let (touched, survived, _) = run(|stop| match stop.as_object_mut() {
        Some(o) if o.contains_key("kind") => {
            o.insert("kind".into(), json!("bogus"));
            true
        }
        _ => false,
    });
    assert!(touched.len() >= 9, "touched {touched:?}");
    assert_eq!(
        survived,
        Vec::<String>::new(),
        "story:harness-loop-modeled-in-ess: a scenario that read the perturbed stop passed"
    );
}

#[test]
fn a_completed_stop_with_an_empty_value_fails_its_scenarios() {
    let (touched, survived, _) = run(|stop| {
        if stop.get("kind") == Some(&json!("completed")) {
            stop["value"] = json!({});
            true
        } else {
            false
        }
    });
    assert!(!touched.is_empty(), "touched nothing");
    assert_eq!(
        survived,
        Vec::<String>::new(),
        "story:harness-loop-modeled-in-ess: a scenario that read the perturbed stop passed"
    );
}

#[test]
fn a_stop_with_a_changed_value_fails_its_scenarios() {
    let (touched, survived, _) = run(|stop| {
        with_value(stop, |v| {
            for x in v.values_mut() {
                let changed = match &*x {
                    Value::Number(n) => json!(n.as_u64().unwrap() + 1),
                    Value::String(s) => json!(format!("{s}!")),
                    other => other.clone(),
                };
                *x = changed;
            }
        })
    });
    assert!(touched.len() >= 8, "touched {touched:?}");
    // A well-typed but different value is invisible to the generated outcome scenario, which
    // asserts no literal; every authored scenario that observes the stop must catch it.
    let authored: Vec<&String> = survived
        .iter()
        .filter(|id| id.contains("/authored/"))
        .collect();
    assert_eq!(
        authored,
        Vec::<&String>::new(),
        "story:harness-loop-modeled-in-ess: an authored scenario that read the perturbed stop passed"
    );
}

// ---------------------------------------------------------------------------------------------
// The drift check still compares when the projections are stale, and only the flag rewrites.
// ---------------------------------------------------------------------------------------------

fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
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

/// A copy of `spec/` and `conformance/` with `file` (under `conformance/`) perturbed.
fn stale_root(name: &str, file: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("adv-d221-u3-{name}"));
    let _ = fs::remove_dir_all(&dir);
    copy_tree(&root().join("spec"), &dir.join("spec"));
    copy_tree(&root().join("conformance"), &dir.join("conformance"));
    let path = dir.join("conformance").join(file);
    let mut value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("adversary".into(), json!("stale"));
    fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    dir
}

fn conformance(root: &std::path::Path, extra: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_b10x-harness-conformance"))
        .arg("--root")
        .arg(root)
        .args(extra)
        .output()
        .unwrap()
}

#[test]
fn a_stale_model_projection_is_refused_without_the_flag() {
    let dir = stale_root("model", "model.json");
    let out = conformance(&dir, &[]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("conformance/model.json differs"));
}

#[test]
fn a_stale_suite_projection_is_refused_without_the_flag() {
    let dir = stale_root("suite", "suite.json");
    let out = conformance(&dir, &[]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("conformance/suite.json differs"));
}

#[test]
fn the_rewritten_projections_equal_the_checked_in_ones() {
    let dir = stale_root("rewrite", "suite.json");
    fs::write(
        dir.join("conformance/model.json"),
        "{\"adversary\": \"stale\"}",
    )
    .unwrap();
    let out = conformance(&dir, &["--write-projections"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for file in ["model.json", "suite.json"] {
        assert_eq!(
            fs::read(dir.join("conformance").join(file)).unwrap(),
            fs::read(root().join("conformance").join(file)).unwrap(),
            "{file} as rewritten differs byte for byte from the checked-in projection",
        );
    }
}
