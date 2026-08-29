//! Turning the loop's event stream into something a person, or a program, can read.

use std::io::Write;

use harness_flow::{FlowEvent, FlowSink, Moment};
use harness_loop::{LoopEvent, LoopSink};

/// Writes the run as it happens.
///
/// Text goes to stdout unbuffered so a person sees the answer forming; everything else goes to
/// stderr, which keeps stdout usable as the answer itself when the output is piped.
///
/// # Under a schema, stdout is one line
///
/// A run started with `--output-schema` was started by something that will read its stdout as
/// JSON. So in that mode the prose deltas join the progress on stderr and **the only thing written
/// to stdout is the `answered` value**, compact, on one line — a run that ended in prose writes
/// nothing there at all and exits 2. Any other arrangement makes `… | jq` work on the good days
/// and read prose as an answer on the bad ones.
///
/// # An answer is written once, at the end, and only if it survived
///
/// `Answered` is not the end of a run. A `stop` hook may **withdraw** it — the loop clears the
/// structured answer and turns again — and the next turn may answer differently, or not at all.
/// Writing each `Answered` as it arrives put the withdrawn value on stdout and then exited 2
/// `unstructured`, and a block-then-answer-again put *two* JSON lines there. So under a schema the
/// value is held, the latest one wins, and it is written at `Finished` only when the run actually
/// completed. Every other stop leaves stdout empty, which is what the exit status already says.
// Four switches over two streams, and each is one question a caller already asked on the command
// line — a state machine over them would be a fifth thing to keep true.
#[allow(clippy::struct_excessive_bools)]
pub struct Renderer<O: Write, E: Write> {
    out: O,
    err: E,
    json: bool,
    quiet: bool,
    /// Whether stdout belongs to the structured answer alone.
    structured: bool,
    /// Whether an answer that stood is written to stdout at all.
    ///
    /// A step of a workflow runs under a schema like any other structured run, and its answer is
    /// the **walk's** to read: stdout there is the flow's own record, and one JSON line per step
    /// beside it would be eight answers to a question nobody asked.
    answers: bool,
    /// The latest `Answered` value under a schema, until `Finished` says whether it stood.
    answer: Option<serde_json::Value>,
    /// The words a `transition-refused` carried, until the retreat or the exit it explains.
    ///
    /// `group-repeating` says which attempt failed and not why, because the notation evaluates no
    /// gate and has nothing to say about one. Here the two lines are next to each other on a
    /// terminal, so the reason is carried across.
    refusal: Option<String>,
}

impl<O: Write, E: Write> Renderer<O, E> {
    pub fn new(out: O, err: E, json: bool, quiet: bool) -> Self {
        Self {
            out,
            err,
            json,
            quiet,
            structured: false,
            answers: true,
            answer: None,
            refusal: None,
        }
    }

    /// Keeps stdout for the answer: prose goes to stderr and only `answered` is printed.
    #[must_use]
    pub fn structured(mut self, structured: bool) -> Self {
        self.structured = structured;
        self
    }

    /// Keeps stdout for the walk: prose is progress and no answer line is ever written.
    ///
    /// Every step of a workflow runs under a schema, so the prose belongs on stderr exactly as it
    /// does under `--output-schema`. What differs is the other half: the answer is read by the
    /// walk, which turns it into `passed` or `failed`, and stdout carries the flow's record —
    /// under `--json` the events, and under prose nothing at all.
    #[must_use]
    pub fn within_a_flow(mut self) -> Self {
        self.structured = true;
        self.answers = false;
        self
    }

    fn note(&mut self, line: &str) {
        if self.quiet {
            return;
        }
        let _ = writeln!(self.err, "{line}");
    }

    /// The model's own words: the answer on stdout, or progress on stderr under a schema.
    ///
    /// Under `--output-schema` the prose is not the answer — something is going to read stdout as
    /// JSON — so it joins the rest of the progress and leaves that stream to the one line below.
    fn prose(&mut self, text: &str) {
        if !self.structured {
            let _ = write!(self.out, "{text}");
            let _ = self.out.flush();
            return;
        }
        if !self.quiet {
            let _ = write!(self.err, "{text}");
            let _ = self.err.flush();
        }
    }

    /// The structured answer, and the only thing on stdout under a schema: one compact line, so
    /// the run composes with anything that reads JSON.
    ///
    /// Called once, from `Finished`, and only for a run that completed — see the type's own note.
    fn answer(&mut self) {
        let Some(value) = self.answer.take() else {
            return;
        };
        let _ = writeln!(
            self.out,
            "{}",
            serde_json::to_string(&value).unwrap_or_default()
        );
        let _ = self.out.flush();
    }

    /// What this run asked for and its machine would not admit, one line each.
    ///
    /// Straight to stderr, past `--quiet`, for the same reason a warning goes past it: this is not
    /// progress, it is a fact about what the run can do, and the run that needed it most was an
    /// unattended one. A person who read the tool list and not this would take a shorter list for
    /// the toolset they asked for — which is exactly what happened.
    ///
    /// Nothing at all for a run that was refused nothing: absence stays absence.
    /// The opening line: what this run is, and what it was refused.
    ///
    /// Split out of [`Self::emit`] because that match is the whole renderer and one arm growing a
    /// field should not be what pushes it over a lint. Takes the event by reference and matches
    /// again rather than taking the fields, so a field added to `Started` later is a compile error
    /// here and not a silent omission from what a person watching a run is shown.
    fn started(&mut self, event: &LoopEvent) {
        let LoopEvent::Started {
            model,
            published_tools,
            operations,
            withheld,
            skills,
            // Not rendered: this loop publishes none yet, and a `agents: []` line every run would
            // be noise. It is in the record, which is where a comparison reads it.
            agents: _,
            profiles,
            credential_source: _,
        } = event
        else {
            return;
        };
        let names: Vec<&str> = published_tools
            .iter()
            .map(harness_wire::ToolName::as_str)
            .collect();
        self.note(&format!("model {model} · tools: {}", names.join(", ")));
        if !operations.is_empty() {
            self.note(&format!("  can: {}", operations.join(", ")));
        }
        // **Shown, because a run given skills and a run given none are different experiments.**
        // The bodies are not here — the model loads those — but which library it had is the sort
        // of thing a person comparing two runs afterwards has no other way to see.
        if !skills.is_empty() {
            self.note(&format!("  skills: {}", skills.join(", ")));
        }
        // **Shown, because a run configured by a file and one configured by flags are different
        // runs and only one of them is reproducible from the command line you can see.**
        if !profiles.is_empty() {
            self.note(&format!(
                "  profiles: {}",
                profiles
                    .iter()
                    .map(|used| used.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        self.withheld(withheld);
    }

    fn withheld(&mut self, withheld: &[harness_loop::Withheld]) {
        for withheld in withheld {
            let _ = writeln!(
                self.err,
                "{}",
                withheld_line(&withheld.tool, &withheld.reason)
            );
        }
    }

    /// One line of a delegate's own run, indented, so a reader can see who is acting.
    ///
    /// The child's **text** is not rendered: it is its report to the parent, not this run's
    /// answer, and printing it would put a second answer on the same stream. The parent reads it
    /// as a tool result and answers for itself.
    fn delegated(&mut self, event: &LoopEvent) {
        match event {
            // A warning is reported even when quiet, indented like the rest of the child's run.
            LoopEvent::Warning { code, message } => {
                let _ = writeln!(self.err, "  warning [{code}] {message}");
            }
            LoopEvent::ToolRequested(call) => {
                let line = requested(call);
                self.note(&format!("  {line}"));
            }
            LoopEvent::ApprovalRequired { name, .. } => {
                self.note(&format!("  ? {name} needs a decision"));
            }
            LoopEvent::ToolCompleted { failed, .. } => {
                self.note(if *failed { "  ← failed" } else { "  ← ok" });
            }
            _ => {}
        }
    }
}

impl<O: Write, E: Write> LoopSink for Renderer<O, E> {
    fn emit(&mut self, event: LoopEvent) {
        if self.json {
            if let Ok(line) = serde_json::to_string(&event) {
                let _ = writeln!(self.out, "{line}");
                let _ = self.out.flush();
            }
            return;
        }
        match event {
            LoopEvent::Started { .. } => self.started(&event),
            LoopEvent::TurnStarted { turn } => self.note(&format!("· turn {turn}")),
            // Whatever streamed for the turn is void, and the person who read it has to be told:
            // a stdout that cannot be un-printed gets a marker line instead.
            LoopEvent::TurnRetried {
                turn,
                attempt,
                reason,
            } => {
                // Under a schema stdout is the answer and nothing else, and a bare newline before
                // it is exactly the byte a reader piping to `jq` cannot survive. The marker is for
                // prose that was already printed; there is none in that mode.
                if !self.structured {
                    let _ = writeln!(self.out);
                    let _ = self.out.flush();
                }
                let _ = writeln!(
                    self.err,
                    "warning [turn-retried] turn {turn} broke off and is being attempted again \
                     ({attempt}): {reason}. Disregard what was printed for it above."
                );
            }
            LoopEvent::TextDelta { text } => self.prose(&text),
            // Argument fragments are noise on a terminal; the call is reported once it is whole.
            LoopEvent::ToolArgumentsDelta { .. } => {}
            LoopEvent::Delegated { event, .. } => self.delegated(&event),
            // Reasoning goes to stderr with the rest of the progress, so stdout stays the answer.
            LoopEvent::ReasoningDelta { text } => {
                if !self.quiet {
                    let _ = write!(self.err, "{text}");
                    let _ = self.err.flush();
                }
            }
            LoopEvent::ToolRequested(call) => {
                let line = requested(&call);
                self.note(&line);
            }
            LoopEvent::ApprovalRequired { name, .. } => {
                self.note(&format!("? {name} needs a decision"));
            }
            LoopEvent::ApprovalResolved { approved, .. } => {
                self.note(if approved { "  approved" } else { "  denied" });
            }
            LoopEvent::ToolCompleted { failed, .. } => {
                self.note(if failed { "← failed" } else { "← ok" });
            }
            LoopEvent::Usage(usage) => self.note(&format!(
                "  usage {} in / {} out ({} cached)",
                usage.input_tokens, usage.output_tokens, usage.cached_input_tokens
            )),
            LoopEvent::Rates { source, as_of } => {
                self.note(&format!("  rates {as_of} — {source}"));
            }
            // Six decimals, because a turn can genuinely cost less than a cent and `$0.00` beside a
            // real charge reads as free.
            LoopEvent::Cost { micro_usd, .. } => self.note(&format!(
                "  cost ${}",
                harness_loop::micro_usd_as_decimal(micro_usd)
            )),
            LoopEvent::Warning { code, message } => {
                let _ = writeln!(self.err, "warning [{code}] {message}");
            }
            // Held, not printed: a `stop` hook can withdraw this one and the next turn can replace
            // it. The latest one is what a completed run writes, at `Finished`.
            LoopEvent::Answered { value, .. } if self.structured => {
                self.answer = Some(value);
                self.note("· answered");
            }
            event @ (LoopEvent::Compacted { .. }
            | LoopEvent::Answered { .. }
            | LoopEvent::DelegateStarted { .. }
            | LoopEvent::DelegateFinished { .. }
            | LoopEvent::HookRan { .. }) => self.note(&owned_line(&event)),
            LoopEvent::Finished { stop, .. } => {
                // No terminating newline under a schema: the answer line ends itself, and a run
                // that never answered must leave stdout empty rather than nearly empty.
                if self.structured {
                    // Only for a run that completed. A withdrawn answer, a bound, a cancel: the
                    // exit status says the run has no answer and stdout must say the same.
                    if self.answers && stop.is_completed() {
                        self.answer();
                    } else {
                        // Dropped rather than held: inside a walk the next step must not inherit
                        // the last one's answer, and a run with no answer has nothing to compose
                        // with either way.
                        self.answer = None;
                    }
                } else {
                    let _ = writeln!(self.out);
                    let _ = self.out.flush();
                }
                self.note(&format!("{stop:?}"));
            }
        }
    }
}

/// The walk's own events, on the same two streams the loop's go to.
///
/// Under `--json` they join the record on stdout, one per line, in the same stream — they already
/// carry `kind`, so a reader parsing line by line needs nothing new. On a terminal each is one
/// line of progress: a section entered, a step passed or failed, a retreat, a refusal. A step's
/// loop events land between its `step-started` and `step-finished` because it is one renderer and
/// one stream, which is the whole reason the walk and the runner share this object.
impl<O: Write, E: Write> FlowSink for Renderer<O, E> {
    fn emit(&mut self, event: FlowEvent) {
        if self.json {
            if let Ok(line) = serde_json::to_string(&event) {
                let _ = writeln!(self.out, "{line}");
                let _ = self.out.flush();
            }
            return;
        }
        match event {
            FlowEvent::FlowStarted { flow, steps } => {
                self.note(&format!("flow ▸ {flow} — {steps} step(s)"));
            }
            FlowEvent::GroupEntered {
                path, attempt, of, ..
            } => self.note(&format!("flow ▸ {path} (attempt {attempt} of {of})")),
            // Named even when it holds one node: *these could have run together* is a fact about
            // the document, and a reader who saw it only sometimes would infer concurrency from
            // silence.
            FlowEvent::LayerReady { path, nodes } => {
                self.note(&format!("  layer {path}: {}", nodes.join(", ")));
            }
            FlowEvent::StepStarted { path } => self.note(&format!("step → {path}")),
            FlowEvent::StepFinished { path, failed } => {
                self.note(&format!("step {} {path}", if failed { "✗" } else { "✓" }));
            }
            FlowEvent::NodeSkipped { path, because } => {
                self.note(&format!("step ⊘ {path}: {because}"));
            }
            FlowEvent::GroupRepeating { path, attempt, of } => {
                let because = self
                    .refusal
                    .take()
                    .unwrap_or_else(|| "it did not come out clean".to_owned());
                self.note(&format!(
                    "retreat ↺ {path} ({} of {of}): {because}",
                    attempt.saturating_add(1)
                ));
            }
            FlowEvent::HandoffIncomplete { path, missing } => {
                self.note(&format!(
                    "handoff ✗ {path}: never gave {}",
                    missing.join(", ")
                ));
            }
            FlowEvent::TransitionRefused {
                path,
                moment,
                attempt,
                reason,
            } => {
                let word = match moment {
                    Moment::Enter => "enter",
                    Moment::Leave => "leave",
                };
                self.note(&format!(
                    "refused ⊘ {path} ({word}, attempt {attempt}): {reason}"
                ));
                self.refusal = Some(reason);
            }
            FlowEvent::GroupLeft {
                path,
                failed,
                gave,
                attempts,
                exhausted,
            } => {
                // Whatever refused this section has been reported; it must not be read as the
                // reason for the next section's retreat.
                self.refusal = None;
                let verdict = match (failed, exhausted) {
                    (true, true) => "exhausted",
                    (true, false) => "failed",
                    (false, _) => "clean",
                };
                let gave = if gave.is_empty() {
                    String::new()
                } else {
                    format!(", gave {}", gave.join(", "))
                };
                self.note(&format!(
                    "flow ◂ {path} {verdict} after {attempts} attempt(s){gave}"
                ));
            }
            FlowEvent::FlowFinished {
                flow,
                ran,
                failed,
                skipped,
                retreats,
                clean,
            } => self.note(&format!(
                "flow {} {flow} — {ran} ran, {failed} failed, {skipped} skipped, {retreats} \
                 retreat(s)",
                if clean { "✓" } else { "✗" }
            )),
        }
    }
}

/// The one line that says a declared tool does not exist on this machine, and why.
///
/// Written here rather than in each caller so the run's own record and `b10x-harness tools` — the
/// command a person checks a machine with before starting a run — say the same sentence. Two
/// spellings of this would be two things to keep true, and the whole defect this closes was two
/// halves of the system disagreeing silently about what the run could do.
///
/// `note:` and not `warning:`: nothing went wrong. The machine answered honestly and the toolset
/// followed it. What is being reported is that the answer was *no*.
pub fn withheld_line(tool: &str, reason: &str) -> String {
    format!("note: `{tool}` is not published on this machine: {reason}")
}

/// The one line a requested call becomes, at whatever depth it was requested.
///
/// Cut at 200 characters because a `file_write` body is not a progress line; the call itself is
/// reported whole in the `--json` record, which is what a reader parses.
fn requested(call: &harness_wire::ToolCall) -> String {
    let arguments = serde_json::to_string(&call.arguments).unwrap_or_default();
    format!(
        "→ {} {}",
        call.name,
        arguments.chars().take(200).collect::<String>()
    )
}

/// One progress line for the events that only ever become a note.
fn owned_line(event: &LoopEvent) -> String {
    match event {
        LoopEvent::Compacted {
            elided_results,
            elided_bytes,
            summarised_items,
            bytes_before,
            bytes_after,
            summary_turn,
        } => format!(
            "  compacted {bytes_before} → {bytes_after} bytes: {elided_results} result(s) \
             elided ({elided_bytes} bytes){}",
            if *summary_turn {
                format!(", {summarised_items} item(s) summarised in one extra turn")
            } else {
                String::new()
            }
        ),
        LoopEvent::Answered { .. } => "· answered".to_owned(),
        LoopEvent::DelegateStarted { task, .. } => {
            format!("· delegate: {}", task.lines().next().unwrap_or(""))
        }
        LoopEvent::DelegateFinished { stop, turns, .. } => {
            format!("· delegate finished after {turns} turn(s): {stop:?}")
        }
        LoopEvent::HookRan {
            point, decision, ..
        } => format!("· hook {}: {decision:?}", point.as_str()),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_loop::LoopStop;
    use harness_wire::ToolName;

    fn render(events: Vec<LoopEvent>, json: bool) -> (String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        {
            let mut renderer = Renderer::new(&mut out, &mut err, json, false);
            for event in events {
                LoopSink::emit(&mut renderer, event);
            }
        }
        (
            String::from_utf8(out).expect("utf-8"),
            String::from_utf8(err).expect("utf-8"),
        )
    }

    #[test]
    fn text_is_the_only_thing_on_stdout() {
        let (out, err) = render(
            vec![
                LoopEvent::Started {
                    model: "m".to_owned(),
                    published_tools: vec![ToolName::new("a").expect("valid")],
                    operations: Vec::new(),
                    withheld: Vec::new(),
                    skills: Vec::new(),
                    agents: Vec::new(),
                    profiles: Vec::new(),
                    credential_source: "named".to_owned(),
                },
                LoopEvent::TextDelta {
                    text: "the ".to_owned(),
                },
                LoopEvent::TextDelta {
                    text: "answer".to_owned(),
                },
                LoopEvent::Finished {
                    stop: LoopStop::Completed,
                    turns: 1,
                },
            ],
            false,
        );
        assert_eq!(out, "the answer\n");
        assert!(err.contains("model m"), "{err}");
    }

    #[test]
    fn json_mode_puts_one_event_per_line_on_stdout() {
        let (out, err) = render(
            vec![
                LoopEvent::TextDelta {
                    text: "x".to_owned(),
                },
                LoopEvent::Finished {
                    stop: LoopStop::Completed,
                    turns: 1,
                },
            ],
            true,
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"kind\":\"text-delta\""), "{}", lines[0]);
        assert!(err.is_empty(), "json mode keeps stderr clean: {err}");
    }

    fn render_structured(events: Vec<LoopEvent>) -> (String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        {
            let mut renderer = Renderer::new(&mut out, &mut err, false, false).structured(true);
            for event in events {
                LoopSink::emit(&mut renderer, event);
            }
        }
        (
            String::from_utf8(out).expect("utf-8"),
            String::from_utf8(err).expect("utf-8"),
        )
    }

    fn a_call(name: &str) -> harness_wire::ToolCall {
        harness_wire::ToolCall {
            call_id: harness_wire::CallId::new("call-1").expect("valid"),
            name: ToolName::new(name).expect("valid"),
            arguments: serde_json::json!({"path": "README.md"}),
        }
    }

    #[test]
    fn under_a_schema_stdout_is_the_answer_and_the_prose_is_progress() {
        // What `… --output-schema s.json | jq` depends on: one line, and nothing else on it.
        let (out, err) = render_structured(vec![
            LoopEvent::TextDelta {
                text: "let me look…".to_owned(),
            },
            LoopEvent::Answered {
                call_id: harness_wire::CallId::new("call-9").expect("valid"),
                value: serde_json::json!({"verdict": "ok", "files": 2}),
            },
            LoopEvent::Finished {
                stop: LoopStop::Completed,
                turns: 2,
            },
        ]);
        assert_eq!(out, "{\"files\":2,\"verdict\":\"ok\"}\n");
        assert!(err.contains("let me look"), "the prose is a note: {err}");
    }

    #[test]
    fn under_a_schema_a_run_that_never_answered_leaves_stdout_empty() {
        // Rather than nearly empty: a reader that got a blank line and exit 2 has to tell the two
        // apart, and a newline is not an answer.
        let (out, err) = render_structured(vec![
            LoopEvent::TextDelta {
                text: "here it is in prose".to_owned(),
            },
            LoopEvent::Finished {
                stop: LoopStop::Unstructured { asked_again: 1 },
                turns: 2,
            },
        ]);
        assert_eq!(out, "");
        assert!(err.contains("Unstructured"), "{err}");
    }

    #[test]
    fn an_answer_a_stop_hook_withdrew_is_never_printed() {
        // The measured failure: the value went to stdout the moment it arrived, a `stop` hook
        // withdrew it, the loop turned again and the run exited 2 `unstructured` — with the
        // withdrawn answer already on the stream a consumer was reading as JSON.
        let (out, err) = render_structured(vec![
            LoopEvent::Answered {
                call_id: harness_wire::CallId::new("call-9").expect("valid"),
                value: serde_json::json!({"verdict": "withdrawn"}),
            },
            LoopEvent::Finished {
                stop: LoopStop::Unstructured { asked_again: 1 },
                turns: 3,
            },
        ]);
        assert_eq!(out, "", "a run with no answer has nothing to compose with");
        assert!(err.contains("Unstructured"), "{err}");
    }

    #[test]
    fn a_second_answer_replaces_the_first_rather_than_joining_it_on_stdout() {
        // Block, then answer again: two `answered` events in one run. Two lines would be two
        // answers, and a reader taking the first would take the one the hook refused.
        let (out, _err) = render_structured(vec![
            LoopEvent::Answered {
                call_id: harness_wire::CallId::new("call-9").expect("valid"),
                value: serde_json::json!({"verdict": "first"}),
            },
            LoopEvent::Answered {
                call_id: harness_wire::CallId::new("call-11").expect("valid"),
                value: serde_json::json!({"verdict": "second"}),
            },
            LoopEvent::Finished {
                stop: LoopStop::Completed,
                turns: 4,
            },
        ]);
        assert_eq!(out, "{\"verdict\":\"second\"}\n");
    }

    #[test]
    fn a_retried_turn_puts_no_bare_newline_before_a_structured_answer() {
        // A transport blip on turn one used to write "\n" to stdout, which is the one byte a
        // consumer piping to a JSON reader cannot survive. The marker belongs to prose, and under
        // a schema there is none on that stream to disregard.
        let (out, err) = render_structured(vec![
            LoopEvent::TurnRetried {
                turn: 1,
                attempt: 2,
                reason: "the stream broke off".to_owned(),
            },
            LoopEvent::Answered {
                call_id: harness_wire::CallId::new("call-9").expect("valid"),
                value: serde_json::json!({"verdict": "ok"}),
            },
            LoopEvent::Finished {
                stop: LoopStop::Completed,
                turns: 2,
            },
        ]);
        assert_eq!(out, "{\"verdict\":\"ok\"}\n");
        assert!(err.contains("turn-retried"), "still reported: {err}");
    }

    #[test]
    fn a_retried_turn_still_marks_the_prose_it_invalidated_when_there_is_no_schema() {
        let (out, err) = render(
            vec![
                LoopEvent::TextDelta {
                    text: "half an answer".to_owned(),
                },
                LoopEvent::TurnRetried {
                    turn: 1,
                    attempt: 2,
                    reason: "the stream broke off".to_owned(),
                },
                LoopEvent::TextDelta {
                    text: "the whole answer".to_owned(),
                },
                LoopEvent::Finished {
                    stop: LoopStop::Completed,
                    turns: 1,
                },
            ],
            false,
        );
        assert_eq!(out, "half an answer\nthe whole answer\n");
        assert!(err.contains("Disregard what was printed"), "{err}");
    }

    #[test]
    fn a_delegates_own_run_is_indented_and_its_text_is_not_a_second_answer() {
        let wrapped = |event: LoopEvent| LoopEvent::Delegated {
            call_id: harness_wire::CallId::new("call-3").expect("valid"),
            event: Box::new(event),
        };
        let (out, err) = render(
            vec![
                LoopEvent::DelegateStarted {
                    call_id: harness_wire::CallId::new("call-3").expect("valid"),
                    task: "survey the crate\nand report".to_owned(),
                },
                wrapped(LoopEvent::ToolRequested(a_call("file_read"))),
                wrapped(LoopEvent::ToolCompleted {
                    call_id: harness_wire::CallId::new("call-4").expect("valid"),
                    failed: false,
                }),
                wrapped(LoopEvent::Warning {
                    code: "unpublished-tool".to_owned(),
                    message: "nope".to_owned(),
                }),
                wrapped(LoopEvent::TextDelta {
                    text: "the child's own report".to_owned(),
                }),
                LoopEvent::DelegateFinished {
                    call_id: harness_wire::CallId::new("call-3").expect("valid"),
                    stop: LoopStop::Completed,
                    turns: 2,
                },
                LoopEvent::TextDelta {
                    text: "the parent's answer".to_owned(),
                },
                LoopEvent::Finished {
                    stop: LoopStop::Completed,
                    turns: 3,
                },
            ],
            false,
        );
        assert_eq!(
            out, "the parent's answer\n",
            "the child reports to the parent, not to stdout"
        );
        assert!(err.contains("· delegate: survey the crate"), "{err}");
        assert!(err.contains("  → file_read"), "indented by two: {err}");
        assert!(err.contains("  ← ok"), "{err}");
        assert!(err.contains("  warning [unpublished-tool]"), "{err}");
        assert!(err.contains("delegate finished after 2 turn(s)"), "{err}");
    }

    #[test]
    fn json_mode_is_unchanged_by_a_schema_and_carries_the_wrapped_events_whole() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        {
            let mut renderer = Renderer::new(&mut out, &mut err, true, false).structured(true);
            LoopSink::emit(
                &mut renderer,
                LoopEvent::Answered {
                    call_id: harness_wire::CallId::new("call-9").expect("valid"),
                    value: serde_json::json!({"verdict": "ok"}),
                },
            );
            LoopSink::emit(
                &mut renderer,
                LoopEvent::Delegated {
                    call_id: harness_wire::CallId::new("call-3").expect("valid"),
                    event: Box::new(LoopEvent::TextDelta {
                        text: "child".to_owned(),
                    }),
                },
            );
        }
        let out = String::from_utf8(out).expect("utf-8");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "one event per line, as always: {out}");
        assert!(lines[0].contains("\"kind\":\"answered\""), "{}", lines[0]);
        assert!(lines[1].contains("\"kind\":\"delegated\""), "{}", lines[1]);
        assert!(lines[1].contains("\"child\""), "nested whole: {}", lines[1]);
    }

    #[test]
    fn a_refused_program_is_printed_by_code_before_the_result_it_explains() {
        // What a person watching the run sees, in the order the loop emitted it. Read off the
        // failed result alone this was a program that would not start; the code says it is a rule.
        let (_, err) = render(
            vec![
                LoopEvent::ToolRequested(a_call("run")),
                LoopEvent::Warning {
                    code: "program-refused".to_owned(),
                    message: "`sh` is not a program this run may start. Declared: cargo."
                        .to_owned(),
                },
                LoopEvent::ToolCompleted {
                    call_id: harness_wire::CallId::new("call-1").expect("valid"),
                    failed: true,
                },
                LoopEvent::Finished {
                    stop: LoopStop::Completed,
                    turns: 1,
                },
            ],
            false,
        );
        let warning = err
            .find("warning [program-refused] `sh` is not a program this run may start")
            .unwrap_or_else(|| panic!("the code and the sentence are both printed: {err}"));
        let failed = err
            .find("← failed")
            .unwrap_or_else(|| panic!("the result is printed too: {err}"));
        assert!(warning < failed, "the warning comes first: {err}");
    }

    #[test]
    fn warnings_are_reported_even_when_quiet() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        {
            let mut renderer = Renderer::new(&mut out, &mut err, false, true);
            LoopSink::emit(&mut renderer, LoopEvent::TurnStarted { turn: 1 });
            LoopSink::emit(
                &mut renderer,
                LoopEvent::Warning {
                    code: "unpublished-tool".to_owned(),
                    message: "nope".to_owned(),
                },
            );
        }
        let err = String::from_utf8(err).expect("utf-8");
        assert!(!err.contains("turn 1"), "quiet drops progress: {err}");
        assert!(err.contains("unpublished-tool"), "{err}");
    }

    #[test]
    fn a_tool_the_machine_would_not_admit_is_stated_at_the_start_and_survives_quiet() {
        // The line that was missing. A person reading this run saw six tools and had no way to
        // know a seventh had been asked for and refused; the run hand-wrote files instead and the
        // failure read as the model's for weeks.
        let started = || LoopEvent::Started {
            model: "m".to_owned(),
            published_tools: vec![ToolName::new("tool_invoke").expect("valid")],
            operations: vec!["file.read".to_owned()],
            withheld: vec![harness_loop::Withheld {
                tool: "run".to_owned(),
                reason: "`exec.argv-only` must be true and this machine says nothing.".to_owned(),
            }],
            skills: Vec::new(),
            agents: Vec::new(),
            profiles: Vec::new(),
            credential_source: "named".to_owned(),
        };

        let (out, err) = render(vec![started()], false);
        assert!(
            err.contains(
                "note: `run` is not published on this machine: `exec.argv-only` must be \
                          true and this machine says nothing."
            ),
            "the tool and the predicate, on one line: {err}"
        );
        assert!(out.is_empty(), "stdout stays the answer: {out:?}");

        // Quiet drops progress, not the shape of the run — the run that most needed this line was
        // an unattended one.
        let mut out = Vec::new();
        let mut err = Vec::new();
        {
            let mut renderer = Renderer::new(&mut out, &mut err, false, true);
            LoopSink::emit(&mut renderer, started());
        }
        let err = String::from_utf8(err).expect("utf-8");
        assert!(!err.contains("model m"), "quiet drops progress: {err}");
        assert!(err.contains("note: `run` is not published"), "{err}");
    }

    fn render_flow(events: Vec<FlowEvent>, json: bool) -> (String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        {
            let mut renderer = Renderer::new(&mut out, &mut err, json, false).within_a_flow();
            for event in events {
                FlowSink::emit(&mut renderer, event);
            }
        }
        (
            String::from_utf8(out).expect("utf-8"),
            String::from_utf8(err).expect("utf-8"),
        )
    }

    #[test]
    fn a_walk_reads_as_one_line_per_thing_that_happened() {
        let (out, err) = render_flow(
            vec![
                FlowEvent::GroupEntered {
                    path: "root.shape".to_owned(),
                    layers: 2,
                    attempt: 1,
                    of: 3,
                },
                FlowEvent::StepFinished {
                    path: "root.shape.specify".to_owned(),
                    failed: false,
                },
                FlowEvent::StepFinished {
                    path: "root.shape.verify".to_owned(),
                    failed: true,
                },
                FlowEvent::GroupRepeating {
                    path: "root.shape".to_owned(),
                    attempt: 1,
                    of: 3,
                },
            ],
            false,
        );
        assert!(
            out.is_empty(),
            "stdout belongs to the walk's record: {out:?}"
        );
        assert!(err.contains("flow ▸ root.shape (attempt 1 of 3)"), "{err}");
        assert!(err.contains("step ✓ root.shape.specify"), "{err}");
        assert!(err.contains("step ✗ root.shape.verify"), "{err}");
        // The attempt about to be taken, not the one that just failed.
        assert!(err.contains("retreat ↺ root.shape (2 of 3):"), "{err}");
    }

    #[test]
    fn a_refused_boundary_is_reported_and_becomes_the_retreat_it_explains() {
        // `group-repeating` says which attempt failed and not why, because the notation evaluates
        // no gate. On a terminal the two lines are next to each other, so the words are carried.
        let (_, err) = render_flow(
            vec![
                FlowEvent::TransitionRefused {
                    path: "root.implement-to-review".to_owned(),
                    moment: Moment::Leave,
                    attempt: 1,
                    reason: "the tests are red".to_owned(),
                },
                FlowEvent::GroupRepeating {
                    path: "root.implement-to-review".to_owned(),
                    attempt: 1,
                    of: 3,
                },
            ],
            false,
        );
        assert!(
            err.contains(
                "refused ⊘ root.implement-to-review (leave, attempt 1): the tests are red"
            ),
            "{err}"
        );
        assert!(
            err.contains("retreat ↺ root.implement-to-review (2 of 3): the tests are red"),
            "{err}"
        );
    }

    #[test]
    fn a_walk_under_json_joins_the_record_on_stdout_one_event_per_line() {
        let (out, err) = render_flow(
            vec![
                FlowEvent::StepStarted {
                    path: "root.specify".to_owned(),
                },
                FlowEvent::FlowFinished {
                    flow: "root".to_owned(),
                    ran: 1,
                    failed: 0,
                    skipped: 0,
                    retreats: 0,
                    clean: true,
                },
            ],
            true,
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "{out}");
        assert!(
            lines[0].contains("\"kind\":\"step-started\""),
            "{}",
            lines[0]
        );
        assert!(
            lines[1].contains("\"kind\":\"flow-finished\""),
            "{}",
            lines[1]
        );
        assert!(err.is_empty(), "json mode keeps stderr clean: {err}");
    }

    #[test]
    fn inside_a_walk_a_step_answer_never_reaches_stdout() {
        // Every step runs under a schema, and its answer is the walk's to read. One JSON line per
        // step on stdout would be eight answers to a question nobody asked, in the middle of the
        // flow's own record.
        let (out, err) = render_flow(Vec::new(), false);
        assert!(out.is_empty() && err.is_empty());
        let mut out = Vec::new();
        let mut err = Vec::new();
        {
            let mut renderer = Renderer::new(&mut out, &mut err, false, false).within_a_flow();
            LoopSink::emit(
                &mut renderer,
                LoopEvent::Answered {
                    call_id: harness_wire::CallId::new("call-9").expect("valid"),
                    value: serde_json::json!({"outcome": "passed"}),
                },
            );
            LoopSink::emit(
                &mut renderer,
                LoopEvent::Finished {
                    stop: LoopStop::Completed,
                    turns: 1,
                },
            );
        }
        assert_eq!(String::from_utf8(out).expect("utf-8"), "");
        assert!(
            String::from_utf8(err)
                .expect("utf-8")
                .contains("· answered"),
            "still reported as progress"
        );
    }

    #[test]
    fn a_run_that_was_refused_nothing_says_nothing_about_it() {
        let (_, err) = render(
            vec![LoopEvent::Started {
                model: "m".to_owned(),
                published_tools: vec![ToolName::new("file_read").expect("valid")],
                operations: vec!["file.read".to_owned()],
                withheld: Vec::new(),
                skills: Vec::new(),
                agents: Vec::new(),
                profiles: Vec::new(),
                credential_source: "named".to_owned(),
            }],
            false,
        );
        assert!(!err.contains("note:"), "absence stays absence: {err}");
    }
}
