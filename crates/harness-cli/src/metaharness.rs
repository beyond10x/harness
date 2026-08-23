//! This loop's own record, written as `metaharness.event/1`.
//!
//! # Why a converter and not a metaharness adapter
//!
//! Every other harness reaches the evaluation matrix through a metaharness adapter: metaharness
//! spawns a vendor binary, reads its record, and decides each tool call at a seam. That is exactly
//! what arm `driven` measures — a policy imposed on somebody else's loop from outside.
//!
//! Arm `native` measures the opposite claim: that the **published toolset is the policy**, because
//! the loop is ours and a tool outside the surface does not exist rather than being refused. Wrapping
//! this loop in a seam that decides its calls would put the driven arm's treatment back on top of it
//! and measure that instead. The two arms would differ in name only.
//!
//! So what crosses is the record, not the control. This writes the same
//! `metaharness.event/1` stream every other arm is judged from, and the judge cannot tell — which
//! is the point, because a matrix whose cells were produced by different instruments compares
//! instruments.
//!
//! # What is stated, and what is honestly absent
//!
//! `session.started` carries `adapter: b10x`, `adapter_class: loop` — **not** `harness`. A harness
//! adapter drives somebody else's loop; this is one. A reader who filters on `adapter_class` must
//! be able to see the difference, and the field exists to carry exactly that (metaharness design
//! § 8.4 O5, *a harness adapter never silently becomes a direct API call*).
//!
//! Several fields other adapters populate are written **`null`**, because this loop has no such
//! thing and inventing one would be a claim about a run nobody made: no `slash_commands`, no
//! `skills`, no `agents`, no `mcp_servers`, no `permission_mode`. `hermetic.installed_plugins` is
//! `[]` and that is a fact rather than an absence: this loop installs no plugins because there is
//! no plugin mechanism for one to be installed into.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

/// The wire every arm of the evaluation is judged from.
const FORMAT: &str = "metaharness.event/1";
/// What this adapter calls itself.
const ADAPTER: &str = "b10x";
/// Not `harness`: this crate holds the loop rather than driving somebody else's.
const ADAPTER_CLASS: &str = "loop";

/// Reads a `--json` loop record and writes the metaharness stream for it.
///
/// # Errors
///
/// Returns an error when a line cannot be read or written. A line that is not a loop event this
/// build knows is **carried across as `opaque`**, never dropped: the failure that costs most is a
/// checker reporting *the tool was never called* when what happened is that it stopped being able
/// to see tool calls.
pub fn convert(
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    version: &str,
) -> std::io::Result<usize> {
    let mut sequence = 0_u64;
    let mut written = 0;
    let mut ended = false;
    let mut emit = |output: &mut dyn Write, mut event: Value| -> std::io::Result<()> {
        sequence += 1;
        if let Some(object) = event.as_object_mut() {
            object.insert("format".to_owned(), json!(FORMAT));
            object.insert("seq".to_owned(), json!(sequence));
        }
        writeln!(output, "{event}")
    };

    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            emit(output, opaque(&line))?;
            written += 1;
            continue;
        };
        let kind = value.get("kind").and_then(Value::as_str).unwrap_or_default();
        let mapped = match kind {
            "started" => started(&value, version),
            "text-delta" => json!({"event": "text", "text": value["text"]}),
            "tool-requested" => json!({
                "event": "tool.requested",
                "call_id": value["call_id"],
                "name": value["name"],
                "input": value["arguments"],
                // Nothing decided this call at a seam, because nothing was in a position to: the
                // toolset it was drawn from is the policy. `false` here is a fact about the arm.
                "decision_required": false,
            }),
            "tool-completed" => json!({
                "event": "tool.result",
                "call_id": value["call_id"],
                "is_error": value["failed"],
            }),
            "approval-resolved" => json!({
                "event": "tool.decided",
                "call_id": value["call_id"],
                "decision": if value["approved"].as_bool() == Some(true) { "allow" } else { "deny" },
            }),
            "usage" => json!({
                "event": "usage",
                "model": value["model"],
                "usage": {
                    "input_tokens": value["input_tokens"],
                    "output_tokens": value["output_tokens"],
                    "cache_read_input_tokens": value["cached_input_tokens"],
                    "cache_creation_input_tokens": Value::Null,
                    "service_tier": Value::Null,
                    "thinking_tokens": Value::Null,
                    "iterations": Value::Null,
                    "speed": Value::Null,
                    "cost_usd": Value::Null,
                },
            }),
            "finished" => {
                ended = true;
                finished(&value)
            }
            // `turn-started`, `tool-arguments-delta`, `approval-required` and `warning` have no
            // counterpart that any expectation reads. Carried as opaque rather than dropped, for
            // the reason in this function's own documentation.
            _ => opaque(&line),
        };
        emit(output, mapped)?;
        written += 1;
    }

    if !ended {
        // A record that stops mid-run has no terminal event, and one must not be invented: a
        // checker reading a synthesised `completed` would call a killed run a finished one. The
        // stream simply ends, and every expectation that reads a terminal record answers `unk`.
        return Ok(written);
    }
    Ok(written)
}

fn started(value: &Value, version: &str) -> Value {
    json!({
        "event": "session.started",
        "adapter": ADAPTER,
        "adapter_class": ADAPTER_CLASS,
        "harness_version": version,
        "session_id": Value::Null,
        "model": value["model"],
        // Absent because this loop has none of them, not because nobody looked.
        "permission_mode": Value::Null,
        "credential_source": "named",
        "output_style": Value::Null,
        "cwd": Value::Null,
        "offered_tools": value["published_tools"],
        "slash_commands": Value::Null,
        "skills": Value::Null,
        "agents": Value::Null,
        "plugins": Value::Null,
        "mcp_servers": Value::Null,
        "inputs_digest": Value::Null,
        "transcript": {"path": Value::Null, "digest": Value::Null},
        "hermetic": {
            // A fact, not an absence: this loop installs no plugins because it has no mechanism
            // for one. An arm that attested a plugin here would be a different arm.
            "installed_plugins": [],
        },
    })
}

fn finished(value: &Value) -> Value {
    let stop = value.get("stop").cloned().unwrap_or(Value::Null);
    let kind = stop.get("kind").and_then(Value::as_str).unwrap_or("unknown");
    json!({
        "event": "session.ended",
        "is_error": kind != "completed",
        "subtype": kind,
        "stop_reason": Value::Null,
        "terminal_reason": if kind == "completed" { "completed" } else { kind },
        "api_error_status": Value::Null,
        "num_turns": stop.get("turns").cloned().unwrap_or(Value::Null),
        "duration_ms": Value::Null,
        "duration_api_ms": Value::Null,
        "ttft_ms": Value::Null,
        "time_to_request_ms": Value::Null,
        // The gateway relays bytes and reports no price, so this loop has never had a cost to
        // state. `null` is the honest answer and a zero would be a lie about a run that cost money.
        "total_cost_usd": Value::Null,
        "permission_denials": [],
        "subagents_spawned": 0,
        "usage": Value::Null,
        "model_usage": Value::Null,
    })
}

fn opaque(line: &str) -> Value {
    json!({
        "event": "opaque",
        "vendor_type": "b10x",
        "vendor_subtype": serde_json::from_str::<Value>(line)
            .ok()
            .and_then(|value| value.get("kind").and_then(Value::as_str).map(ToOwned::to_owned)),
        "digest": format!("{:x}", Digest::of(line)),
    })
}

/// A stable, dependency-free digest of one line.
///
/// FNV-1a rather than SHA-256: what an opaque record needs is *these two lines are the same line*,
/// and nothing here is a security claim. Taking `sha2` for it would be a dependency to say
/// something this does not say.
struct Digest(u64);

impl Digest {
    fn of(line: &str) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for byte in line.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x1000_0000_01b3);
        }
        hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn convert_all(lines: &str) -> Vec<Value> {
        let mut out = Vec::new();
        convert(&mut Cursor::new(lines), &mut out, "0.1.0").expect("converts");
        String::from_utf8(out)
            .expect("utf-8")
            .lines()
            .map(|line| serde_json::from_str(line).expect("a JSON line"))
            .collect()
    }

    const RUN: &str = r#"{"kind":"started","model":"gpt-5.6-sol","published_tools":["workspace_read"]}
{"kind":"turn-started","turn":1}
{"kind":"tool-requested","call_id":"c-1","name":"workspace_read","arguments":{"path":"README.md"}}
{"kind":"tool-completed","call_id":"c-1","failed":false}
{"kind":"usage","model":"gpt-5.6-sol","input_tokens":297,"output_tokens":25,"cached_input_tokens":0}
{"kind":"finished","stop":{"kind":"completed"}}"#;

    #[test]
    fn a_loop_record_becomes_the_stream_every_other_arm_is_judged_from() {
        let events = convert_all(RUN);
        assert!(
            events.iter().all(|event| event["format"] == FORMAT),
            "the tag is on every line, because a truncated capture must stay self-describing"
        );
        let kinds: Vec<&str> = events
            .iter()
            .map(|event| event["event"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            kinds,
            vec![
                "session.started",
                "opaque",
                "tool.requested",
                "tool.result",
                "usage",
                "session.ended"
            ]
        );
        // Sequence numbers are the reader's only ordering, so they are dense and start at one.
        let seqs: Vec<u64> = events
            .iter()
            .map(|event| event["seq"].as_u64().unwrap_or_default())
            .collect();
        assert_eq!(seqs, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn this_is_a_loop_and_says_so_rather_than_calling_itself_a_harness() {
        let events = convert_all(RUN);
        let started = &events[0];
        assert_eq!(started["adapter"], "b10x");
        assert_eq!(
            started["adapter_class"], "loop",
            "a harness adapter drives somebody else's loop; this is one"
        );
        assert_eq!(started["harness_version"], "0.1.0");
        assert_eq!(started["model"], "gpt-5.6-sol");
        assert_eq!(started["offered_tools"], json!(["workspace_read"]));
    }

    #[test]
    fn an_arm_with_no_plugin_mechanism_attests_an_empty_list_rather_than_nothing() {
        // A fact, not an absence. The evaluation's manifest reader refuses a `plugin` arm whose
        // stream attests nothing and a `raw` arm whose stream attests something, so this field is
        // read - and `[]` is what says *this loop installs no plugins because it cannot*.
        let events = convert_all(RUN);
        assert_eq!(events[0]["hermetic"]["installed_plugins"], json!([]));
    }

    #[test]
    fn nothing_decided_the_call_at_a_seam_and_the_record_says_so() {
        // The whole difference between this arm and `driven`, in one field.
        let events = convert_all(RUN);
        let requested = events
            .iter()
            .find(|event| event["event"] == "tool.requested")
            .expect("there is one");
        assert_eq!(requested["decision_required"], json!(false));
        assert_eq!(requested["name"], "workspace_read");
        assert_eq!(requested["input"]["path"], "README.md");
    }

    #[test]
    fn a_line_this_build_does_not_map_is_carried_across_and_never_dropped() {
        // The failure that costs most is a checker reporting *the tool was never called* when what
        // happened is that it stopped being able to see tool calls.
        let events = convert_all(
            "{\"kind\":\"invented-later\",\"detail\":1}\n{\"kind\":\"finished\",\"stop\":{\"kind\":\"completed\"}}",
        );
        assert_eq!(events[0]["event"], "opaque");
        assert_eq!(events[0]["vendor_subtype"], "invented-later");
        assert!(events[0]["digest"].as_str().is_some_and(|d| !d.is_empty()));

        // Even a line that is not JSON at all.
        let events = convert_all("this is not json\n");
        assert_eq!(events[0]["event"], "opaque");
        assert_eq!(events[0]["vendor_subtype"], Value::Null);
    }

    #[test]
    fn a_run_that_did_not_finish_gets_no_invented_terminal_record() {
        // A checker reading a synthesised `completed` would call a killed run a finished one.
        let events = convert_all(
            "{\"kind\":\"started\",\"model\":\"m\",\"published_tools\":[]}\n{\"kind\":\"turn-started\",\"turn\":1}",
        );
        assert!(
            !events.iter().any(|event| event["event"] == "session.ended"),
            "the stream simply ends"
        );
    }

    #[test]
    fn a_stop_that_is_not_completion_is_an_error_and_keeps_its_own_word() {
        let events = convert_all(
            "{\"kind\":\"finished\",\"stop\":{\"kind\":\"budget-exhausted\",\"turns\":9}}",
        );
        let ended = &events[0];
        assert_eq!(ended["is_error"], json!(true));
        assert_eq!(ended["terminal_reason"], "budget-exhausted");
        assert_eq!(ended["num_turns"], json!(9));
    }

    #[test]
    fn a_cost_this_loop_never_had_is_null_and_never_zero() {
        // The gateway relays bytes and reports no price. A zero would be a lie about a run that
        // cost money, and the matrix reports every resource total over the runs that stated one.
        let events = convert_all(RUN);
        let ended = events.last().expect("terminal");
        assert_eq!(ended["total_cost_usd"], Value::Null);
    }
}
