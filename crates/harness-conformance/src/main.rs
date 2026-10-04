//! Native specification compilation, synthesis and execution. Requires no ambient ESS binary.
mod boundary;

use clap::Parser;
use ess_compiler::source::SourceMap;
use ess_conformance::{
    AdmittedSuite, CountReport, CountRun, Runner,
    counts::CountStatus,
    coverage::{Origins, Scope},
    coverage_build::{self, CoverageSource},
    runner::{Clock, Ids, RunnerConfig},
    target::{
        ConformanceTarget, EventObservationRequest, ExternalOutcomeControl, ImplementationIdentity,
        ObservedEvent, RedeliveryRequest, ScenarioContext, SemanticCommandRequest,
        SemanticCommandResult, SemanticViewRequest, SemanticViewResult, TargetError,
    },
};
use ess_primitives::{node::Node, time::Timestamp};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Parser)]
#[command(about = "Compile and execute the Harness ESS contract through native library boundaries")]
struct Args {
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Deliberately inert adapter: every authored scenario must fail.
    #[arg(long)]
    audit_noop: bool,
}

struct Target {
    noop: bool,
}
impl ConformanceTarget for Target {
    fn identity(&self) -> std::result::Result<ImplementationIdentity, TargetError> {
        Ok(ImplementationIdentity::new(
            if self.noop {
                "harness-noop"
            } else {
                "harness-native"
            },
            env!("CARGO_PKG_VERSION"),
        ))
    }
    fn begin_scenario(&self, _: &ScenarioContext) -> std::result::Result<(), TargetError> {
        Ok(())
    }
    fn end_scenario(&self, _: &ScenarioContext) -> std::result::Result<(), TargetError> {
        Ok(())
    }
    fn execute_command(
        &self,
        request: SemanticCommandRequest,
    ) -> std::result::Result<SemanticCommandResult, TargetError> {
        let input = integer_tokens(serde_json::to_value(&request.input).map_err(unavailable)?)
            .map_err(unavailable)?;
        let response = if self.noop {
            json!({})
        } else {
            boundary::execute(&request.command.to_string(), &input).map_err(unavailable)?
        };
        let mut result = SemanticCommandResult::took(ess_conformance::scenario::OutcomeRef::new(
            request.command,
            "returned".parse().map_err(unavailable)?,
        ));
        let fields: BTreeMap<String, Node> =
            serde_json::from_value(response).map_err(unavailable)?;
        result.response = Some(fields);
        Ok(result)
    }
    fn query_view(
        &self,
        _: SemanticViewRequest,
    ) -> std::result::Result<SemanticViewResult, TargetError> {
        Err(TargetError::unsupported(
            "view",
            "loop/flow values have no persistent view boundary",
        ))
    }
    fn observe_events(
        &self,
        _: EventObservationRequest,
    ) -> std::result::Result<Vec<ObservedEvent>, TargetError> {
        Err(TargetError::unsupported(
            "events",
            "no asynchronous event bus is claimed",
        ))
    }
    fn configure_external_outcome(
        &self,
        _: ExternalOutcomeControl,
    ) -> std::result::Result<(), TargetError> {
        Err(TargetError::unsupported(
            "external outcome",
            "fixture ports are explicit command inputs",
        ))
    }
    fn redeliver_event(&self, _: RedeliveryRequest) -> std::result::Result<(), TargetError> {
        Err(TargetError::unsupported(
            "redelivery",
            "no asynchronous event bus is claimed",
        ))
    }
}
// ESS decimal Integer values can serialize as 1.0. Restore integral JSON tokens before
// invoking the existing u64/u32 serde APIs; never round or narrow a supplied integer.
fn integer_tokens(value: Value) -> Result<Value> {
    Ok(match value {
        Value::Number(number) if number.is_f64() => {
            let value = number.as_f64().ok_or("invalid numeric value")?;
            if value.fract() == 0.0 {
                if value.abs() > 9_007_199_254_740_992.0 {
                    return Err("inexact decimal fixture integer".into());
                }
                Value::Number(format!("{value:.0}").parse()?)
            } else {
                Value::Number(number)
            }
        }
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(integer_tokens)
                .collect::<Result<_>>()?,
        ),
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| Ok((key, integer_tokens(value)?)))
                .collect::<Result<_>>()?,
        ),
        value => value,
    })
}
fn unavailable(error: impl std::fmt::Display) -> TargetError {
    TargetError::unavailable("native boundary", error.to_string())
}
struct WallClock;
impl Clock for WallClock {
    fn now(&mut self) -> Timestamp {
        Timestamp::from_epoch_millis(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("UTC clock")
                .as_millis()
                .try_into()
                .expect("milliseconds fit u64"),
        )
    }
}
fn strings(value: &Value) -> Result<Vec<&str>> {
    value
        .as_array()
        .ok_or("manifest list missing")?
        .iter()
        .map(|v| {
            v.as_str()
                .ok_or_else(|| "manifest entry must be String".into())
        })
        .collect()
}
fn compile(root: &Path) -> Result<AdmittedSuite> {
    let spec = root.join("spec");
    let manifest: Value =
        serde_yaml_ng::from_str(&fs::read_to_string(spec.join("ess-inputs.yaml"))?)?;
    if manifest["requires"] != "ess 0.52.0" || manifest["format"] != "ess-inputs/2" {
        return Err("ESS manifest must match the pinned 0.52.0 compiler".into());
    }
    let paths = strings(&manifest["specification"])?;
    let texts: Vec<String> = paths
        .iter()
        .map(|p| fs::read_to_string(spec.join(p)))
        .collect::<std::io::Result<_>>()?;
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let mut sources = SourceMap::new();
    let mut parsed = Vec::new();
    for ((path, text), raw) in paths
        .iter()
        .zip(&texts)
        .zip(ess_domain::spec::RawSpecFile::parse_all(&refs))
    {
        sources.insert(*path, text);
        parsed.push((ess_domain::system::Source::new(*path), raw?));
    }
    let model = ess_domain::spec::Specification::assemble(parsed)
        .map_err(|e| format!("ESS assembly: {e:?}"))?;
    let ir = ess_compiler::compile(&model, &sources).map_err(|e| format!("ESS compile: {e:?}"))?;
    let expected_model: Value =
        serde_json::from_str(&fs::read_to_string(root.join("conformance/model.json"))?)?;
    let actual_model: Value = serde_json::from_str(&ir.to_canonical_json())?;
    if expected_model != actual_model {
        return Err("conformance/model.json differs from native ESS compilation".into());
    }
    let scenarios: Vec<CoverageSource> = strings(&manifest["scenarios"])?
        .iter()
        .map(|path| {
            Ok(CoverageSource::new(
                *path,
                fs::read_to_string(spec.join(path))?,
            )?)
        })
        .collect::<Result<_>>()?;
    let input = coverage_build::build(
        &ir,
        &scenarios,
        Scope::System,
        Origins::GeneratedAndAuthored,
    )?;
    let suite = input.selected();
    if !suite
        .coverage()
        .ok_or("missing coverage claim")?
        .is_complete()
    {
        return Err("ESS coverage inventory incomplete".into());
    }
    let expected: Value =
        serde_json::from_str(&fs::read_to_string(root.join("conformance/suite.json"))?)?;
    let actual: Value = serde_json::from_str(suite.original_json())?;
    if actual != expected {
        return Err("conformance/suite.json differs from native ESS synthesis; regenerate with the pinned CLI".into());
    }
    AdmittedSuite::from_json(suite.original_json()).map_err(Into::into)
}
fn main() -> Result<()> {
    let args = Args::parse();
    let admitted = compile(&args.root)?;
    let run = Runner::new(
        RunnerConfig::default(),
        WallClock,
        Ids::for_suite(admitted.suite()),
    )
    .run_admitted(
        &admitted,
        &Target {
            noop: args.audit_noop,
        },
    );
    let report = CountReport::from_run(&run, &admitted)?;
    let detailed = CountRun::from_run(&run, &admitted)?;
    let output = args.root.join(".engineering/drafts");
    fs::create_dir_all(&output)?;
    let suffix = if args.audit_noop { "noop" } else { "native" };
    fs::write(
        output.join(format!("harness-{suffix}-report.json")),
        report.to_canonical_json()?,
    )?;
    fs::write(
        output.join(format!("harness-{suffix}-run.json")),
        detailed.to_canonical_json()?,
    )?;
    let counts = report.counts();
    println!(
        "Harness ESS: {} total, {} passed, {} failed, {} skipped, {} unsupported, {} errors",
        counts.total,
        counts.passed,
        counts.failed,
        counts.skipped,
        counts.unsupported,
        counts.error
    );
    if args.audit_noop {
        let value = serde_json::to_value(&detailed)?;
        for scenario in value["scenarios"]
            .as_array()
            .ok_or("scenario results missing")?
        {
            if scenario["scenario"]
                .as_str()
                .ok_or("scenario identity missing")?
                .contains("/authored/")
                && scenario["status"] == "passed"
            {
                return Err(format!(
                    "authored scenario passes a no-op target: {}",
                    scenario["scenario"]
                )
                .into());
            }
        }
        return Ok(());
    }
    let baseline: Value = serde_json::from_str(&fs::read_to_string(
        args.root.join("conformance/baseline.json"),
    )?)?;
    let floor = baseline["answered_floor"]
        .as_u64()
        .ok_or("answered floor missing")?;
    if counts.passed + counts.failed < floor
        || counts.total
            < baseline["total_floor"]
                .as_u64()
                .ok_or("total floor missing")?
        || counts.skipped
            > baseline["skipped_ceiling"]
                .as_u64()
                .ok_or("skip ceiling missing")?
    {
        return Err("ESS coverage baseline regressed".into());
    }
    let suite: Value = serde_json::from_str(admitted.original_json())?;
    for id in strings(&baseline["required_scenarios"])? {
        if suite["scenarios"].get(id).is_none() {
            return Err(format!("required scenario removed: {id}").into());
        }
    }
    if report.conformance_status() != CountStatus::Passed {
        for scenario in &run.scenarios {
            if scenario.status != ess_conformance::report::Status::Passed {
                eprintln!("{scenario:#?}");
            }
        }
        return Err("ESS native conformance failed; inspect report document".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::integer_tokens;
    use serde_json::json;

    #[test]
    fn integer_adapter_preserves_unsigned_width_and_exact_decimal_tokens() {
        let value = integer_tokens(json!({"wide":u64::MAX,"decimal":1.0,"zero":0.0})).unwrap();
        assert_eq!(value["wide"].as_u64(), Some(u64::MAX));
        assert_eq!(value["decimal"].as_u64(), Some(1));
        assert_eq!(value["zero"].as_u64(), Some(0));
        assert!(integer_tokens(json!(18_446_744_073_709_551_616.0)).is_err());
        assert_eq!(integer_tokens(json!(1.5)).unwrap(), json!(1.5));
    }
}
