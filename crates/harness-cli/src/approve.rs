//! Asking the person at the terminal about one call at a time.
//!
//! Until this existed the command had two settings: `--yes`, which approves everything, and the
//! default, which refuses everything. Neither is supervision — the first removes the gate for the
//! whole run because one write was wanted, and the second means a run that needs a single approved
//! write cannot be done at all. What a person actually wants is to approve *this* write and be
//! asked again about the next one.
//!
//! Two properties of the gate are kept exactly as they were. The decision is a blocking call made
//! **before** the effect, so an answer cannot arrive after the fact. And what is named to the
//! person is the **entry** the loop resolved — `file_write`, `run` — never the verb `tool_invoke`
//! it travelled through, because a person asked to approve `tool_invoke` is being asked to approve
//! everything the catalogue can do (`harness_tools::Verbs::invoked`).
//!
//! What is *not* here is a default: nothing in this module decides that a run should be
//! interactive. [`stdio_is_interactive`] answers the question the caller asks before choosing, and
//! [`Terminal::open`] fails **by name** so a caller that falls back to `DenyAll` can say why it
//! did. A harness that silently fell back to asking nobody would be the decorative gate again.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufRead, BufReader, IsTerminal, Write};

use harness_loop::{ApprovalDecision, ApprovalPort};
use harness_wire::{ToolCall, ToolSpec};
use serde_json::Value;

/// How much of an argument blob the prompt shows before it says, in the prompt, that it stopped.
///
/// A person cannot read a 40 KiB `file_write` body at a prompt, and a prompt that scrolled the
/// decision off the screen is a prompt that gets answered `y` blindly. The cut is always stated
/// with both figures, never silent (`AGENTS.md` invariant 8).
const MAX_ARGUMENT_CHARS: usize = 400;

/// How many answers that are neither yes, no nor always are read before the call is denied.
///
/// Bounded because the reader may be something that answers forever without ever answering the
/// question — a pipe of noise, a terminal emulator replaying a buffer. An unbounded re-prompt
/// would hang the run there with no turn budget being spent and nothing on screen changing.
const MAX_UNREADABLE_ANSWERS: u8 = 3;

/// The `run` catalogue entry, whose interesting argument is an argv rather than a path.
const RUN_ENTRY: &str = "run";
/// The `file_write` catalogue entry: a whole-file replacement, so the size is what matters.
const FILE_WRITE_ENTRY: &str = "file_write";
/// The `file_edit` catalogue entry: a replacement of one exact string by another.
const FILE_EDIT_ENTRY: &str = "file_edit";

/// How many lines of each side of an edit the prompt shows.
const EDIT_PREVIEW_LINES: usize = 3;

/// An approver that asks a person, remembers what they said `always` to, and refuses when nobody
/// is there.
///
/// Generic over its reader and writer so the tests drive it with [`std::io::Cursor`]s: an approval
/// gate whose only test is a person typing at it is a gate with no tests. [`Terminal::open`]
/// builds the real one over `/dev/tty`.
pub struct Terminal<R: BufRead, W: Write> {
    reader: R,
    writer: W,
    /// Entry names a person said `always` to, for this process only.
    ///
    /// Session-scoped and never persisted: an `always` that outlived the run would approve calls
    /// in a later run that nobody was watching, which is the `--yes` problem wearing a memory.
    always: BTreeSet<String>,
    /// Whether the reader has ended. Once it has, no later call prompts.
    gone: bool,
}

impl<R: BufRead, W: Write> Terminal<R, W> {
    /// An approver over an arbitrary reader and writer.
    pub fn over(reader: R, writer: W) -> Self {
        Self {
            reader,
            writer,
            always: BTreeSet::new(),
            gone: false,
        }
    }

    /// Entry names this session has been told to stop asking about.
    pub fn always(&self) -> impl Iterator<Item = &str> {
        self.always.iter().map(String::as_str)
    }

    /// Whether the reader has ended, so every further call is refused without asking.
    pub fn is_gone(&self) -> bool {
        self.gone
    }

    /// Records that nobody is answering, says so once, and denies.
    fn terminal_is_gone(&mut self, name: &str) -> ApprovalDecision {
        if !self.gone {
            self.gone = true;
            let _ = writeln!(
                self.writer,
                "nothing is answering the approval prompt; every further call is refused without \
                 asking."
            );
            let _ = self.writer.flush();
        }
        ApprovalDecision::denied(format!(
            "`{name}` needs a person's decision and the terminal that would give one has gone \
             away, so retrying cannot approve it either; do what can be done without it and say \
             what could not"
        ))
    }
}

impl Terminal<BufReader<File>, File> {
    /// Opens the controlling terminal, so a person is asked even when stdin and stdout are pipes.
    ///
    /// `/dev/tty` rather than stdin on purpose: the interesting runs are the ones whose input is a
    /// heredoc and whose output is being read by something else, and a prompt written to a
    /// redirected stdout is a prompt nobody sees and nobody can answer. The file is opened once
    /// read **and** write and the handle duplicated, so the question and the answer travel over
    /// the same terminal.
    ///
    /// # Errors
    ///
    /// Returns the reason `/dev/tty` could not be opened — no controlling terminal, a platform
    /// without one — so the caller can fall back to `DenyAll` and **tell the person** that this
    /// run will refuse every call rather than leaving them to discover it from a refusal.
    pub fn open() -> Result<Self, String> {
        let tty = File::options()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .map_err(|error| {
                format!("opening `/dev/tty` to ask a person about each call: {error}")
            })?;
        let writer = tty.try_clone().map_err(|error| {
            format!("duplicating the `/dev/tty` handle to write prompts to it: {error}")
        })?;
        Ok(Self::over(BufReader::new(tty), writer))
    }
}

impl<R: BufRead, W: Write> ApprovalPort for Terminal<R, W> {
    fn decide(&mut self, call: &ToolCall, spec: &ToolSpec) -> ApprovalDecision {
        let name = spec.name.as_str();
        if self.always.contains(name) {
            return ApprovalDecision::Approved;
        }
        if self.gone {
            return self.terminal_is_gone(name);
        }
        let description = describe(call, spec);
        let _ = writeln!(self.writer, "\n{description}");
        let mut unreadable: u8 = 0;
        loop {
            let _ = write!(
                self.writer,
                "approve? [y]es once · [n]o · [a]lways for `{name}` > "
            );
            let _ = self.writer.flush();
            let mut line = String::new();
            match self.reader.read_line(&mut line) {
                Ok(0) | Err(_) => return self.terminal_is_gone(name),
                Ok(_) => {}
            }
            match line.trim().to_ascii_lowercase().as_str() {
                "y" | "yes" => return ApprovalDecision::Approved,
                "a" | "always" => {
                    self.always.insert(name.to_owned());
                    return ApprovalDecision::Approved;
                }
                // An empty line is a refusal, not an unreadable answer: the safe key is the one a
                // person hits when they did not follow what they were being asked.
                "" | "n" | "no" => {
                    return ApprovalDecision::denied(format!(
                        "`{name}` was put to a person and refused, so retrying cannot approve it \
                         either; do what can be done without it and say what could not"
                    ));
                }
                _ => {
                    unreadable = unreadable.saturating_add(1);
                    if unreadable >= MAX_UNREADABLE_ANSWERS {
                        return ApprovalDecision::denied(format!(
                            "`{name}` needs a person's decision and \
                             {MAX_UNREADABLE_ANSWERS} answers came back that were none of yes, no \
                             or always, so retrying cannot approve it either; do what can be done \
                             without it and say what could not"
                        ));
                    }
                    let _ = writeln!(self.writer, "answer `y`, `n` or `a`.");
                }
            }
        }
    }
}

/// Whether both ends of a person's attention are a terminal.
///
/// Stdin **and** stderr, because the caller's fallback needs both: stderr is where the run's
/// progress is already written, and a run whose stdin is a pipe is a run being driven by a
/// program. Only the caller can decide what to do with the answer — this module never chooses.
pub fn stdio_is_interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

/// The arguments of the **entry**, whichever surface the call came over.
///
/// Under the three verbs the real arguments are nested one level down inside `tool_invoke`; under
/// a flat surface they are the call's own. Both are handled here rather than at each use so that
/// publishing tools flat one day changes nothing a person sees.
fn entry_arguments(call: &ToolCall) -> &Value {
    static NONE: Value = Value::Null;
    if call.name.as_str() == harness_tools::INVOKE_VERB {
        call.arguments.get("arguments").unwrap_or(&NONE)
    } else {
        &call.arguments
    }
}

/// One line — or a few — naming what is about to happen, in the entry's own vocabulary.
///
/// An entry this function does not know about falls back to its arguments as JSON rather than to
/// nothing: a new catalogue entry must show up at the prompt as *something*, and the failure mode
/// of a silent default is a person approving a call whose effect was never displayed.
fn describe(call: &ToolCall, spec: &ToolSpec) -> String {
    let name = spec.name.as_str();
    let arguments = entry_arguments(call);
    let path = arguments.get("path").and_then(Value::as_str);
    match name {
        FILE_WRITE_ENTRY => {
            if let Some(path) = path
                && let Some(text) = arguments.get("text").and_then(Value::as_str)
            {
                return format!("{name}  {path}  ({} bytes)", text.len());
            }
        }
        FILE_EDIT_ENTRY => {
            if let Some(path) = path
                && let Some(old) = arguments.get("old").and_then(Value::as_str)
                && let Some(new) = arguments.get("new").and_then(Value::as_str)
            {
                return format!(
                    "{name}  {path}\n{}{}",
                    preview(old, "- "),
                    preview(new, "+ ")
                );
            }
        }
        RUN_ENTRY => {
            if let Some(argv) = arguments.get("argv").and_then(Value::as_array) {
                let words: Vec<&str> = argv.iter().filter_map(Value::as_str).collect();
                return format!("{name}  {}", bounded(&words.join(" ")));
            }
        }
        _ => {
            if let Some(path) = path {
                return format!("{name}  {path}");
            }
        }
    }
    format!("{name}  {}", bounded(&arguments.to_string()))
}

/// The first few lines of one side of an edit, each prefixed, saying how many were left out.
fn preview(text: &str, marker: &str) -> String {
    let mut rendered = String::new();
    let total = text.lines().count();
    for line in text.lines().take(EDIT_PREVIEW_LINES) {
        rendered.push_str("  ");
        rendered.push_str(marker);
        rendered.push_str(&bounded(line));
        rendered.push('\n');
    }
    if total > EDIT_PREVIEW_LINES {
        let hidden = total - EDIT_PREVIEW_LINES;
        let _ = writeln!(rendered, "  {marker}… ({hidden} more lines)");
    }
    rendered
}

/// Text bounded for a prompt, with the cut visible and counted.
///
/// Counted in characters rather than bytes so the cut lands on a character boundary; the figures
/// are stated because a bound that is not stated reads exactly like a complete value.
fn bounded(text: &str) -> String {
    let total = text.chars().count();
    if total <= MAX_ARGUMENT_CHARS {
        return text.to_owned();
    }
    let kept: String = text.chars().take(MAX_ARGUMENT_CHARS).collect();
    format!("{kept}…  (first {MAX_ARGUMENT_CHARS} of {total} characters)")
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

    /// A call over the three verbs: the entry's arguments sit one level down.
    fn invoke(entry: &str, arguments: &Value) -> ToolCall {
        ToolCall {
            call_id: CallId::new("call-1").expect("a valid call id"),
            name: ToolName::new(harness_tools::INVOKE_VERB).expect("a valid tool name"),
            arguments: json!({"name": entry, "arguments": arguments}),
        }
    }

    /// A call over a flat surface: the entry's arguments are the call's own.
    fn flat(entry: &str, arguments: Value) -> ToolCall {
        ToolCall {
            call_id: CallId::new("call-2").expect("a valid call id"),
            name: ToolName::new(entry).expect("a valid tool name"),
            arguments,
        }
    }

    fn terminal(answers: &'static str) -> Terminal<Cursor<&'static [u8]>, Cursor<Vec<u8>>> {
        Terminal::over(Cursor::new(answers.as_bytes()), Cursor::new(Vec::new()))
    }

    fn shown<R: BufRead>(terminal: &Terminal<R, Cursor<Vec<u8>>>) -> String {
        String::from_utf8(terminal.writer.get_ref().clone()).expect("prompts are utf-8")
    }

    #[test]
    fn the_real_terminal_is_named_without_its_reader_and_writer_and_says_why_it_could_not_open() {
        // Pins the spelling a caller uses — `Terminal::open()`, no type arguments — and that a
        // machine without a controlling terminal gets a reason it can pass on rather than a
        // silent fall back to approving nothing.
        match Terminal::open() {
            Ok(terminal) => assert!(!terminal.is_gone()),
            Err(error) => assert!(error.contains("/dev/tty"), "{error}"),
        }
    }

    #[test]
    fn yes_approves_this_call_and_the_next_one_is_asked_about_again() {
        let mut approver = terminal("y\ny\n");
        let call = invoke("file_write", &json!({"path": "a.txt", "text": "hi"}));
        let spec = spec("file_write");
        assert!(approver.decide(&call, &spec).is_approved());
        assert!(approver.decide(&call, &spec).is_approved());
        assert_eq!(
            shown(&approver).matches("approve?").count(),
            2,
            "a `yes` approves once and the second call is asked about again"
        );
        assert!(approver.always().next().is_none());
    }

    #[test]
    fn always_stops_asking_about_that_entry_and_still_asks_about_another() {
        let mut approver = terminal("a\nn\n");
        let write = spec("file_write");
        let call = invoke("file_write", &json!({"path": "a.txt", "text": "hi"}));
        assert!(approver.decide(&call, &write).is_approved());
        assert!(approver.decide(&call, &write).is_approved());
        assert_eq!(
            shown(&approver).matches("approve?").count(),
            1,
            "the second `file_write` is not asked about"
        );
        assert_eq!(approver.always().collect::<Vec<_>>(), vec!["file_write"]);

        let run = spec("run");
        let decision = approver.decide(&invoke("run", &json!({"argv": ["ls"]})), &run);
        assert!(
            !decision.is_approved(),
            "another entry is still asked about"
        );
        assert_eq!(shown(&approver).matches("approve?").count(), 2);
    }

    #[test]
    fn no_denies_and_says_that_retrying_cannot_help() {
        let mut approver = terminal("n\n");
        let decision = approver.decide(
            &invoke("file_write", &json!({"path": "a.txt", "text": "hi"})),
            &spec("file_write"),
        );
        let ApprovalDecision::Denied { reason } = decision else {
            panic!("a `no` denies");
        };
        assert!(reason.contains("file_write"), "{reason}");
        assert!(reason.contains("retrying cannot approve it"), "{reason}");
    }

    #[test]
    fn an_empty_answer_denies() {
        let mut approver = terminal("\n");
        let decision = approver.decide(&flat("run", json!({"argv": ["ls"]})), &spec("run"));
        assert!(!decision.is_approved(), "the safe key denies");
        assert!(
            !approver.is_gone(),
            "an empty line is an answer, not an end"
        );
    }

    #[test]
    fn end_of_input_denies_and_stops_prompting() {
        let mut approver = terminal("");
        let spec = spec("file_write");
        let call = invoke("file_write", &json!({"path": "a.txt", "text": "hi"}));
        let first = approver.decide(&call, &spec);
        assert!(!first.is_approved());
        assert!(approver.is_gone());
        let after_first = shown(&approver).len();

        let second = approver.decide(&call, &spec);
        assert!(!second.is_approved());
        assert_eq!(
            shown(&approver).len(),
            after_first,
            "once the terminal is gone nothing more is written to it"
        );
        let ApprovalDecision::Denied { reason } = second else {
            panic!("a gone terminal denies");
        };
        assert!(reason.contains("retrying cannot approve it"), "{reason}");
    }

    #[test]
    fn three_unreadable_answers_deny_by_name() {
        let mut approver = terminal("what\nhuh\nsorry\ny\n");
        let decision = approver.decide(
            &invoke("file_write", &json!({"path": "a.txt", "text": "hi"})),
            &spec("file_write"),
        );
        let ApprovalDecision::Denied { reason } = decision else {
            panic!("unreadable answers deny");
        };
        assert!(reason.contains("file_write"), "{reason}");
        assert_eq!(
            shown(&approver).matches("approve?").count(),
            3,
            "the answer is asked for three times and then the call is denied"
        );
    }

    #[test]
    fn an_unreadable_answer_is_asked_about_again_rather_than_guessed() {
        let mut approver = terminal("maybe\ny\n");
        let decision = approver.decide(
            &invoke("file_write", &json!({"path": "a.txt", "text": "hi"})),
            &spec("file_write"),
        );
        assert!(decision.is_approved());
        assert!(shown(&approver).contains("answer `y`, `n` or `a`."));
    }

    #[test]
    fn the_prompt_names_the_entry_and_its_path_not_the_verb_it_came_through() {
        let mut approver = terminal("n\n");
        let _ = approver.decide(
            &invoke(
                "file_write",
                &json!({"path": "src/main.rs", "text": "fn main() {}"}),
            ),
            &spec("file_write"),
        );
        let prompt = shown(&approver);
        assert!(prompt.contains("file_write"), "{prompt}");
        assert!(prompt.contains("src/main.rs"), "{prompt}");
        assert!(prompt.contains("12 bytes"), "{prompt}");
        assert!(
            !prompt.contains(harness_tools::INVOKE_VERB),
            "a person decides on the entry, never on the verb: {prompt}"
        );
    }

    #[test]
    fn the_prompt_for_a_run_shows_the_argv() {
        let mut approver = terminal("n\n");
        let _ = approver.decide(
            &invoke("run", &json!({"argv": ["cargo", "test", "--locked"]})),
            &spec("run"),
        );
        assert!(shown(&approver).contains("cargo test --locked"));
    }

    #[test]
    fn the_prompt_for_an_edit_shows_both_sides_and_counts_what_it_left_out() {
        let mut approver = terminal("n\n");
        let _ = approver.decide(
            &invoke(
                "file_edit",
                &json!({"path": "a.rs", "old": "one\ntwo\nthree\nfour", "new": "ONE"}),
            ),
            &spec("file_edit"),
        );
        let prompt = shown(&approver);
        assert!(prompt.contains("- one"), "{prompt}");
        assert!(prompt.contains("- three"), "{prompt}");
        assert!(prompt.contains("+ ONE"), "{prompt}");
        assert!(prompt.contains("1 more lines"), "{prompt}");
    }

    #[test]
    fn a_flat_call_is_described_from_its_own_arguments() {
        let mut approver = terminal("n\n");
        let _ = approver.decide(
            &flat("file_read", json!({"path": "README.md"})),
            &spec("file_read"),
        );
        assert!(shown(&approver).contains("file_read  README.md"));
    }

    #[test]
    fn an_oversized_argument_blob_says_how_much_of_it_is_shown() {
        let mut approver = terminal("n\n");
        let long = "x".repeat(MAX_ARGUMENT_CHARS * 2);
        let _ = approver.decide(&invoke("mystery", &json!({"blob": long})), &spec("mystery"));
        let prompt = shown(&approver);
        assert!(prompt.contains('…'), "the cut is visible: {prompt}");
        assert!(
            prompt.contains(&format!("first {MAX_ARGUMENT_CHARS} of")),
            "the cut is counted: {prompt}"
        );
    }
}
