//! The loop stop is a tagged union: a variant carries exactly its own fields.
//!
//! `LoopStop::Completed` carries nothing (`crates/harness-loop/src/lib.rs:129`), so a scenario that
//! expects a `completed` stop holding `limit_ms` describes a value the type cannot produce. ESS
//! refuses that scenario when it compiles it, before any target runs.

use std::{fs, path::PathBuf};

use ess_compiler::{ir::EssIr, source::SourceMap};
use ess_conformance::{
    authored::Cause,
    coverage_build::{self, CoverageSource},
};
use serde_json::{Value, json};

fn spec() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec")
}

/// The model `spec/ess-inputs.yaml` selects, compiled by the pinned ESS libraries.
fn model() -> EssIr {
    let manifest: Value = serde_yaml_ng::from_str(
        &fs::read_to_string(spec().join("ess-inputs.yaml")).expect("read ess-inputs.yaml"),
    )
    .expect("parse ess-inputs.yaml");
    let paths: Vec<&str> = manifest["specification"]
        .as_array()
        .expect("specification list")
        .iter()
        .map(|path| path.as_str().expect("specification path"))
        .collect();
    let texts: Vec<String> = paths
        .iter()
        .map(|path| fs::read_to_string(spec().join(path)).expect("read specification"))
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
        parsed.push((
            ess_domain::system::Source::new(*path),
            raw.expect("parse specification"),
        ));
    }
    let model = ess_domain::spec::Specification::assemble(parsed).expect("assemble specification");
    ess_compiler::compile(&model, &sources).expect("compile specification")
}

/// `spec/scenarios/run-completed.yaml` under `name`, its expected stop replaced by `stop`.
fn completed_run(name: &str, stop: Option<Value>) -> CoverageSource {
    let mut scenario: Value = serde_json::from_str(
        &fs::read_to_string(spec().join("scenarios/run-completed.yaml"))
            .expect("read run-completed"),
    )
    .expect("run-completed is JSON");
    scenario["scenario"] = json!(name);
    if let Some(stop) = stop {
        scenario["timeline"][0]["response"]["stop"] = stop;
    }
    CoverageSource::new(
        format!("scenarios/{name}.yaml"),
        serde_json::to_string_pretty(&scenario).expect("serialize scenario"),
    )
    .expect("scenario source identity")
}

/// The refusals ESS gives `source` alone, as `(code, detail)`.
fn refusals(ir: &EssIr, source: CoverageSource) -> Vec<(String, String)> {
    let batch = coverage_build::compile_sources(ir, &[source]).expect("admit the model");
    batch
        .refusals()
        .map(|refusal| {
            let detail = match &refusal.cause {
                Cause::Unreadable { detail } => detail.clone(),
                other => other.to_string(),
            };
            (refusal.code().to_string(), detail)
        })
        .collect()
}

#[test]
fn the_completed_scenario_as_written_compiles() {
    assert_eq!(
        refusals(&model(), completed_run("run-completed", None)),
        Vec::<(String, String)>::new()
    );
}

#[test]
fn a_completed_stop_carrying_limit_ms_is_refused_by_ess() {
    let refused = refusals(
        &model(),
        completed_run(
            "run-completed-carrying-limit-ms",
            Some(json!({"kind": "completed", "limit_ms": 5})),
        ),
    );
    assert_eq!(
        refused,
        vec![(
            "ESS-AUTHOR-001".to_owned(),
            "response literal stop: invalid_input".to_owned()
        )]
    );
}
