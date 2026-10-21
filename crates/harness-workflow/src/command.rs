//! What one `run` call says a `command` step did.

use harness_wire::ToolOutcome;
use serde_json::Value;

use crate::document::plain;

/// What one `run` call says the command step did: exit `0` passed, everything else did not.
///
/// A refusal at any stage of the gate, a tool that failed the call, a killed process and a non-zero
/// exit are all the step failing, each with its own words. The words are what a reader gets, so a
/// suite that went red and a program nobody approved are not the same line in the record.
///
/// # Errors
///
/// The reason the step is not passed, in one sentence.
pub(crate) fn command_outcome(outcome: &ToolOutcome) -> Result<(), String> {
    if let Some(refusal) = &outcome.refusal {
        return Err(format!("it was refused by rule: {}", refusal.message()));
    }
    if outcome.failed {
        return Err(format!(
            "it did not run: {}",
            outcome.output.as_str().map_or_else(
                || plain(&outcome.output),
                |text| text.chars().take(240).collect()
            )
        ));
    }
    if outcome
        .output
        .get("timed_out")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err("it timed out".to_owned());
    }
    match exit_of(&outcome.output) {
        Exit::Code(0) => Ok(()),
        Exit::Code(code) => Err(format!("exit {code}")),
        Exit::Signal(signal) => Err(format!("killed by signal {signal}")),
        Exit::Unfinished(state) => Err(format!("it did not exit (state: {state})")),
        Exit::Absent => Err("it ended without an exit code".to_owned()),
    }
}

/// How a program the `run` tool ran came to an end.
#[derive(Debug, PartialEq, Eq)]
enum Exit {
    Code(i64),
    /// Substrate names a signal (`KILL`, `TERM`, `INT`); the local port has no field for one.
    Signal(String),
    /// The confined record says the execution is in a state that is not `exited` — `cancelled`,
    /// `expired` — and carries no status.
    Unfinished(String),
    Absent,
}

/// The exit, read from **either shape the `run` tool answers in**.
///
/// The local port writes `exit: <code>` (`tool_process::RunOutput`, which the
/// `harness-catalogue` adapter answers field for field): `null` for a killed process, and the
/// program's own code on a timed-out run whose program had already ended — `command_outcome`
/// reads `timed_out` first, so such a run never reaches this;
/// the confined port writes `exit: <the execution record>` with `exit.exit: {code, signal}`
/// (`harness-substrate/embedded.rs` and `client.rs`, substrate's own resource, serialised). The
/// sixth paid native walk ran its validator inside the sandbox, saw it exit `0`, and read the
/// record as *no exit code* because only the first shape was looked at — a passed verifier
/// reported as a failed step, which is the misreport a command step exists to prevent.
fn exit_of(output: &Value) -> Exit {
    let read = |exit: &Value| match exit {
        Value::Number(code) => code.as_i64().map_or(Exit::Absent, Exit::Code),
        Value::Object(status) => match (
            status.get("code").and_then(Value::as_i64),
            status.get("signal").filter(|signal| !signal.is_null()),
        ) {
            (Some(code), _) => Exit::Code(code),
            (None, Some(signal)) => Exit::Signal(plain(signal)),
            (None, None) => Exit::Absent,
        },
        _ => Exit::Absent,
    };
    match output.get("exit") {
        // The confined record: the status is one level down, beside the execution's id and state.
        Some(Value::Object(record))
            if record.contains_key("state") || record.contains_key("exit") =>
        {
            match record.get("exit") {
                Some(status) if !status.is_null() => read(status),
                _ => match record.get("state").and_then(Value::as_str) {
                    Some(state) if state != "exited" => Exit::Unfinished(state.to_owned()),
                    _ => Exit::Absent,
                },
            }
        }
        Some(exit) => read(exit),
        None => Exit::Absent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Design 0003 § 6's outcome rule for a command step, case by case: only a clean exit `0` is a
    /// passed step, and each of the other ways a call ends says which one it was.
    #[test]
    fn a_command_step_passes_on_exit_zero_and_names_every_other_ending() {
        let ran = |exit: Value, timed_out: bool| {
            ToolOutcome::ok(serde_json::json!({
                "argv": ["cargo", "test"],
                "exit": exit,
                "stdout": "",
                "stderr": "",
                "timed_out": timed_out,
            }))
        };
        assert_eq!(command_outcome(&ran(serde_json::json!(0), false)), Ok(()));
        assert_eq!(
            command_outcome(&ran(serde_json::json!(101), false)),
            Err("exit 101".to_owned())
        );
        assert_eq!(
            command_outcome(&ran(Value::Null, false)),
            Err("it ended without an exit code".to_owned())
        );
        assert_eq!(
            command_outcome(&ran(Value::Null, true)),
            Err("it timed out".to_owned())
        );
        assert_eq!(
            command_outcome(&ToolOutcome::failed("`run` was not approved: the ceiling")),
            Err("it did not run: `run` was not approved: the ceiling".to_owned())
        );
    }

    /// The confined port answers with substrate's execution record under `exit`, and the status
    /// is one level down. The sixth paid native walk read a validator that exited `0` in the
    /// sandbox as *no exit code*; this is the shape it saw, byte for byte where it matters.
    #[test]
    fn a_command_step_reads_its_exit_from_the_confined_record_as_well_as_the_local_one() {
        let confined = |code: Value, signal: Value| {
            ToolOutcome::ok(serde_json::json!({
                "stdout": "1 file(s) in .engineering/planning: 1 artifact(s)\nvalid\n",
                "stderr": "",
                "stdout_truncated": false,
                "stderr_truncated": false,
                "output_complete": true,
                "exit": {
                    "id": "ex_85092_16",
                    "kind": "exec",
                    "state": "exited",
                    "workspace": "ws_project",
                    "exit": {"code": code, "signal": signal},
                },
                "slice": {"stream": "stdout", "eof": true},
            }))
        };
        assert_eq!(
            command_outcome(&confined(serde_json::json!(0), Value::Null)),
            Ok(())
        );
        assert_eq!(
            command_outcome(&confined(serde_json::json!(2), Value::Null)),
            Err("exit 2".to_owned())
        );
        assert_eq!(
            command_outcome(&confined(Value::Null, serde_json::json!("KILL"))),
            Err("killed by signal KILL".to_owned())
        );
        assert_eq!(
            command_outcome(&confined(Value::Null, Value::Null)),
            Err("it ended without an exit code".to_owned())
        );
        // A record with no status is not read as a success, and one whose execution never exited
        // says which state it is in.
        assert_eq!(
            command_outcome(&ToolOutcome::ok(
                serde_json::json!({"exit": {"id": "ex_1"}})
            )),
            Err("it ended without an exit code".to_owned())
        );
        assert_eq!(
            command_outcome(&ToolOutcome::ok(serde_json::json!({
                "exit": {"id": "ex_1", "state": "expired", "exit": null}
            }))),
            Err("it did not exit (state: expired)".to_owned())
        );
    }
}
