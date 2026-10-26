//! Handing each approval to the process that launched this run (atlas ADR 0104).
//!
//! Under metaharness this binary runs with no terminal and nobody at it, so `--approve auto`
//! resolves to `DenyAll` and nothing above harness could approve a call. ADR 0104 makes the
//! launching layer the one that decides per-call tool permission, and this harness the one that
//! enforces: the loop still asks **before** the effect (`AGENTS.md` invariant 12), and what it asks
//! is now a process instead of a person.
//!
//! # The exchange
//!
//! The question is the record's own `approval-required` event on stdout, flushed before the run
//! blocks; the `tool-requested` event with the same `call_id` carries the arguments. The answer is
//! one line on stdin:
//!
//! ```text
//! {"call_id":"<id>","decision":"allow"}
//! {"call_id":"<id>","decision":"deny","reason":"<non-empty>"}
//! ```
//!
//! The vocabulary is a **copy** of metaharness's `Decision` (`allow`, `deny`), never an import
//! (`AGENTS.md` invariant 15's rule). `replace` and `abstain` are not decisions here: how a rewrite
//! reaches this layer is ADR 0104's open question, and a word this module does not know is a
//! refusal, not a guess.
//!
//! # Fails closed, by name
//!
//! Every answer other than a well-formed `allow` for exactly the call asked about is a denial whose
//! reason says what was wrong and that retrying cannot help (`AGENTS.md` invariant 9) — the model
//! has to know the effect did not happen. A line over [`MAX_DECISION_LINE_BYTES`] is refused by
//! name and never cut down to something that parses (invariant 8). When stdin ends, this call and
//! every later one is denied **without reading**, and the run says so once on stderr.
//!
//! A decision names only a `call_id`, so a call id already decided in this run is refused whatever
//! the answer: a second line the caller wrote for one call would otherwise approve whatever the
//! model asks next under the same id, a call the caller was never shown. The answer line for the
//! reused id is still read and discarded, because the caller saw `approval-required` for it and
//! answered; leaving that line in the pipe would hand every later call the answer meant for the
//! call before it.
//!
//! # Interruptible
//!
//! Stdin is read on one thread of its own, one line per call asked about, and the approver waits on
//! that thread while watching the run's cancellation token. A run interrupted while it waits on its
//! caller ends like a run interrupted anywhere else, instead of blocking until the caller writes.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::{BufRead, ErrorKind, Stderr, StdinLock, Write};
use std::marker::PhantomData;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use harness_loop::{ApprovalDecision, ApprovalPort, LoopCancel};
use harness_wire::{ToolCall, ToolSpec};
use serde::Deserialize;

/// The longest decision line read, its newline included.
///
/// A decision is two short strings and a reason; 64 KiB is room for any reason a model should be
/// shown and small enough that a caller writing a runaway line cannot make this process hold it.
pub const MAX_DECISION_LINE_BYTES: usize = 64 * 1024;

/// How many characters of a value the caller sent are quoted back in a refusal.
const MAX_QUOTED_CHARS: usize = 64;

/// How often a wait on the caller looks at the run's cancellation token.
const CANCEL_POLL: Duration = Duration::from_millis(50);

/// What every refusal ends with, in the words the loop's other approvers use.
const NO_RETRY: &str =
    "so retrying cannot approve it either; do what can be done without it and say what could not";

/// One decision line, exactly as the contract shapes it.
///
/// `deny_unknown_fields` because a field this harness does not read is a field the caller thinks
/// means something — `"scope":"always"` read as an ordinary `allow` would be an approval broader
/// than anybody here granted.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    call_id: String,
    decision: String,
    #[serde(default)]
    reason: Option<String>,
}

/// An approver that asks the process that launched this run, one line per call.
///
/// Generic over its reader and its notice writer so the tests drive it with cursors;
/// [`Caller::stdio`] builds the real one over this process's stdin and stderr. The reader `R` is
/// owned by the thread that reads it, not by the approver, so it appears here only as a type.
pub struct Caller<R: BufRead, W: Write> {
    /// Asks the reading thread for exactly one more line.
    ask: Sender<()>,
    /// The reading thread's answers, one per ask.
    lines: Receiver<std::io::Result<Line>>,
    notices: W,
    /// Whether the reader has ended. Once it has, nothing is read again.
    closed: bool,
    /// Every call id this approver has been asked about in this run.
    decided: BTreeSet<String>,
    /// The run's cancellation token, watched while waiting on the caller.
    cancel: LoopCancel,
    reader: PhantomData<fn() -> R>,
}

impl<R: BufRead + Send + 'static, W: Write> Caller<R, W> {
    /// An approver over an arbitrary reader, telling `notices` when that reader ends, in a run
    /// nothing interrupts.
    pub fn over(reader: R, notices: W) -> Self {
        Self::interruptible(reader, notices, LoopCancel::new())
    }

    /// An approver over an arbitrary reader that stops waiting when `cancel` fires.
    pub fn interruptible(reader: R, notices: W, cancel: LoopCancel) -> Self {
        Self::reading(move || reader, notices, cancel)
    }
}

impl<R: BufRead, W: Write> Caller<R, W> {
    /// Starts the thread that opens the reader and reads one line each time it is asked.
    ///
    /// The thread reads only when asked, so a line is never taken off the stream before a call
    /// needs it, and it stops after the stream ends or fails. If it cannot start, its channels
    /// close and the first call is refused as a closed stdin naming why.
    fn reading(open: impl FnOnce() -> R + Send + 'static, notices: W, cancel: LoopCancel) -> Self
    where
        R: 'static,
    {
        let (ask, asked) = mpsc::channel::<()>();
        let (answer, lines) = mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("caller-decisions".to_owned())
            .spawn(move || {
                let mut reader = open();
                for () in asked {
                    let line = line(&mut reader);
                    let last = !matches!(line, Ok(Line::Text(_) | Line::Over));
                    if answer.send(line).is_err() || last {
                        return;
                    }
                }
            });
        Self {
            ask,
            lines,
            notices,
            closed: false,
            decided: BTreeSet::new(),
            cancel,
            reader: PhantomData,
        }
    }

    /// Whether the reader has ended, so every further call is refused without reading.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// The next line, waited for while watching the run's cancellation token.
    ///
    /// `None` when the run was interrupted first. The line asked for may still arrive afterwards;
    /// nothing reads it, because a cancelled token stays cancelled and every later call is refused
    /// before asking.
    fn next(&mut self) -> Option<std::io::Result<Line>> {
        if self.ask.send(()).is_err() {
            return Some(Err(stopped()));
        }
        loop {
            match self.lines.recv_timeout(CANCEL_POLL) {
                Ok(line) => return Some(line),
                Err(RecvTimeoutError::Disconnected) => return Some(Err(stopped())),
                Err(RecvTimeoutError::Timeout) if self.cancel.is_cancelled() => return None,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    /// Records that no decision can come any more, says so once, and denies.
    fn closed(&mut self, name: &str, cause: Option<&std::io::Error>) -> ApprovalDecision {
        if !self.closed {
            self.closed = true;
            let because = cause.map_or_else(String::new, |error| format!(" ({error})"));
            let _ = writeln!(
                self.notices,
                "stdin closed{because}: no decision can come from the process that launched this \
                 run; every further call that needs one is refused without reading."
            );
            let _ = self.notices.flush();
        }
        ApprovalDecision::denied(format!(
            "`{name}` needs a decision from the process that launched this run and its stdin has \
             closed, {NO_RETRY}"
        ))
    }
}

/// Why a reading thread that is gone cannot answer.
fn stopped() -> std::io::Error {
    std::io::Error::other("the thread reading it has stopped")
}

/// One line, newline included, or how reading it failed.
///
/// Read through `fill_buf` rather than `read_line` so the bound holds **while reading**: a line is
/// never held past [`MAX_DECISION_LINE_BYTES`], and the rest of an oversized one is discarded up to
/// its newline, so the caller's next answer is read as the next answer rather than the tail of this
/// one being read as it.
fn line(reader: &mut impl BufRead) -> std::io::Result<Line> {
    let mut line: Vec<u8> = Vec::new();
    loop {
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if available.is_empty() {
            return Ok(if line.is_empty() {
                Line::End
            } else {
                Line::Text(line)
            });
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |at| at + 1);
        if line.len() + take > MAX_DECISION_LINE_BYTES {
            reader.consume(take);
            if newline.is_none() {
                discard_to_newline(reader)?;
            }
            return Ok(Line::Over);
        }
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            return Ok(Line::Text(line));
        }
    }
}

/// Reads and drops everything up to and including the next newline, or to the end.
fn discard_to_newline(reader: &mut impl BufRead) -> std::io::Result<()> {
    loop {
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if available.is_empty() {
            return Ok(());
        }
        if let Some(at) = available.iter().position(|byte| *byte == b'\n') {
            reader.consume(at + 1);
            return Ok(());
        }
        let all = available.len();
        reader.consume(all);
    }
}

impl Caller<StdinLock<'static>, Stderr> {
    /// The approver over this process's own stdin, noticing on stderr, interrupted by `cancel`.
    ///
    /// The lock is taken on the reading thread and held for the run: under `--approve caller`
    /// stdin carries decisions, and nothing else in this process reads it. The shipped hooks get a
    /// pipe of their own, and every shipped tool and MCP server is started with a stdin of its own,
    /// so none of them is handed this stream. That is a property of what is started, not a wall:
    /// a host-process run under the substrate has no pid namespace, and a child there could still
    /// open `/proc/$PPID/fd/0` and read a decision line meant for this process.
    pub fn stdio(cancel: LoopCancel) -> Self {
        Self::reading(|| std::io::stdin().lock(), std::io::stderr(), cancel)
    }
}

/// One read off the caller's stream.
enum Line {
    /// A whole line, its newline included when it had one.
    Text(Vec<u8>),
    /// A line longer than the bound. Its bytes were discarded, not kept.
    Over,
    /// The stream ended with nothing more to read.
    End,
}

/// What one decision line says about the call it was asked about.
///
/// `line` may carry its newline; `call_id` is the call the loop is asking about and `name` the
/// catalogue entry it resolved to, which is what every refusal names.
pub fn judge(line: &[u8], call_id: &str, name: &str) -> ApprovalDecision {
    let line = line
        .strip_suffix(b"\n")
        .map_or(line, |line| line.strip_suffix(b"\r").unwrap_or(line));
    let asked = format!("`{name}` was put to the process that launched this run");
    let answer: Answer = match serde_json::from_slice(line) {
        Ok(answer) => answer,
        Err(error) => {
            return ApprovalDecision::denied(format!(
                "{asked} and the line it answered with is not a decision ({error}); a decision is \
                 {{\"call_id\":\"{call_id}\",\"decision\":\"allow\"}} or a `deny` with a \
                 `reason`, {NO_RETRY}"
            ));
        }
    };
    if answer.call_id != call_id {
        return ApprovalDecision::denied(format!(
            "{asked} and its answer named another call ({}, not `{call_id}`), {NO_RETRY}",
            quoted(&answer.call_id)
        ));
    }
    let reason = answer
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|reason| !reason.is_empty());
    match (answer.decision.as_str(), reason) {
        ("allow", None) if answer.reason.is_none() => ApprovalDecision::Approved,
        ("allow", _) => ApprovalDecision::denied(format!(
            "{asked} and it answered `allow` with a `reason`, which only a `deny` carries, so the \
             answer is not one this harness can read as approval; {NO_RETRY}"
        )),
        ("deny", Some(reason)) => ApprovalDecision::denied(format!(
            "`{name}` was refused by the process that launched this run, and retrying cannot \
             approve it either; do what can be done without it and say what could not. Its \
             reason: {reason}"
        )),
        ("deny", None) => ApprovalDecision::denied(format!(
            "{asked} and it answered `deny` without a reason, which a refusal must carry; \
             {NO_RETRY}"
        )),
        (other, _) => ApprovalDecision::denied(format!(
            "{asked} and it answered {}, which is not a decision this harness takes (it takes \
             `allow` and `deny`), {NO_RETRY}",
            quoted(other)
        )),
    }
}

/// A value the caller sent, backticked, bounded, and with every control character made inert.
///
/// The cut is stated with both figures, because a bound that is not stated reads exactly like a
/// complete value (`AGENTS.md` invariant 8).
fn quoted(text: &str) -> String {
    let total = text.chars().count();
    let mut shown = String::new();
    for character in text.chars().take(MAX_QUOTED_CHARS) {
        if character.is_control() {
            let _ = write!(shown, "\\u{{{:x}}}", u32::from(character));
        } else if character == '`' {
            shown.push('\'');
        } else {
            shown.push(character);
        }
    }
    if total > MAX_QUOTED_CHARS {
        format!("`{shown}…` (first {MAX_QUOTED_CHARS} of {total} characters)")
    } else {
        format!("`{shown}`")
    }
}

/// The refusal of a call the run was interrupted while asking about, or asked about after.
fn interrupted(name: &str) -> ApprovalDecision {
    ApprovalDecision::denied(format!(
        "`{name}` needs a decision from the process that launched this run and the run was \
         interrupted, {NO_RETRY}"
    ))
}

impl<R: BufRead, W: Write> ApprovalPort for Caller<R, W> {
    fn decide(&mut self, call: &ToolCall, spec: &ToolSpec) -> ApprovalDecision {
        let name = spec.name.as_str();
        let call_id = call.call_id.as_str();
        let reused = !self.decided.insert(call_id.to_owned());
        if self.cancel.is_cancelled() {
            return interrupted(name);
        }
        if self.closed {
            return self.closed(name, None);
        }
        match self.next() {
            None => interrupted(name),
            // The caller was shown `approval-required` for this call and answers it like any
            // other, so its line is read and discarded: refusing without reading would leave that
            // answer in the pipe for the next call, and every later call would read the answer
            // meant for the one before it.
            Some(Ok(Line::Text(_) | Line::Over)) if reused => ApprovalDecision::denied(format!(
                "`{name}` was put to the process that launched this run under call id {}, which \
                 was already decided in this run; a reused id is refused whatever the answer, \
                 {NO_RETRY}",
                quoted(call_id)
            )),
            Some(Ok(Line::Text(line))) => judge(&line, call_id, name),
            Some(Ok(Line::Over)) => ApprovalDecision::denied(format!(
                "`{name}` was put to the process that launched this run and it answered with a \
                 line over the {MAX_DECISION_LINE_BYTES}-byte bound for one decision, which was \
                 discarded unread, {NO_RETRY}"
            )),
            Some(Ok(Line::End)) => self.closed(name, None),
            Some(Err(error)) => self.closed(name, Some(&error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_wire::{Approval, CallId, Envelope, ToolName};
    use serde_json::json;
    use std::io::Cursor;

    fn spec(name: &str) -> ToolSpec {
        ToolSpec {
            name: ToolName::new(name).expect("a valid tool name"),
            description: "entry under test".to_owned(),
            input_schema: json!({"type": "object"}),
            approval: Approval::Required,
            envelope: Envelope::default(),
        }
    }

    fn call(id: &str) -> ToolCall {
        ToolCall {
            call_id: CallId::new(id).expect("a valid call id"),
            name: ToolName::new("file_write").expect("a valid tool name"),
            arguments: json!({"path": "a.txt", "text": "hi"}),
        }
    }

    fn caller(input: &[u8]) -> Caller<Cursor<Vec<u8>>, Cursor<Vec<u8>>> {
        Caller::over(Cursor::new(input.to_vec()), Cursor::new(Vec::new()))
    }

    fn noticed<R: BufRead>(caller: &Caller<R, Cursor<Vec<u8>>>) -> String {
        String::from_utf8(caller.notices.get_ref().clone()).expect("notices are utf-8")
    }

    fn reason(decision: ApprovalDecision) -> String {
        match decision {
            ApprovalDecision::Denied { reason } => reason,
            other => panic!("expected a denial, got {other:?}"),
        }
    }

    /// Every refusal names the entry and tells the model a retry cannot help.
    fn refuses(decision: ApprovalDecision, naming: &str) -> String {
        let reason = reason(decision);
        assert!(reason.contains("`file_write`"), "names the entry: {reason}");
        assert!(
            reason.contains("retrying cannot approve it"),
            "says a retry cannot help: {reason}"
        );
        assert!(reason.contains(naming), "names {naming:?}: {reason}");
        reason
    }

    #[test]
    fn an_allow_for_this_call_approves_it() {
        let decision = judge(
            br#"{"call_id":"call-1","decision":"allow"}"#,
            "call-1",
            "file_write",
        );
        assert!(decision.is_approved(), "{decision:?}");
    }

    #[test]
    fn a_deny_carries_the_callers_reason_to_the_model() {
        let reason = refuses(
            judge(
                br#"{"call_id":"call-1","decision":"deny","reason":"not in this repository"}"#,
                "call-1",
                "file_write",
            ),
            "not in this repository",
        );
        assert!(
            reason.contains("process that launched this run"),
            "{reason}"
        );
    }

    #[test]
    fn an_allow_for_another_call_is_refused_by_name() {
        refuses(
            judge(
                br#"{"call_id":"call-2","decision":"allow"}"#,
                "call-1",
                "file_write",
            ),
            "another call",
        );
    }

    #[test]
    fn a_line_that_is_not_json_is_refused_as_not_a_decision() {
        refuses(
            judge(b"yes please", "call-1", "file_write"),
            "not a decision",
        );
        refuses(judge(b"", "call-1", "file_write"), "not a decision");
        refuses(
            judge(&[0xff, 0xfe], "call-1", "file_write"),
            "not a decision",
        );
    }

    #[test]
    fn an_unknown_field_is_refused_rather_than_ignored() {
        refuses(
            judge(
                br#"{"call_id":"call-1","decision":"allow","scope":"always"}"#,
                "call-1",
                "file_write",
            ),
            "scope",
        );
    }

    #[test]
    fn a_decision_missing_its_call_id_is_refused() {
        refuses(
            judge(br#"{"decision":"allow"}"#, "call-1", "file_write"),
            "call_id",
        );
    }

    #[test]
    fn replace_abstain_and_every_other_word_are_not_decisions_this_harness_takes() {
        for word in ["replace", "abstain", "Allow", "yes", ""] {
            let line = json!({"call_id": "call-1", "decision": word}).to_string();
            refuses(
                judge(line.as_bytes(), "call-1", "file_write"),
                "not a decision this harness takes",
            );
        }
    }

    #[test]
    fn a_deny_without_a_reason_is_refused_by_name() {
        for line in [
            json!({"call_id": "call-1", "decision": "deny"}),
            json!({"call_id": "call-1", "decision": "deny", "reason": ""}),
            json!({"call_id": "call-1", "decision": "deny", "reason": "  \t"}),
            json!({"call_id": "call-1", "decision": "deny", "reason": null}),
        ] {
            refuses(
                judge(line.to_string().as_bytes(), "call-1", "file_write"),
                "without a reason",
            );
        }
    }

    #[test]
    fn an_allow_carrying_a_reason_is_not_the_allow_shape_and_is_refused() {
        refuses(
            judge(
                br#"{"call_id":"call-1","decision":"allow","reason":"fine"}"#,
                "call-1",
                "file_write",
            ),
            "only a `deny` carries",
        );
    }

    #[test]
    fn a_decision_that_is_not_an_object_is_refused() {
        refuses(
            judge(b"[\"allow\"]", "call-1", "file_write"),
            "not a decision",
        );
        refuses(
            judge(b"\"allow\"", "call-1", "file_write"),
            "not a decision",
        );
    }

    #[test]
    fn a_caller_value_quoted_back_is_bounded_and_inert() {
        let word = format!("{}\u{1b}[2J", "x".repeat(500));
        let line = json!({"call_id": "call-1", "decision": word}).to_string();
        let reason = refuses(
            judge(line.as_bytes(), "call-1", "file_write"),
            "not a decision this harness takes",
        );
        assert!(!reason.contains('\u{1b}'), "{reason:?}");
        assert!(
            reason.contains("characters)"),
            "the cut is counted: {reason}"
        );
        assert!(reason.len() < 1000, "{} bytes: {reason}", reason.len());
    }

    #[test]
    fn decide_reads_exactly_one_line_per_call() {
        let mut approver = caller(
            b"{\"call_id\":\"call-1\",\"decision\":\"allow\"}\n\
              {\"call_id\":\"call-2\",\"decision\":\"deny\",\"reason\":\"no\"}\n",
        );
        let spec = spec("file_write");
        assert!(approver.decide(&call("call-1"), &spec).is_approved());
        refuses(approver.decide(&call("call-2"), &spec), "no");
        assert!(!approver.is_closed());
    }

    #[test]
    fn a_crlf_line_and_a_last_line_without_a_newline_are_lines() {
        let mut approver = caller(
            b"{\"call_id\":\"call-1\",\"decision\":\"allow\"}\r\n\
              {\"call_id\":\"call-2\",\"decision\":\"allow\"}",
        );
        let spec = spec("file_write");
        assert!(approver.decide(&call("call-1"), &spec).is_approved());
        assert!(approver.decide(&call("call-2"), &spec).is_approved());
    }

    #[test]
    fn end_of_input_denies_this_call_and_every_later_one_and_says_so_once() {
        let mut approver = caller(b"");
        let spec = spec("file_write");
        refuses(approver.decide(&call("call-1"), &spec), "stdin has closed");
        assert!(approver.is_closed());
        refuses(approver.decide(&call("call-2"), &spec), "stdin has closed");
        let notices = noticed(&approver);
        assert_eq!(notices.lines().count(), 1, "said once: {notices:?}");
        assert!(notices.contains("refused without reading"), "{notices}");
    }

    #[test]
    fn after_end_of_input_nothing_more_is_read() {
        // A reader that would answer `allow` if asked again: once closed, it is not asked.
        struct Reopening {
            calls: u8,
        }
        impl std::io::Read for Reopening {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                self.calls += 1;
                if self.calls == 1 {
                    return Ok(0);
                }
                let line = b"{\"call_id\":\"call-2\",\"decision\":\"allow\"}\n";
                buffer[..line.len()].copy_from_slice(line);
                Ok(line.len())
            }
        }
        let mut approver = Caller::over(
            std::io::BufReader::new(Reopening { calls: 0 }),
            Cursor::new(Vec::new()),
        );
        let spec = spec("file_write");
        assert!(!approver.decide(&call("call-1"), &spec).is_approved());
        assert!(
            !approver.decide(&call("call-2"), &spec).is_approved(),
            "a closed stdin is never read again"
        );
    }

    #[test]
    fn a_line_over_the_bound_is_refused_by_name_and_the_next_line_is_still_read() {
        let mut input = Vec::new();
        input.extend_from_slice(br#"{"call_id":"call-1","decision":"deny","reason":""#);
        input.extend(std::iter::repeat_n(b'x', MAX_DECISION_LINE_BYTES));
        input.extend_from_slice(b"\"}\n");
        input.extend_from_slice(b"{\"call_id\":\"call-2\",\"decision\":\"allow\"}\n");
        let mut approver = caller(&input);
        let spec = spec("file_write");
        refuses(
            approver.decide(&call("call-1"), &spec),
            &MAX_DECISION_LINE_BYTES.to_string(),
        );
        assert!(
            approver.decide(&call("call-2"), &spec).is_approved(),
            "the rest of the oversized line was discarded, not read as the next decision"
        );
    }

    #[test]
    fn a_line_exactly_at_the_bound_is_read() {
        let head = br#"{"call_id":"call-1","decision":"deny","reason":""#;
        let tail = b"\"}\n";
        let filler = MAX_DECISION_LINE_BYTES - head.len() - tail.len();
        let mut input = head.to_vec();
        input.extend(std::iter::repeat_n(b'x', filler));
        input.extend_from_slice(tail);
        assert_eq!(input.len(), MAX_DECISION_LINE_BYTES);
        let mut approver = caller(&input);
        let reason = reason(approver.decide(&call("call-1"), &spec("file_write")));
        assert!(reason.contains("xxxx"), "the caller's reason, read whole");
    }

    #[test]
    fn an_oversized_last_line_without_a_newline_is_refused_and_then_input_has_ended() {
        let input = vec![b'x'; MAX_DECISION_LINE_BYTES + 10];
        let mut approver = caller(&input);
        let spec = spec("file_write");
        refuses(approver.decide(&call("call-1"), &spec), "bound");
        refuses(approver.decide(&call("call-2"), &spec), "stdin has closed");
    }

    #[test]
    fn a_call_id_already_decided_is_refused_by_name_and_its_answer_consumed() {
        let mut approver = caller(
            b"{\"call_id\":\"call-1\",\"decision\":\"allow\"}\n\
              {\"call_id\":\"call-1\",\"decision\":\"allow\"}\n\
              {\"call_id\":\"call-2\",\"decision\":\"allow\"}\n",
        );
        let spec = spec("file_write");
        assert!(approver.decide(&call("call-1"), &spec).is_approved());
        refuses(
            approver.decide(&call("call-1"), &spec),
            "call id `call-1`, which was already decided in this run; a reused id is refused",
        );
        // The reused id's answer was read and discarded, so the next call reads its own answer
        // and the caller and the harness stay one line per call in step.
        assert!(approver.decide(&call("call-2"), &spec).is_approved());
    }

    /// A reader that never answers, as a caller that has not written yet.
    struct Silent(mpsc::Receiver<()>);

    impl std::io::Read for Silent {
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
            let _ = self.0.recv();
            Ok(0)
        }
    }

    #[test]
    fn an_interrupt_ends_the_wait_on_the_caller_and_refuses_by_name() {
        let (_keep, never) = mpsc::channel();
        let cancel = LoopCancel::new();
        let mut approver = Caller::interruptible(
            std::io::BufReader::new(Silent(never)),
            Cursor::new(Vec::new()),
            cancel.clone(),
        );
        let interrupter = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            cancel.cancel();
        });
        let started = std::time::Instant::now();
        refuses(
            approver.decide(&call("call-1"), &spec("file_write")),
            "the run was interrupted",
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        interrupter.join().expect("the interrupter ran");
        refuses(
            approver.decide(&call("call-2"), &spec("file_write")),
            "the run was interrupted",
        );
        assert!(
            !approver.is_closed(),
            "an interrupt is not the end of input"
        );
    }

    #[test]
    fn a_run_already_interrupted_reads_no_decision() {
        let cancel = LoopCancel::new();
        cancel.cancel();
        let mut approver = Caller::interruptible(
            Cursor::new(b"{\"call_id\":\"call-1\",\"decision\":\"allow\"}\n".to_vec()),
            Cursor::new(Vec::new()),
            cancel,
        );
        refuses(
            approver.decide(&call("call-1"), &spec("file_write")),
            "the run was interrupted",
        );
    }
}
