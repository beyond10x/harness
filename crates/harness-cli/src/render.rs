//! Turning the loop's event stream into something a person, or a program, can read.

use std::io::Write;

use harness_loop::{LoopEvent, LoopSink};

/// Writes the run as it happens.
///
/// Text goes to stdout unbuffered so a person sees the answer forming; everything else goes to
/// stderr, which keeps stdout usable as the answer itself when the output is piped.
pub struct Renderer<O: Write, E: Write> {
    out: O,
    err: E,
    json: bool,
    quiet: bool,
}

impl<O: Write, E: Write> Renderer<O, E> {
    pub fn new(out: O, err: E, json: bool, quiet: bool) -> Self {
        Self {
            out,
            err,
            json,
            quiet,
        }
    }

    fn note(&mut self, line: &str) {
        if self.quiet {
            return;
        }
        let _ = writeln!(self.err, "{line}");
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
            } => {
                let names: Vec<&str> = published_tools
                    .iter()
                    .map(harness_wire::ToolName::as_str)
                    .collect();
                self.note(&format!("model {model} · tools: {}", names.join(", ")));
            }
            LoopEvent::TurnStarted { turn } => self.note(&format!("· turn {turn}")),
            LoopEvent::TextDelta { text } => {
                let _ = write!(self.out, "{text}");
                let _ = self.out.flush();
            }
            // Argument fragments are noise on a terminal; the call is reported once it is whole.
            LoopEvent::ToolArgumentsDelta { .. } => {}
            LoopEvent::ToolRequested(call) => {
                let arguments = serde_json::to_string(&call.arguments).unwrap_or_default();
                self.note(&format!(
                    "→ {} {}",
                    call.name,
                    arguments.chars().take(200).collect::<String>()
                ));
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
            LoopEvent::Warning { code, message } => {
                let _ = writeln!(self.err, "warning [{code}] {message}");
            }
            LoopEvent::Finished { stop } => {
                let _ = writeln!(self.out);
                let _ = self.out.flush();
                self.note(&format!("{stop:?}"));
            }
        }
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
                },
                LoopEvent::TextDelta {
                    text: "the ".to_owned(),
                },
                LoopEvent::TextDelta {
                    text: "answer".to_owned(),
                },
                LoopEvent::Finished {
                    stop: LoopStop::Completed,
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
                },
            ],
            true,
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"kind\":\"text-delta\""), "{}", lines[0]);
        assert!(err.is_empty(), "json mode keeps stderr clean: {err}");
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
}
