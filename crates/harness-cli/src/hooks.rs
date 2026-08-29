//! The operator's own programs, run at the three moments `harness_loop` consults a hook.
//!
//! # Declared, never discovered
//!
//! The file is named on the command line — `--hooks <FILE>` — and nothing here ever looks for one
//! in the workspace. A hook found in a repository would be a program the *repository* runs on the
//! operator's machine, which is the ambient fallback the safety envelope forbids for credentials,
//! and the argument is the same (design 0002 § 3).
//!
//! # A hook can only narrow
//!
//! `before-call` fires **after** the approver said yes, so a block here is one more refusal and
//! never an approval. Nothing in this module can change a call, add a tool, or reach one the run
//! did not publish.
//!
//! # An argv, never a shell
//!
//! `command` is a list of words, spawned directly, exactly as the `run` catalogue entry is: a
//! command line assembled as one string is a shell with extra steps. The hook is otherwise
//! **unconfined** — it is the operator's own program, running on the operator's machine, with the
//! environment the harness was started in — **except for this run's own credential**. The variable
//! `--api-key-env` or `--oauth-token-env` named is removed before the spawn
//! ([`Hooks::without_env`]): a hook is trusted to act, not to be handed the key this run
//! authenticates with, and one that printed its environment would otherwise print it.
//!
//! # The protocol
//!
//! One JSON document on stdin, one exit status back:
//!
//! ```text
//! { "hook": "before-call" | "after-call" | "stop",
//!   "call": { "call_id": …, "name": …, "arguments": … },   // at a call point
//!   "entry": "file_write",                                  // the invoked entry, never the verb
//!   "outcome": { "output": …, "failed": false },            // after-call only
//!   "text": "…",                                            // stop only: what the run would answer
//!   "workspace": "/abs/path" }
//! ```
//!
//! `0` proceeds — and at `after-call` a `{"note": "…"}` on stdout is what the model reads beside
//! the result. `2` blocks, with the reason from `{"reason": "…"}` on stdout, else from stderr.
//! Any other status, a program that could not be started, more than [`MAX_HOOK_STDOUT_BYTES`] on
//! stdout, a hook still running at [`HOOK_TIMEOUT`], or one that exited and left its output pipe
//! held open past it is a [`HookDecision::Failed`] naming the program — which the loop reads as
//! *fail closed* before a call and *fail open* at a stop. At `after-call` nothing can be refused
//! any more, so a failure there is reported both ways: the model reads it as a note, and the
//! record carries it as the point's decision ([`AfterCall`]).

use std::fs;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use harness_loop::{AfterCall, HookDecision, HookPoint, HookPort};
use harness_wire::{ToolCall, ToolOutcome, ToolSpec};
use serde::Deserialize;
use serde_json::{Value, json};

/// The file format this build reads. A file declaring anything else is refused by name.
pub const HOOKS_VERSION: u32 = 1;

/// How long a hook may take before it is killed and its decision is [`HookDecision::Failed`].
///
/// It bounds the **whole consultation** and not merely the wait: a hook that exits while something
/// it started still holds the output pipe is past this bound exactly as one that never exits is.
///
/// The port is not told the run's remaining wall clock: the loop's deadline check between calls is
/// what bounds the overshoot, exactly as it is for a tool call.
pub const HOOK_TIMEOUT: Duration = Duration::from_secs(60);

/// How much a hook may write on stdout before its answer is refused rather than cut.
///
/// A truncated `{"reason": …}` reads to this code exactly like a complete one, and the reason is
/// what a person sees for a refused call (`AGENTS.md` invariant 8).
pub const MAX_HOOK_STDOUT_BYTES: usize = 16 * 1024;

/// How often a running hook is looked at. The same slice the tool runner waits in.
const POLL: Duration = Duration::from_millis(25);

/// The file as it is written, before anything in it has been checked.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    hooks: Vec<Declared>,
}

/// One declaration, still carrying the operator's own spelling of the point.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Declared {
    on: String,
    #[serde(default)]
    tools: Option<Vec<String>>,
    command: Vec<String>,
}

/// One hook: where it fires, which entries it fires for, and the argv that answers.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Hook {
    point: HookPoint,
    /// The **invoked entry** names this hook is about — `file_write`, never `tool_invoke`.
    /// [`None`] is every call.
    tools: Option<Vec<String>>,
    command: Vec<String>,
}

impl Hook {
    /// Whether this hook speaks about `entry` at `point`.
    fn matches(&self, point: HookPoint, entry: Option<&str>) -> bool {
        if self.point != point {
            return false;
        }
        match (&self.tools, entry) {
            (None, _) => true,
            (Some(names), Some(entry)) => names.iter().any(|name| name == entry),
            // Refused at load, so unreachable from a file; a hook that filtered by tool at a point
            // with no call would otherwise silently never fire.
            (Some(_), None) => false,
        }
    }

    /// The program, for a message that names what could not be run.
    fn program(&self) -> &str {
        self.command.first().map_or("", String::as_str)
    }
}

/// What one hook said, before a point decides what that means.
///
/// `after-call` cannot block and cannot fail the outcome, so it reads all three as notes — and
/// reports a `Failed` as its recorded decision too, because a hook that never ran is not something
/// only the model should learn. The two deciding points read them as the design's table says.
enum Spoken {
    Proceed { note: Option<String> },
    Block { reason: String },
    Failed { reason: String },
}

/// The operator's hooks, ready to be consulted.
///
/// Several declarations may name one point: they run **in the order the file lists them**, and the
/// first `Block` or `Failed` is the answer — a later hook cannot undo an earlier refusal. At
/// `after-call` every one of them runs and their notes are joined.
#[derive(Debug, Clone)]
pub struct Hooks {
    declared: Vec<Hook>,
    workspace: PathBuf,
    timeout: Duration,
    /// Environment variable names no hook may see, because this run reads its credential from them.
    without_env: Vec<String>,
}

impl Hooks {
    /// Reads the file the operator named.
    ///
    /// # Errors
    ///
    /// Names the file and what is wrong with it: a version this build does not read, a point it
    /// does not know, an empty `command`, an empty `tools` list, and anything serde refuses.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path)
            .map_err(|error| format!("reading the hooks file `{}`: {error}", path.display()))?;
        Self::parse(&text, path)
    }

    /// The same, from text already in hand.
    ///
    /// # Errors
    ///
    /// As [`Hooks::load`].
    pub fn parse(text: &str, path: &Path) -> Result<Self, String> {
        let named = |message: String| format!("the hooks file `{}`: {message}", path.display());
        let document: Document =
            serde_json::from_str(text).map_err(|error| named(error.to_string()))?;
        if document.version != HOOKS_VERSION {
            return Err(named(format!(
                "declares version {} and this build reads version {HOOKS_VERSION}",
                document.version
            )));
        }
        let mut hooks = Vec::with_capacity(document.hooks.len());
        for (index, declared) in document.hooks.into_iter().enumerate() {
            let position = index + 1;
            let point = match declared.on.as_str() {
                "before-call" => HookPoint::BeforeCall,
                "after-call" => HookPoint::AfterCall,
                "stop" => HookPoint::Stop,
                other => {
                    return Err(named(format!(
                        "hook {position} fires `{other}`, which is not a hook point; this build \
                         knows `before-call`, `after-call` and `stop`"
                    )));
                }
            };
            if declared.command.is_empty() {
                return Err(named(format!(
                    "hook {position} declares an empty `command`; it must name a program, as an \
                     argv rather than a shell string"
                )));
            }
            if declared.tools.as_ref().is_some_and(Vec::is_empty) {
                return Err(named(format!(
                    "hook {position} declares an empty `tools` list, which would match no call — \
                     omit it to mean every call"
                )));
            }
            if declared.tools.is_some() && point == HookPoint::Stop {
                return Err(named(format!(
                    "hook {position} is a `stop` hook and declares `tools`; a stop is about the \
                     run and not about one call, so nothing would ever match — omit it"
                )));
            }
            hooks.push(Hook {
                point,
                tools: declared.tools,
                command: declared.command,
            });
        }
        Ok(Self {
            declared: hooks,
            workspace: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            timeout: HOOK_TIMEOUT,
            without_env: Vec::new(),
        })
    }

    /// The directory every hook is told this run is working in.
    ///
    /// Absolute, so a hook that resolves a path from it lands where the run does, whatever
    /// directory the hook itself was started in.
    #[must_use]
    pub fn in_workspace(mut self, workspace: &Path) -> Self {
        self.workspace = workspace.canonicalize().unwrap_or_else(|_| {
            std::env::current_dir()
                .map_or_else(|_| workspace.to_path_buf(), |here| here.join(workspace))
        });
        self
    }

    /// A shorter patience, so a test can prove the timeout without waiting a minute for it.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Environment variables to remove before every hook is spawned.
    ///
    /// This run's own credential, in practice: the names `--api-key-env` and `--oauth-token-env`
    /// gave. A hook is otherwise **unconfined** — it is the operator's own program on the
    /// operator's machine — but it is not a place this harness hands its key to. The child would
    /// inherit the whole environment, so a hook that echoed `$ANTHROPIC_API_KEY` into its
    /// `{"note": …}` would put the key in the conversation and in the record.
    ///
    /// Removing the *name* is all that can be done here and all that is claimed: a credential the
    /// operator exported under a second name, or one a hook reads from a file, is outside what this
    /// knows about.
    #[must_use]
    pub fn without_env<S: Into<String>>(mut self, names: impl IntoIterator<Item = S>) -> Self {
        self.without_env.extend(names.into_iter().map(Into::into));
        self.without_env.sort();
        self.without_env.dedup();
        self
    }

    /// How many hooks were declared. A file may legitimately declare none.
    #[must_use]
    pub fn len(&self) -> usize {
        self.declared.len()
    }

    /// Whether the file declared no hooks at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.declared.is_empty()
    }

    /// The document one hook reads on stdin at a call point.
    fn call_document(
        &self,
        point: HookPoint,
        call: &ToolCall,
        invoked: &ToolSpec,
        outcome: Option<&ToolOutcome>,
    ) -> Value {
        let mut document = json!({
            "hook": point.as_str(),
            "call": {
                "call_id": call.call_id.as_str(),
                "name": call.name.as_str(),
                "arguments": call.arguments,
            },
            // The entry, never the verb: a hook declared for `file_write` must not be handed
            // `tool_invoke` and left to dig the real name out of the arguments.
            "entry": invoked.name.as_str(),
            "workspace": self.workspace.display().to_string(),
        });
        if let Some(outcome) = outcome
            && let Some(object) = document.as_object_mut()
        {
            object.insert(
                "outcome".to_owned(),
                json!({"output": outcome.output, "failed": outcome.failed}),
            );
        }
        document
    }

    /// Every hook that speaks at `point` about `entry`, in the order the file listed them.
    fn matching(&self, point: HookPoint, entry: Option<&str>) -> Vec<Hook> {
        self.declared
            .iter()
            .filter(|hook| hook.matches(point, entry))
            .cloned()
            .collect()
    }

    /// Runs each hook in turn and stops at the first one that does not proceed.
    fn decide(&self, hooks: &[Hook], document: &Value) -> HookDecision {
        for hook in hooks {
            match self.speak(hook, document) {
                Spoken::Proceed { .. } => {}
                Spoken::Block { reason } => return HookDecision::Block { reason },
                Spoken::Failed { reason } => return HookDecision::Failed { reason },
            }
        }
        HookDecision::Proceed
    }

    /// Spawns one hook, writes it the document, and reads what it decided.
    ///
    /// # Bounded end to end, not merely at the wait
    ///
    /// [`Hooks::timeout`] is a deadline for the whole consultation. A hook that exits promptly and
    /// leaves a grandchild holding the pipe — `sh -c 'sleep 30 & exit 0'`, or anything that starts
    /// a daemon — closes nothing, and a `join` on a drain thread waits for the *last writer* to
    /// close, not for the hook to exit: the run would stall for the grandchild's whole life, and
    /// for ever behind a daemon. So the drains report over a channel and are collected with
    /// `recv_timeout` against that same deadline. Past it the child is killed, the threads are
    /// abandoned — they end by themselves when the pipe finally closes, and hold nothing this
    /// process needs — and the answer is a failure naming the program and the bound.
    fn speak(&self, hook: &Hook, document: &Value) -> Spoken {
        let program = hook.program();
        let deadline = Instant::now() + self.timeout;
        let mut command = Command::new(program);
        command
            .args(&hook.command[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Unconfined, but never carrying this run's own credential: see `Hooks::without_env`.
        for name in &self.without_env {
            command.env_remove(name);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                return Spoken::Failed {
                    reason: format!("the hook `{program}` could not be started: {error}"),
                };
            }
        };

        // Every one of the three pipes is served on its own thread, and **none of them is joined**.
        // A document larger than a pipe buffer and a hook that answers before reading all of it are
        // the same deadlock from two directions, a `wait` that has not drained stdout is the third,
        // and an unbounded `join` on any of them is the fourth.
        let body = document.to_string();
        let stdin = child.stdin.take();
        std::thread::spawn(move || {
            if let Some(mut stdin) = stdin {
                let _ = stdin.write_all(body.as_bytes());
                let _ = stdin.write_all(b"\n");
                let _ = stdin.flush();
            }
        });
        let out = child.stdout.take();
        let err = child.stderr.take();
        let (reports, drained) = mpsc::channel();
        let stderr_reports = reports.clone();
        std::thread::spawn(move || {
            let _ = reports.send((Stream::Stdout, bounded(out, MAX_HOOK_STDOUT_BYTES)));
        });
        std::thread::spawn(move || {
            let _ = stderr_reports.send((Stream::Stderr, bounded(err, MAX_HOOK_STDOUT_BYTES)));
        });

        let status = match self.wait(&mut child, program, deadline) {
            Ok(status) => status,
            Err(reason) => return Spoken::Failed { reason },
        };
        let (stdout, stderr) = match self.collect(&drained, deadline, program) {
            Ok(pair) => pair,
            Err(reason) => {
                // The hook itself is already gone; whatever it started is not. Kill what can be
                // killed and leave the threads to end when the pipe does.
                let _ = child.kill();
                let _ = child.wait();
                return Spoken::Failed { reason };
            }
        };

        if stdout.over {
            return Spoken::Failed {
                reason: format!(
                    "the hook `{program}` wrote more than {MAX_HOOK_STDOUT_BYTES} bytes on stdout, \
                     and a cut answer reads exactly like a whole one"
                ),
            };
        }
        let said: Option<Value> = serde_json::from_str(stdout.text.trim()).ok();
        let field = |name: &str| -> Option<String> {
            said.as_ref()
                .and_then(|value| value.get(name))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        match status.code() {
            Some(0) => Spoken::Proceed {
                note: field("note"),
            },
            // The first 16 KiB of a reason is still the reason, so a cut one is delivered rather
            // than refused — but it says it was cut, and by how much. A person reading a refusal
            // that stops mid-sentence must not have to wonder whether the hook meant it to
            // (`AGENTS.md` invariant 8).
            Some(2) => Spoken::Block {
                reason: field("reason")
                    .or_else(|| {
                        let trimmed = stderr.text.trim();
                        (!trimmed.is_empty()).then(|| format!("{trimmed}{}", stderr.cut()))
                    })
                    .unwrap_or_else(|| format!("hook `{program}` blocked without a reason")),
            },
            other => Spoken::Failed {
                reason: format!(
                    "the hook `{program}` {}{}",
                    other.map_or_else(
                        || "was killed by a signal".to_owned(),
                        |code| format!("exited {code}, which is neither 0 nor 2")
                    ),
                    describe(&stderr)
                ),
            },
        }
    }

    /// Waits for one hook, killing it at `deadline`.
    ///
    /// # Errors
    ///
    /// A hook that had to be killed, and one this process could not wait for, are both reasons
    /// naming the program.
    fn wait(
        &self,
        child: &mut Child,
        program: &str,
        deadline: Instant,
    ) -> Result<std::process::ExitStatus, String> {
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Ok(status),
                Ok(None) => {}
                Err(error) => return Err(format!("waiting for the hook `{program}`: {error}")),
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "the hook `{program}` was still running after {} ms and was killed",
                    self.timeout.as_millis()
                ));
            }
            std::thread::sleep(POLL);
        }
    }

    /// Collects both drains, or gives up at the same deadline the wait used.
    ///
    /// # Errors
    ///
    /// A pipe still open past the bound: the hook exited, something it started did not, and this
    /// process is not going to wait for it. Names the program and the bound.
    fn collect(
        &self,
        drained: &mpsc::Receiver<(Stream, Drained)>,
        deadline: Instant,
        program: &str,
    ) -> Result<(Drained, Drained), String> {
        let mut stdout = None;
        let mut stderr = None;
        while stdout.is_none() || stderr.is_none() {
            match drained.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok((Stream::Stdout, said)) => stdout = Some(said),
                Ok((Stream::Stderr, said)) => stderr = Some(said),
                Err(_) => {
                    return Err(format!(
                        "the hook `{program}` exited but left its output open past {} ms — \
                         something it started still holds the pipe — so it was abandoned rather \
                         than waited for",
                        self.timeout.as_millis()
                    ));
                }
            }
        }
        Ok((
            stdout.unwrap_or_else(Drained::empty),
            stderr.unwrap_or_else(Drained::empty),
        ))
    }
}

/// Which of a hook's two output pipes a drain thread is reporting.
enum Stream {
    Stdout,
    Stderr,
}

/// What one stream held, whether it held more than it was allowed to, and how much there was.
struct Drained {
    text: String,
    over: bool,
    /// Every byte the stream produced, including the ones dropped. `0` when nothing was read.
    total: u64,
}

impl Drained {
    fn empty() -> Self {
        Self {
            text: String::new(),
            over: false,
            total: 0,
        }
    }

    /// What to append to a message built from this stream, when it was cut.
    ///
    /// Empty when it was not, so it can be appended unconditionally.
    fn cut(&self) -> String {
        if !self.over {
            return String::new();
        }
        format!(
            " …cut at {MAX_HOOK_STDOUT_BYTES} bytes of {}",
            self.total
                .max(u64::try_from(MAX_HOOK_STDOUT_BYTES).unwrap_or(u64::MAX))
        )
    }
}

/// Drains a stream, keeping at most `limit` bytes and saying when there were more.
///
/// The rest is read and dropped rather than left in the pipe: a hook blocked writing an answer
/// nobody is reading is a hook that never exits.
fn bounded<S: std::io::Read>(stream: Option<S>, limit: usize) -> Drained {
    let Some(mut stream) = stream else {
        return Drained::empty();
    };
    let mut kept = Vec::new();
    let bound = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    let read = stream.by_ref().take(bound).read_to_end(&mut kept).is_ok();
    let over = kept.len() > limit;
    // Whatever is left is read and dropped — a hook blocked on a full pipe never exits, and the
    // wait would then be a timeout rather than an answer — but it is *counted*, so a message built
    // from a cut stream can say how much of it there was.
    let mut total = u64::try_from(kept.len()).unwrap_or(u64::MAX);
    if over || !read {
        total = total
            .saturating_add(std::io::copy(&mut stream, &mut std::io::sink()).unwrap_or_default());
    }
    kept.truncate(limit);
    Drained {
        text: String::from_utf8_lossy(&kept).into_owned(),
        over,
        total,
    }
}

/// What a failing hook said on stderr, in a form that fits on one line.
///
/// Says so when it was cut: a reason that stops mid-sentence must not read like a whole one.
fn describe(stderr: &Drained) -> String {
    let trimmed = stderr.text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    format!(
        ": {}{}",
        trimmed
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
        stderr.cut()
    )
}

impl HookPort for Hooks {
    fn before_call(&mut self, call: &ToolCall, invoked: &ToolSpec) -> HookDecision {
        let hooks = self.matching(HookPoint::BeforeCall, Some(invoked.name.as_str()));
        if hooks.is_empty() {
            return HookDecision::Proceed;
        }
        let document = self.call_document(HookPoint::BeforeCall, call, invoked, None);
        self.decide(&hooks, &document)
    }

    fn after_call(
        &mut self,
        call: &ToolCall,
        invoked: &ToolSpec,
        outcome: &ToolOutcome,
    ) -> AfterCall {
        let hooks = self.matching(HookPoint::AfterCall, Some(invoked.name.as_str()));
        if hooks.is_empty() {
            return AfterCall::default();
        }
        let document = self.call_document(HookPoint::AfterCall, call, invoked, Some(outcome));
        let mut notes = Vec::new();
        let mut decision = HookDecision::Proceed;
        for hook in &hooks {
            // A hook here cannot block and cannot fail the outcome — the effect has happened. But
            // the model still has to learn that the formatter ran, or did not, so everything a
            // hook said becomes a note it reads. Every declared hook runs: unlike the deciding
            // points, there is no refusal here for an earlier one to have won.
            match self.speak(hook, &document) {
                Spoken::Proceed { note } => notes.extend(note),
                // Exit 2 is a block at the other two points and means nothing at this one, so it
                // is a note and the record still says `proceed`: the hook ran and had its say.
                Spoken::Block { reason } => notes.push(reason),
                // A hook that could not run at all is the record's business as well as the
                // model's — a reader of the JSONL record could not otherwise tell a crashed
                // after-call hook from one that quietly approved. The first failure is the
                // decision, as the first refusal is at the deciding points.
                Spoken::Failed { reason } => {
                    notes.push(reason.clone());
                    if decision.is_proceed() {
                        decision = HookDecision::failed(reason);
                    }
                }
            }
        }
        AfterCall {
            note: (!notes.is_empty()).then(|| notes.join("\n")),
            decision,
        }
    }

    fn on_stop(&mut self, text: &str) -> HookDecision {
        let hooks = self.matching(HookPoint::Stop, None);
        if hooks.is_empty() {
            return HookDecision::Proceed;
        }
        // No `call` and no `entry`: a stop is about the run. A hook reads `hook == "stop"` and
        // finds `text`, which is what a consumer of this run would have read.
        let document = json!({
            "hook": HookPoint::Stop.as_str(),
            "text": text,
            "workspace": self.workspace.display().to_string(),
        });
        self.decide(&hooks, &document)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_wire::{Approval, CallId, Envelope, ToolName};

    /// A hooks file naming one program, as the operator would write it.
    fn file(on: &str, tools: Option<&[&str]>, command: &[&str]) -> String {
        let declaration = match tools {
            Some(names) => json!({"on": on, "tools": names, "command": command}),
            None => json!({"on": on, "command": command}),
        };
        json!({"version": 1, "hooks": [declaration]}).to_string()
    }

    fn hooks(text: &str) -> Hooks {
        Hooks::parse(text, Path::new("hooks.json"))
            .expect("the file parses")
            .with_timeout(Duration::from_secs(10))
    }

    /// A shell is used **only here**, as the hook program: it is the shortest way to write a
    /// program with an exact exit status. The harness itself never invokes one.
    fn shell(script: &str) -> Vec<String> {
        vec!["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()]
    }

    fn call() -> ToolCall {
        ToolCall {
            call_id: CallId::new("call-1").expect("valid"),
            name: ToolName::new("tool_invoke").expect("valid"),
            arguments: json!({"name": "file_write", "arguments": {"path": "note.md"}}),
        }
    }

    fn spec(name: &str) -> ToolSpec {
        ToolSpec {
            name: ToolName::new(name).expect("valid"),
            description: "writes a file".to_owned(),
            input_schema: json!({"type": "object"}),
            approval: Approval::NotRequired,
            envelope: Envelope::default(),
        }
    }

    fn declared(on: &str, tools: Option<&[&str]>, script: &str) -> Hooks {
        let command = shell(script);
        let words: Vec<&str> = command.iter().map(String::as_str).collect();
        hooks(&file(on, tools, &words))
    }

    #[test]
    fn a_hook_that_exits_zero_proceeds() {
        let mut port = declared("before-call", None, "exit 0");
        assert_eq!(
            port.before_call(&call(), &spec("file_write")),
            HookDecision::Proceed
        );
    }

    #[test]
    fn a_hook_that_exits_two_blocks_with_the_reason_it_printed() {
        let mut port = declared(
            "before-call",
            None,
            r#"printf '{"reason": "the tree is dirty"}'; exit 2"#,
        );
        assert_eq!(
            port.before_call(&call(), &spec("file_write")),
            HookDecision::block("the tree is dirty")
        );
    }

    #[test]
    fn a_block_with_nothing_on_stdout_falls_back_to_stderr_and_then_to_the_program() {
        let mut from_stderr = declared("before-call", None, "echo 'no writes today' >&2; exit 2");
        assert_eq!(
            from_stderr.before_call(&call(), &spec("file_write")),
            HookDecision::block("no writes today")
        );

        let mut silent = declared("before-call", None, "exit 2");
        let HookDecision::Block { reason } = silent.before_call(&call(), &spec("file_write"))
        else {
            panic!("an exit 2 blocks");
        };
        assert!(reason.contains("/bin/sh"), "{reason}");
        assert!(reason.contains("without a reason"), "{reason}");
    }

    #[test]
    fn any_other_status_is_a_failure_naming_the_program_and_the_status() {
        let mut port = declared("before-call", None, "echo 'boom' >&2; exit 7");
        let HookDecision::Failed { reason } = port.before_call(&call(), &spec("file_write")) else {
            panic!("an exit 7 is neither a yes nor a no");
        };
        assert!(reason.contains("/bin/sh"), "{reason}");
        assert!(reason.contains("exited 7"), "{reason}");
        assert!(reason.contains("boom"), "the program's own words: {reason}");
    }

    #[test]
    fn a_program_that_cannot_be_started_fails_by_name() {
        let mut port = hooks(&file(
            "before-call",
            None,
            &["/nonexistent/b10x-hook-that-is-not-there"],
        ));
        let HookDecision::Failed { reason } = port.before_call(&call(), &spec("file_write")) else {
            panic!("a program that is not there cannot say yes");
        };
        assert!(reason.contains("b10x-hook-that-is-not-there"), "{reason}");
    }

    #[test]
    fn the_hook_reads_one_document_naming_the_invoked_entry_and_the_workspace() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let seen = dir.path().join("stdin.json");
        let mut port = declared("before-call", None, &format!("cat > {}", seen.display()))
            .in_workspace(dir.path());
        assert_eq!(
            port.before_call(&call(), &spec("file_write")),
            HookDecision::Proceed
        );

        let document: Value =
            serde_json::from_str(&fs::read_to_string(&seen).expect("the hook wrote its stdin"))
                .expect("one JSON document");
        assert_eq!(document["hook"], json!("before-call"));
        assert_eq!(document["call"]["call_id"], json!("call-1"));
        assert_eq!(document["call"]["name"], json!("tool_invoke"));
        assert_eq!(document["call"]["arguments"]["name"], json!("file_write"));
        // The entry, never the verb.
        assert_eq!(document["entry"], json!("file_write"));
        assert_eq!(document["outcome"], Value::Null);
        assert!(
            Path::new(document["workspace"].as_str().expect("a path")).is_absolute(),
            "{document}"
        );
    }

    #[test]
    fn an_after_call_hook_is_given_the_outcome_and_answers_with_a_note() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let seen = dir.path().join("stdin.json");
        let mut port = declared(
            "after-call",
            None,
            &format!(
                r#"cat > {}; printf '{{"note": "rustfmt reformatted it"}}'"#,
                seen.display()
            ),
        );
        let spoke = port.after_call(
            &call(),
            &spec("file_write"),
            &ToolOutcome::ok(json!({"written": 12})),
        );
        assert_eq!(spoke, AfterCall::note("rustfmt reformatted it"));

        let document: Value =
            serde_json::from_str(&fs::read_to_string(&seen).expect("the hook wrote its stdin"))
                .expect("one JSON document");
        assert_eq!(document["hook"], json!("after-call"));
        assert_eq!(document["outcome"]["output"]["written"], json!(12));
        assert_eq!(document["outcome"]["failed"], json!(false));
    }

    #[test]
    fn an_after_call_hook_that_failed_says_so_to_the_model_and_to_the_record() {
        let mut port = declared("after-call", None, "echo 'formatter missing' >&2; exit 9");
        let spoke = port.after_call(&call(), &spec("file_write"), &ToolOutcome::ok(json!({})));
        let note = spoke
            .note
            .as_deref()
            .expect("the model must learn about it");
        assert!(note.contains("exited 9"), "{note}");
        assert_eq!(
            spoke.decision,
            HookDecision::failed(note),
            "and the record must not say a hook that crashed proceeded"
        );
    }

    #[test]
    fn an_after_call_hook_that_exits_two_is_a_note_and_not_a_block() {
        // Exit 2 is the block status at the deciding points and means nothing here: the effect has
        // already happened, so there is nothing left to refuse. The hook ran and had its say, so
        // the record says `proceed` and the model reads what it said.
        let mut port = declared(
            "after-call",
            None,
            r#"printf '{"reason": "the tests now fail"}'; exit 2"#,
        );
        assert_eq!(
            port.after_call(&call(), &spec("file_write"), &ToolOutcome::ok(json!({}))),
            AfterCall::note("the tests now fail"),
            "exit 2 is not a failure of the hook, so the record must not say the point failed"
        );
    }

    #[test]
    fn a_stop_hook_reads_the_text_the_run_would_answer_with() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let seen = dir.path().join("stdin.json");
        let mut port = declared("stop", None, &format!("cat > {}", seen.display()));
        assert_eq!(port.on_stop("all done"), HookDecision::Proceed);

        let document: Value =
            serde_json::from_str(&fs::read_to_string(&seen).expect("the hook wrote its stdin"))
                .expect("one JSON document");
        assert_eq!(document["hook"], json!("stop"));
        assert_eq!(document["text"], json!("all done"));
        // A stop is about the run, so there is no call and no entry to name.
        assert_eq!(document["call"], Value::Null);
        assert_eq!(document["entry"], Value::Null);
    }

    #[test]
    fn the_tools_filter_reads_the_invoked_entry_and_not_the_verb() {
        // Declared for `file_write`; the call travels as `tool_invoke` either way.
        let mut port = declared("before-call", Some(&["file_write"]), "exit 2");
        assert_eq!(
            port.before_call(&call(), &spec("file_read")),
            HookDecision::Proceed,
            "a hook about `file_write` says nothing about a read"
        );
        assert!(
            matches!(
                port.before_call(&call(), &spec("file_write")),
                HookDecision::Block { .. }
            ),
            "and everything about a write"
        );
    }

    #[test]
    fn more_than_the_bound_on_stdout_is_refused_rather_than_cut() {
        // 20 000 bytes, in a program with no dependency beyond the shell itself.
        let mut port = declared(
            "before-call",
            None,
            "i=0; while [ $i -lt 200 ]; do printf '%0100d' 0; i=$((i+1)); done; exit 0",
        );
        let HookDecision::Failed { reason } = port.before_call(&call(), &spec("file_write")) else {
            panic!("an answer too big to trust is refused");
        };
        assert!(reason.contains("16384"), "{reason}");
    }

    #[test]
    fn a_hook_that_does_not_finish_is_killed_and_fails() {
        let mut port =
            declared("before-call", None, "sleep 30").with_timeout(Duration::from_millis(150));
        let started = Instant::now();
        let HookDecision::Failed { reason } = port.before_call(&call(), &spec("file_write")) else {
            panic!("a hook that never answers did not say yes");
        };
        assert!(reason.contains("was killed"), "{reason}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "it was killed at the timeout, not waited out"
        );
    }

    #[test]
    fn a_hook_that_exits_leaving_a_grandchild_on_the_pipe_is_abandoned_at_the_bound() {
        // The stall the bound has to cover: the hook itself exits at once, and something it
        // started holds stdout for another thirty seconds. Draining with a `join` would wait for
        // the *last writer* to close and cost the run those thirty seconds — for ever, behind a
        // daemon. The deadline is one deadline, so this answers inside it.
        let mut port = declared("before-call", None, "sleep 30 & exit 0")
            .with_timeout(Duration::from_millis(200));
        let started = Instant::now();
        let HookDecision::Failed { reason } = port.before_call(&call(), &spec("file_write")) else {
            panic!("a hook whose answer never arrived did not say yes");
        };
        assert!(reason.contains("/bin/sh"), "the program is named: {reason}");
        assert!(reason.contains("200 ms"), "and the bound: {reason}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "it was abandoned at the bound, not waited out: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_reason_cut_at_the_bound_says_it_was_cut_and_by_how_much() {
        // 200 000 bytes on stderr, of which the first 16 KiB is still the reason. Delivering it
        // silently would read to a person exactly like a hook that stopped mid-sentence on purpose
        // (`AGENTS.md` invariant 8).
        let mut port = declared(
            "before-call",
            None,
            "i=0; while [ $i -lt 2000 ]; do printf '%0100d' 0; i=$((i+1)); done >&2; exit 2",
        );
        let HookDecision::Block { reason } = port.before_call(&call(), &spec("file_write")) else {
            panic!("an exit 2 blocks");
        };
        assert!(
            reason.contains("…cut at 16384 bytes of 200000"),
            "the cut is named, with what there was: {}",
            &reason[reason.len().saturating_sub(60)..]
        );
    }

    #[test]
    fn a_named_variable_is_removed_from_the_environment_a_hook_inherits() {
        // What `--api-key-env B10X_TEST_SECRET` reaches: unconfined is not the same as handed the
        // key, and a hook that echoed its environment into a note would otherwise put this run's
        // credential in the conversation and in the record.
        //
        // `CARGO_PKG_NAME` stands in for the credential variable: it is set in this process, it is
        // not one a shell invents for itself the way it invents `PATH`, and nothing has to be
        // written into the environment to arrange it. **Nothing calls `set_var` to prove this** —
        // that mutates the whole test process while other tests are spawning their own hooks, and
        // it would put a stand-in for a credential in a shared place. The end-to-end suite proves
        // the same removal over the real flag, on the real credential variable.
        let script = r#"printf '{"note": "saw [%s]"}' "$CARGO_PKG_NAME""#;
        let carried = declared("after-call", None, script)
            .after_call(&call(), &spec("file_write"), &ToolOutcome::ok(json!({})))
            .note
            .expect("the hook answers");
        assert_eq!(
            carried,
            format!("saw [{}]", env!("CARGO_PKG_NAME")),
            "the environment is otherwise the one the harness was started in"
        );

        let stripped = declared("after-call", None, script)
            .without_env(["CARGO_PKG_NAME"])
            .after_call(&call(), &spec("file_write"), &ToolOutcome::ok(json!({})))
            .note
            .expect("the hook still answers");
        assert_eq!(stripped, "saw []", "the named variable is gone");
    }

    #[test]
    fn several_hooks_at_one_point_run_in_order_and_the_first_refusal_wins() {
        let text = json!({
            "version": 1,
            "hooks": [
                {"on": "before-call", "command": shell("exit 0")},
                {"on": "before-call", "command": shell(r#"printf '{"reason": "second said no"}'; exit 2"#)},
                {"on": "before-call", "command": shell("exit 7")},
            ],
        })
        .to_string();
        let mut port = hooks(&text);
        assert_eq!(
            port.before_call(&call(), &spec("file_write")),
            HookDecision::block("second said no"),
            "the third never ran, or its failure would be the answer"
        );
    }

    #[test]
    fn several_after_call_notes_are_joined_rather_than_one_replacing_another() {
        let text = json!({
            "version": 1,
            "hooks": [
                {"on": "after-call", "command": shell(r#"printf '{"note": "first"}'"#)},
                {"on": "after-call", "command": shell(r#"printf '{"note": "second"}'"#)},
            ],
        })
        .to_string();
        let mut port = hooks(&text);
        assert_eq!(
            port.after_call(&call(), &spec("file_write"), &ToolOutcome::ok(json!({}))),
            AfterCall::note("first\nsecond")
        );
    }

    #[test]
    fn a_point_no_hook_named_spawns_nothing() {
        let mut port = declared("stop", None, "exit 2");
        assert_eq!(
            port.before_call(&call(), &spec("file_write")),
            HookDecision::Proceed
        );
        assert_eq!(
            port.after_call(&call(), &spec("file_write"), &ToolOutcome::ok(json!({}))),
            AfterCall::default()
        );
    }

    #[test]
    fn a_file_this_build_cannot_read_is_refused_by_what_is_wrong_with_it() {
        let path = Path::new("hooks.json");
        for (text, expected) in [
            (
                json!({"version": 2, "hooks": []}).to_string(),
                "reads version 1",
            ),
            (
                json!({"version": 1, "hooks": [{"on": "at-the-end", "command": ["x"]}]}).to_string(),
                "not a hook point",
            ),
            (
                json!({"version": 1, "hooks": [{"on": "stop", "command": []}]}).to_string(),
                "empty `command`",
            ),
            (
                json!({"version": 1, "hooks": [{"on": "before-call", "tools": [], "command": ["x"]}]})
                    .to_string(),
                "omit it to mean every call",
            ),
            (
                json!({"version": 1, "hooks": [{"on": "stop", "tools": ["run"], "command": ["x"]}]})
                    .to_string(),
                "nothing would ever match",
            ),
            (
                json!({"hooks": []}).to_string(),
                "missing field `version`",
            ),
            ("not json at all".to_owned(), "expected"),
        ] {
            let error = Hooks::parse(&text, path).expect_err("refused");
            assert!(
                error.contains(expected),
                "expected `{expected}` in: {error}"
            );
            assert!(error.contains("hooks.json"), "the file is named: {error}");
        }
    }

    #[test]
    fn a_file_declaring_no_hooks_is_a_port_that_says_nothing() {
        let mut port = hooks(&json!({"version": 1, "hooks": []}).to_string());
        assert!(port.is_empty());
        assert_eq!(port.len(), 0);
        assert_eq!(port.on_stop("done"), HookDecision::Proceed);
    }
}
