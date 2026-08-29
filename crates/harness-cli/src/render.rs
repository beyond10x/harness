//! Turning the loop's event stream into something a person, or a program, can read.

use std::io::Write;

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
pub struct Renderer<O: Write, E: Write> {
    out: O,
    err: E,
    json: bool,
    quiet: bool,
    /// Whether stdout belongs to the structured answer alone.
    structured: bool,
    /// The latest `Answered` value under a schema, until `Finished` says whether it stood.
    answer: Option<serde_json::Value>,
}

impl<O: Write, E: Write> Renderer<O, E> {
    pub fn new(out: O, err: E, json: bool, quiet: bool) -> Self {
        Self {
            out,
            err,
            json,
            quiet,
            structured: false,
            answer: None,
        }
    }

    /// Keeps stdout for the answer: prose goes to stderr and only `answered` is printed.
    #[must_use]
    pub fn structured(mut self, structured: bool) -> Self {
        self.structured = structured;
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
            LoopEvent::Started {
                model,
                published_tools,
                operations,
                withheld,
            } => {
                let names: Vec<&str> = published_tools
                    .iter()
                    .map(harness_wire::ToolName::as_str)
                    .collect();
                self.note(&format!("model {model} · tools: {}", names.join(", ")));
                if !operations.is_empty() {
                    self.note(&format!("  can: {}", operations.join(", ")));
                }
                self.withheld(&withheld);
            }
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
                    if stop.is_completed() {
                        self.answer();
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
                renderer.emit(event);
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
                renderer.emit(event);
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
            renderer.emit(LoopEvent::Answered {
                call_id: harness_wire::CallId::new("call-9").expect("valid"),
                value: serde_json::json!({"verdict": "ok"}),
            });
            renderer.emit(LoopEvent::Delegated {
                call_id: harness_wire::CallId::new("call-3").expect("valid"),
                event: Box::new(LoopEvent::TextDelta {
                    text: "child".to_owned(),
                }),
            });
        }
        let out = String::from_utf8(out).expect("utf-8");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "one event per line, as always: {out}");
        assert!(lines[0].contains("\"kind\":\"answered\""), "{}", lines[0]);
        assert!(lines[1].contains("\"kind\":\"delegated\""), "{}", lines[1]);
        assert!(lines[1].contains("\"child\""), "nested whole: {}", lines[1]);
    }

    #[test]
    fn warnings_are_reported_even_when_quiet() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        {
            let mut renderer = Renderer::new(&mut out, &mut err, false, true);
            renderer.emit(LoopEvent::TurnStarted { turn: 1 });
            renderer.emit(LoopEvent::Warning {
                code: "unpublished-tool".to_owned(),
                message: "nope".to_owned(),
            });
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
            renderer.emit(started());
        }
        let err = String::from_utf8(err).expect("utf-8");
        assert!(!err.contains("model m"), "quiet drops progress: {err}");
        assert!(err.contains("note: `run` is not published"), "{err}");
    }

    #[test]
    fn a_run_that_was_refused_nothing_says_nothing_about_it() {
        let (_, err) = render(
            vec![LoopEvent::Started {
                model: "m".to_owned(),
                published_tools: vec![ToolName::new("file_read").expect("valid")],
                operations: vec!["file.read".to_owned()],
                withheld: Vec::new(),
            }],
            false,
        );
        assert!(!err.contains("note:"), "absence stays absence: {err}");
    }
}
