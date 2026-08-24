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
//! `session.started` carries `adapter: b10x`, `adapter_class: direct_provider` — **not** `harness`.
//! A harness adapter drives somebody else's loop; this is one, and metaharness's own vocabulary
//! already had the word: *"the embedder holds the conversation and calls a model API"*. A reader
//! who filters on `adapter_class` must be able to see the difference, and the field exists to carry
//! exactly that (metaharness design § 8.4 O5, *a harness adapter never silently becomes a direct
//! API call*).
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
/// Not `harness`, and the word is metaharness's own.
///
/// `AdapterClass::DirectProvider` — *"the embedder holds the conversation and calls a model API"* —
/// has been in that protocol since v0.1 with a note saying nothing was one yet. This is, so it
/// takes the existing word rather than coining a second one for the same thing.
const ADAPTER_CLASS: &str = "direct_provider";

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
    // Summed as the record is read, so `session.ended` can state what the run cost. Stays `None`
    // when no turn was priced, and `None` becomes `null` rather than `0` downstream.
    let mut spent_micro_usd: Option<u64> = None;
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
        let kind = value
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mapped = match kind {
            "started" => started(&value, version),
            "text-delta" => json!({"event": "text", "text": value["text"]}),
            "tool-requested" => json!({
                "event": "tool.requested",
                "call_id": value["call_id"],
                "name": value["name"],
                "input": value["arguments"],
                // What the call *is*, in the neutral vocabulary a consumer selects on. Without it
                // this stream said `tool_invoke` for every act in the run and buried the entry in
                // the input — so a reader could not tell a write from a read, and one written for
                // another harness could not read this arm at all.
                "operations": operations(&value),
                // *Which* file or program, in the same neutral form. A path-scoped expectation
                // reads `input.file_path` on a vendor's record and finds nothing here: the entry's
                // arguments are nested one level down, under a name this loop chose. So the answer
                // is stated rather than left to be dug for.
                "subjects": subjects(&value),
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
            // Folded into the run's total rather than mapped onto the `usage` line it follows: the
            // per-turn figure would have to reach an event already written, and buffering the
            // stream to backfill one field is a worse trade than carrying this line as it stands.
            // The number a reader compares arms on is the run total, and that is stated below.
            "cost" => {
                if let Some(micro) = value["micro_usd"].as_u64() {
                    spent_micro_usd = Some(spent_micro_usd.unwrap_or(0).saturating_add(micro));
                }
                // Read and used; the run's total goes out on `session.ended`. See the control-plane
                // arm below for why emitting nothing is not a drop.
                continue;
            }
            "finished" => {
                ended = true;
                finished(&value, spent_micro_usd)
            }
            // **Control plane, not opaque.** `opaque` means *this build could not read it*, and a
            // consumer reads it that way: an unread event could have been the tool call an
            // expectation was looking for, so every count over the run goes `unk`. Sending this
            // loop's own bookkeeping down that road put 130 opaque events in a twelve-call run and
            // turned seven of eleven corpus rows undecidable — about a stream that had been read
            // perfectly.
            //
            // A turn boundary and a warning are metaharness's own control-plane events: understood,
            // projecting into no `trace-ir/1` family, and not uncertain.
            "turn-started" => json!({"event": "turn.started", "turn": value["turn"]}),
            "warning" => json!({
                "event": "warning",
                "code": value["code"],
                "message": value["message"],
            }),
            // Read, understood, and modelled by no `trace-ir/1` family. Emitting nothing here is
            // not the drop D4 forbids: D4 protects an event nobody could read, and these were read.
            "tool-arguments-delta" | "approval-required" | "rates" => continue,
            // A kind this build does not know, which is what `opaque` is for.
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
        // **What the run could do**, beside what the model was offered. Behind three verbs the
        // second is the same on every run, so a control asking *was there a writer to refuse*
        // read a list of three verbs and answered that there was none - which reads as a defect
        // and is a vocabulary mismatch.
        "available_operations": value.get("operations").cloned().unwrap_or(Value::Null),
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

fn finished(value: &Value, spent_micro_usd: Option<u64>) -> Value {
    let stop = value.get("stop").cloned().unwrap_or(Value::Null);
    let kind = stop
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    json!({
        "event": "session.ended",
        "is_error": kind != "completed",
        "subtype": kind,
        "stop_reason": Value::Null,
        "terminal_reason": if kind == "completed" { "completed" } else { kind },
        "api_error_status": Value::Null,
        // The loop's own count, beside the stop rather than inside it: only the two bound-bound
        // stops carry one, so a reader asking how long a run was got an answer from a run that hit
        // a ceiling and `null` from one that finished.
        "num_turns": value
            .get("turns")
            .filter(|turns| !turns.is_null())
            .or_else(|| stop.get("turns"))
            // The stop's own count is the fallback, for a record written before the loop reported
            // one beside it: a bound-bound stop has always carried the figure, and losing it on a
            // replay of an older capture would be this converter taking a fact away.
            .cloned()
            .unwrap_or(Value::Null),
        "duration_ms": Value::Null,
        "duration_api_ms": Value::Null,
        "ttft_ms": Value::Null,
        "time_to_request_ms": Value::Null,
        // What the run cost at the rates the operator declared with `--prices`.
        //
        // `null` where no card priced the run — which is the honest answer, and the same one this
        // field carried unconditionally until rate cards existed. A zero would be a lie about a run
        // that cost money. The figure is this harness's own, exactly as Claude Code's
        // `total_cost_usd` is Claude Code's: neither provider returns a price, and both state one
        // anyway, because a subscription is not a reason for a run to be uncosted.
        "total_cost_usd": spent_micro_usd.map_or(Value::Null, dollars),
        "permission_denials": [],
        "subagents_spawned": 0,
        "usage": Value::Null,
        "model_usage": Value::Null,
    })
}

/// Millionths of a dollar as the JSON number this field is declared to hold.
///
/// Built from the decimal rather than by dividing into a float, so the number in this stream and
/// the number in the loop record are the same figure — not two that agree to within a rounding
/// nobody wrote down.
fn dollars(micro_usd: u64) -> Value {
    serde_json::from_str(&harness_loop::micro_usd_as_decimal(micro_usd)).unwrap_or(Value::Null)
}

/// The neutral operations one `tool-requested` resolves to.
///
/// This loop publishes three verbs and nothing else, so the tool name is never the answer:
/// `tool_invoke` is every act in the run, and which one is inside its arguments. Resolved here
/// rather than by the consumer, because the mapping is a fact about the catalogue and a consumer
/// that kept its own copy is a copy that drifts.
///
/// Empty for `tool_search` and `tool_describe` — they are questions about the catalogue, not acts —
/// and for an entry outside the vocabulary, which reached no tool.
fn operations(value: &Value) -> Vec<&'static str> {
    if value["name"].as_str() != Some(harness_tools::INVOKE_VERB) {
        return Vec::new();
    }
    value["arguments"]["name"]
        .as_str()
        .and_then(harness_tools::operation_of)
        .into_iter()
        .collect()
}

/// The concrete things one `tool-requested` would touch, as `file:…` / `proc:…`.
///
/// Read from the catalogue's own rule rather than from the arguments directly, so this stream and
/// a live gate answer the same question the same way.
fn subjects(value: &Value) -> Vec<String> {
    if value["name"].as_str() != Some(harness_tools::INVOKE_VERB) {
        return Vec::new();
    }
    let Some(entry) = value["arguments"]["name"].as_str() else {
        return Vec::new();
    };
    harness_tools::subjects_of(entry, &value["arguments"]["arguments"])
        .into_iter()
        .map(|subject| subject.as_str().to_owned())
        .collect()
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
struct Digest;

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

    const RUN: &str = r#"{"kind":"started","model":"gpt-5.6-sol","published_tools":["tool_search","tool_describe","tool_invoke"]}
{"kind":"turn-started","turn":1}
{"kind":"tool-requested","call_id":"c-1","name":"tool_invoke","arguments":{"name":"file_read","arguments":{"path":"README.md"}}}
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
                // A turn boundary, as what it is. It crossed as `opaque` until a live corpus run
                // showed what that costs: an unread event could have been any tool call, so seven
                // of eleven rows went `unk` about a stream that had been read perfectly.
                "turn.started",
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
    fn this_names_the_class_metaharness_already_had_rather_than_a_synonym() {
        let events = convert_all(RUN);
        let started = &events[0];
        assert_eq!(started["adapter"], "b10x");
        assert_eq!(
            started["adapter_class"], "direct_provider",
            "metaharness's own word for this, rather than a synonym invented here"
        );
        assert_eq!(started["harness_version"], "0.1.0");
        assert_eq!(started["model"], "gpt-5.6-sol");
        assert_eq!(
            started["offered_tools"],
            json!(["tool_search", "tool_describe", "tool_invoke"]),
            "what the *model* was offered, which on this loop is three verbs whatever the \
             catalogue behind them holds"
        );
    }

    const PRICED: &str = r#"{"kind":"started","model":"m","published_tools":["tool_search"]}
{"kind":"rates","source":"a table the operator read","as_of":"2026-08-24"}
{"kind":"usage","model":"m","input_tokens":100,"output_tokens":10,"cached_input_tokens":0}
{"kind":"cost","model":"m","micro_usd":106000}
{"kind":"usage","model":"m","input_tokens":50,"output_tokens":5,"cached_input_tokens":40}
{"kind":"cost","model":"m","micro_usd":233}
{"kind":"finished","stop":{"kind":"completed"}}"#;

    #[test]
    fn a_call_names_which_file_it_touched_in_a_form_a_reader_can_select_on() {
        // `RUN` invokes `file_read` on README.md. A path-scoped expectation reading `input.file_path`
        // finds nothing on this wire - the entry's arguments are nested a level down under names
        // this loop chose - so the record answers the question instead.
        let events = convert_all(RUN);
        let call = events
            .iter()
            .find(|event| event["event"] == "tool.requested")
            .expect("a call");
        assert_eq!(call["subjects"], json!(["file:README.md"]));
        assert_eq!(call["operations"], json!(["file.read"]));
    }

    #[test]
    fn a_catalogue_question_touches_nothing_and_says_so() {
        // `tool_search` and `tool_describe` are questions about the list, not acts. A subject here
        // would name a file nobody read.
        let line = r#"{"kind":"tool-requested","call_id":"c-9","name":"tool_search","arguments":{}}"#;
        let events = convert_all(line);
        assert_eq!(events[0]["subjects"], json!([]));
    }

    #[test]
    fn a_priced_run_states_its_cost_where_every_other_arm_states_theirs() {
        // The number the matrix compares arms on. Nothing on this wire returns a price - and
        // nothing returns one for Claude Code either, which states `total_cost_usd` all the same.
        let events = convert_all(PRICED);
        let ended = events.last().expect("a terminal event");
        assert_eq!(ended["event"], "session.ended");
        assert_eq!(
            ended["total_cost_usd"],
            json!(0.106_233),
            "the turns below it sum to exactly this"
        );
    }

    #[test]
    fn an_unpriced_run_reports_no_cost_rather_than_a_zero() {
        // `RUN` carries no rate card. A zero here would say the run was free, which is a claim
        // about somebody's invoice that nobody made.
        let events = convert_all(RUN);
        assert_eq!(
            events.last().expect("terminal")["total_cost_usd"],
            Value::Null
        );
    }

    #[test]
    fn a_line_this_build_understands_never_crosses_as_something_it_could_not_read() {
        // `opaque` is a claim: *this build could not read that*. A consumer acts on it - an unread
        // event could have been the tool call an expectation was looking for - so a run's own
        // bookkeeping sent down that road turns counts `unk` for no reason. The rate card and the
        // per-turn costs were read; the total below proves it, and the raw lines are still in the
        // record metaharness retains.
        let events = convert_all(PRICED);
        assert!(
            !events.iter().any(|event| event["event"] == "opaque"),
            "nothing here was unreadable: {events:?}"
        );
        assert_eq!(
            events.last().expect("terminal")["total_cost_usd"],
            json!(0.106_233),
            "and the figure those lines carried still arrives"
        );
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
        assert_eq!(
            requested["name"], "tool_invoke",
            "the verb the model called"
        );
        assert_eq!(
            requested["operations"],
            json!(["file.read"]),
            "and what it was"
        );
        assert_eq!(requested["input"]["arguments"]["path"], "README.md");
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

#[cfg(test)]
mod operation_tests {
    use super::*;
    use serde_json::json;

    /// The stream says what each call **was**, not only which verb carried it.
    ///
    /// Every act in this loop travels through `tool_invoke`, so a consumer reading tool names saw
    /// one name for reads, writes and processes alike — and a consumer written against another
    /// harness saw a name it had never heard of. Both are the same blindness, and this is the
    /// field that ends it: `file.write` reads the same here, on Claude Code's wire, and on codex's.
    #[test]
    fn every_act_carries_the_neutral_operation_and_a_catalogue_question_carries_none() {
        for (entry, expected) in [
            ("file_read", vec!["file.read"]),
            ("file_write", vec!["file.write"]),
            ("file_edit", vec!["file.edit"]),
            ("dir_list", vec!["dir.list"]),
            ("search", vec!["search"]),
            ("run", vec!["shell"]),
        ] {
            assert_eq!(
                operations(&json!({
                    "name": harness_tools::INVOKE_VERB,
                    "arguments": {"name": entry, "arguments": {}}
                })),
                expected,
                "{entry}"
            );
        }

        for verb in [harness_tools::SEARCH_VERB, harness_tools::DESCRIBE_VERB] {
            assert!(
                operations(&json!({"name": verb, "arguments": {}})).is_empty(),
                "{verb} asks what the run may do; it does none of it"
            );
        }

        assert!(
            operations(&json!({
                "name": harness_tools::INVOKE_VERB,
                "arguments": {"name": "Bash", "arguments": {}}
            }))
            .is_empty(),
            "a name outside the vocabulary reached no tool, so it is no operation"
        );
    }

    /// The field reaches the converted stream, which is the thing a judge actually reads.
    #[test]
    fn the_converted_stream_carries_it_where_a_consumer_will_look() {
        let line = r#"{"kind":"tool-requested","call_id":"c-1","name":"tool_invoke","arguments":{"name":"run","arguments":{"argv":["/usr/bin/python3","test.py"]}}}"#;
        let mut out = Vec::new();
        convert(&mut std::io::Cursor::new(line), &mut out, "0.1.0").expect("converts");
        let event: Value =
            serde_json::from_str(String::from_utf8(out).expect("utf-8").trim()).expect("JSON");
        assert_eq!(event["event"], "tool.requested");
        assert_eq!(event["operations"], json!(["shell"]));
        assert_eq!(
            event["name"], "tool_invoke",
            "the vendor's name stays too: the neutral one is an addition, not a replacement"
        );
    }
}
