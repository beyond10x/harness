#![forbid(unsafe_code)]

//! The b10x agent loop.
//!
//! One turn goes out, tool calls come back, the results go in and the next turn goes out. Owning
//! that cycle is the whole point: the tools are ours to publish, the budgets are ours to count,
//! and an approval is a blocking call rather than a protocol round trip that can land after the
//! effect.
//!
//! The loop reaches a model through [`harness_wire::ModelPort`] and its tools through
//! [`harness_wire::ToolPort`]. In-process those tools are direct calls; under a bridge they are a
//! callback over the wire. The loop cannot tell the difference, which is what keeps the embedded
//! and the bridged harness the same code.
//!
//! # Hooks narrow a run; they never widen it
//!
//! An operator may attach their own programs at three moments — before a call, after one, and at
//! the stop ([`HookPort`], design 0002 § 3). Every one of them can only **narrow**. `before-call`
//! is consulted after the approver has already said yes, so a call a person refused never reaches
//! a hook and a hook cannot un-refuse it; its only answers are *proceed* and *no*, and a hook that
//! could not decide is a *no* (fail closed — a hook that could not run did not say yes).
//! `after-call` may leave the model a note beside a result and cannot mark the result failed.
//! `stop` may take away the model's right to stop, at most [`MAX_STOP_HOOK_CONTINUES`] times, and
//! cannot add a right to act. A hook that widened would be a second gate nobody reviews.
//!
//! All three fire for the loop's own tools as well ([`OutputSchema`], [`Delegation`]), with each
//! tool's own spec as the entry — a `before-call` declaration with no tool filter means *every*
//! call. The one place a point does **not** fire is the end of a delegate: `stop` is about the
//! run's ending, and a child's ending is not one (see [`AgentLoop::stop_hook`]).
//!
//! **The loop spawns no process.** [`HookPort`] is a seam exactly as [`ApprovalPort`] is; the
//! implementation that runs a process — the argv, the timeout, the stdout bound, the file naming
//! the hooks — lives in the shell, which read that file from a path the operator gave it. Nothing
//! here discovers a hook from a workspace. A run with hooks attached also batches nothing, so a
//! hook fires exactly once per call.
//!
//! It does start **threads**, in one place: a turn's `delegate` calls may run side by side
//! ([`AgentLoop::delegates_side_by_side`]). That is not a hole in the sentence above — a hook is
//! still asked once per call, still by the shell's own [`HookPort`], and still on this thread. A
//! child on a worker thread reaches the approver, the hooks and the record by asking this one
//! ([`parallel`]), so there is exactly one of each however many children there are.

//! # The two tools the loop owns
//!
//! [`OutputSchema`] publishes `answer` and [`Delegation`] publishes `delegate`, and neither is a
//! catalogue entry: `answer` performs no operation on any machine, and `delegate` performs
//! whatever this run's own tools perform, through this run's own gate. So they belong here rather
//! than to a [`harness_wire::ToolPort`], which never sees them — their specs are appended after
//! the port's on every turn, a call naming one is resolved before anything else and is never
//! batched, never routed by bare name and never handed to the port, and a port that already
//! publishes a name they need refuses the run ([`LoopError::Config`]) before the first byte goes
//! out. Each still produces exactly one [`Item::ToolResult`], because a call replayed without its
//! result is a hard error on the next turn.
//!
//! Both are opt-in per run ([`LoopConfig::output_schema`], [`LoopConfig::delegation`], both
//! [`None`] by default), so every invocation written before they existed means what it did.
//! Design: `docs/design/0002-sub-agents-structured-output-hooks.md` § 0.

mod agent;
mod answer;
mod approval;
mod budget;
mod delegate;
mod event;
mod hook;
mod parallel;
mod price;
mod skill;

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use harness_wire::{
    Approval, CallId, Item, MAX_TOOL_ARGUMENT_BYTES, MAX_TOOL_RESULT_BYTES, ModelPort, Risk,
    Sampling, StopReason, StreamEvent, StreamSink, ToolCall, ToolOutcome, ToolPort, ToolSpec,
    TurnOutcome, TurnRequest, Usage, WireError, WireErrorCode, exceeds,
};
use serde::{Deserialize, Serialize};

pub use agent::{Agent, Agents};
pub use answer::{
    DEFAULT_ANSWER_DESCRIPTION, DEFAULT_ANSWER_NAME, MAX_ANSWER_NUDGES, OutputSchema,
    OutputSchemaError,
};
pub use approval::{ApprovalDecision, ApprovalPort, ApproveAll, DenyAll};
pub use budget::{Budget, BudgetError};
pub use delegate::{
    DEFAULT_DELEGATE_NAME, DELEGATE_DESCRIPTION, DELEGATE_MAX_PARALLEL, DELEGATE_MAX_TURNS,
    DELEGATE_PARALLEL_NOTE, DELEGATE_PREAMBLE, Delegation, MAX_DELEGATION_DEPTH,
};
pub use event::{
    CredentialRenewal, LoopEvent, LoopSink, NullLoopSink, ProfileRef, VecLoopSink, Withheld,
};
pub use hook::{AfterCall, HookDecision, HookPoint, HookPort, NoHooks};
pub use price::{ModelRates, RateCard, RateCardError, Rates, micro_usd_as_decimal};
pub use skill::{DEFAULT_SKILL_NAME, SKILL_DESCRIPTION, Skill, Skills};

/// How many times a `stop` hook may keep one run working after the model tried to end it.
///
/// Three. A hook that blocks every stop is a run with no end, and the model told the same thing
/// a fourth time has already shown it cannot act on it. After this the loop warns
/// `stop-hook-exhausted` and the run ends as the model asked.
pub const MAX_STOP_HOOK_CONTINUES: u32 = 3;

/// Why the loop stopped. Every variant is a real terminal state, not a failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum LoopStop {
    /// The model answered and asked for nothing further.
    Completed,
    MaxTurns {
        limit: u64,
    },
    MaxInputTokens {
        limit: u64,
        reported: u64,
    },
    MaxOutputTokens {
        limit: u64,
        reported: u64,
    },
    /// A spend ceiling bound. Reachable only for a run whose rate card prices its model — an
    /// unpriced run cannot be held to a figure nobody could compute.
    MaxCost {
        limit_micro_usd: u64,
        spent_micro_usd: u64,
    },
    Deadline {
        limit_ms: u64,
    },
    Cancelled {
        reason: String,
    },
    /// The provider ended a turn early for a reason it named.
    ProviderIncomplete {
        reason: String,
    },
    /// The run was asked for a structured answer and the model ended in prose instead.
    ///
    /// Not `Completed`: a consumer reading stdout as JSON must not get prose with a success
    /// status. `asked_again` is how many nudges were spent first — see
    /// [`MAX_ANSWER_NUDGES`].
    Unstructured {
        asked_again: u32,
    },
}

impl LoopStop {
    pub fn is_completed(&self) -> bool {
        matches!(self, Self::Completed)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoopOutcome {
    pub stop: LoopStop,
    /// The final assistant text. Empty when the run stopped before the model answered.
    pub text: String,
    /// The complete conversation, ready to be replayed into a following run.
    pub items: Vec<Item>,
    pub turns: u64,
    /// One entry per turn the provider reported for. An empty list means usage is unknown, not
    /// that nothing was spent.
    pub usage: Vec<Usage>,
    /// What the run cost in millionths of a US dollar, at the rates the caller declared.
    ///
    /// [`None`] when no rate card was supplied or none of it priced this model. Absent rather than
    /// zero, for the reason [`LoopEvent::Cost`] gives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_micro_usd: Option<u64>,
    /// The answer in the shape [`LoopConfig::output_schema`] asked for, when the model gave one.
    ///
    /// The arguments of the `answer` call, exactly as the provider accepted them — parsed by
    /// nobody here and validated against the schema by nobody here (design 0002 § 1, M3).
    /// [`None`] on a run that asked for no schema, and on one that stopped
    /// [`LoopStop::Unstructured`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured: Option<serde_json::Value>,
}

impl LoopOutcome {
    /// Returns the summed reported token counts, or `None` when no turn reported any.
    pub fn total_tokens(&self) -> Option<(u64, u64)> {
        total_tokens(&self.usage)
    }
}

/// What a run spent, readable however the run ended.
///
/// [`AgentLoop::run_in`] writes one on **every** exit path, exactly as it writes the conversation
/// back, and for the same reason: a run that broke on the wire on turn twenty still bought
/// nineteen turns. Their [`LoopEvent::Usage`] and [`LoopEvent::Cost`] events have already gone out
/// on the sink by then, so a shell that filed the conversation of a failed run but none of its
/// spend under-reports what the failure cost — and the session file is the only record a person
/// still has after the process is gone.
///
/// # Why a second out-parameter rather than a payload on [`LoopError`]
///
/// The smallest shape that changes nothing else. `LoopError` stays three variants a caller matches
/// on and formats, rather than three that each have to be unpacked before the reason can be read;
/// [`AgentLoop::run`] keeps its signature and its behaviour for callers that do not care what a
/// run spent; and the rule a caller learns is the one [`AgentLoop::run_in`] already has for items —
/// *lend the loop somewhere to write, read it whatever comes back* — rather than a second, different
/// rule for the same question.
///
/// **Replaced, never accumulated.** One ledger holds one run: the figures are this run's, not the
/// session's, so a caller reusing a ledger across runs reads the last run and does its own folding
/// — which is what `usage` being one entry per billed turn is for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunLedger {
    /// One entry per turn the provider reported for, this run's delegates included.
    ///
    /// An empty list means usage is unknown, not that nothing was spent (`AGENTS.md` invariant 7).
    pub usage: Vec<Usage>,
    /// What the run spent in millionths of a US dollar, at the rates the caller declared.
    ///
    /// [`None`] when nothing priced this model, and absent stays absent: a failed run nobody could
    /// price must not fold a zero into a session's total.
    pub cost_micro_usd: Option<u64>,
    /// The turns this run started — the same count [`LoopOutcome::turns`] carries.
    pub turns: u64,
}

impl RunLedger {
    /// Returns the summed reported token counts, or `None` when no turn reported any.
    pub fn total_tokens(&self) -> Option<(u64, u64)> {
        total_tokens(&self.usage)
    }
}

/// The reported token counts of `usage`, summed, or `None` when no turn reported any.
///
/// One function behind both [`LoopOutcome::total_tokens`] and [`RunLedger::total_tokens`], so the
/// two records of one run cannot answer the same question differently.
fn total_tokens(usage: &[Usage]) -> Option<(u64, u64)> {
    if usage.is_empty() {
        return None;
    }
    Some(usage.iter().fold((0, 0), |(input, output), reported| {
        (
            input.saturating_add(reported.input_tokens),
            output.saturating_add(reported.output_tokens),
        )
    }))
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoopError {
    #[error("budget refused: {0}")]
    Budget(#[from] BudgetError),
    #[error("model wire refused: {0}")]
    Wire(#[from] WireError),
    /// The run as configured could not be described to a provider at all.
    ///
    /// Raised before the first request, so nothing is spent finding out. Today the only cause is a
    /// clash over a tool name — a [`ToolPort`] publishing one of the loop's own, or an
    /// [`OutputSchema`] and a [`Delegation`] published under the same name — which would put that
    /// name in `tools` twice and leave the model able to address neither.
    #[error("the run cannot start: {0}")]
    Config(String),
}

/// Ends the run between turns and between tool calls.
///
/// The same token the model wire uses. Sharing it is what makes a cancel reach the layer that is
/// actually blocked, which during a turn is almost always the model read rather than the loop.
pub type LoopCancel = harness_wire::Cancel;

// No `Eq`: sampling carries floating-point values, and two temperatures that are equal by `Eq`
// would be a stronger claim than the wire can make about them.
#[derive(Debug, Clone, PartialEq)]
pub struct LoopConfig {
    /// Exact model identifier sent on every turn.
    pub model: String,
    /// The standing instruction for the run, sent separately from the person's input.
    pub instructions: String,
    pub budget: Budget,
    /// How the model is asked to sample. Sent on every turn, because a stateless loop replays the
    /// whole conversation each time and a value set once would otherwise apply only to the first.
    pub sampling: Sampling,
    /// Rates to price this run at, declared by whoever started it.
    ///
    /// [`None`] leaves the run unpriced, which is what it has always been. It is also what makes
    /// [`Budget::max_cost_microunits`] enforceable or not: a ceiling is only real where the figure
    /// it bounds can be computed.
    pub prices: Option<RateCard>,
    /// The highest risk a call may carry without a person being asked.
    ///
    /// [`Risk::Low`] by default, because the default approver is [`DenyAll`] and *a harness that
    /// approves by default turns a review gate into decoration* — so the default posture is to ask
    /// about anything above a cheap, visible read. Every tool this harness ships declares
    /// `Approval::NotRequired`, so without this the gate decided nothing at all.
    ///
    /// Raising it is how a caller says *unattended* out loud, and it only means anything alongside
    /// an approver that says yes: [`ApproveAll`] with [`Risk::Destructive`] is an unattended run
    /// declared, rather than one arrived at because no tool happened to ask.
    pub unattended_ceiling: Risk,
    /// How many tokens the model's context window holds, when the caller knows.
    ///
    /// [`None`] leaves compaction on the fixed byte rule it has always had
    /// ([`MAX_CONVERSATION_BYTES`]), which is 192 KiB — roughly 50k tokens, so about 60 % of a
    /// 128k window was never reachable and a longer run met the provider's wall as a hard error
    /// instead of a compaction. With a window declared, the trigger becomes token-aware: see
    /// [`COMPACTION_TRIGGER_PERCENT`].
    ///
    /// A count of tokens, not of bytes, because that is the unit the provider refuses in.
    pub context_window: Option<u64>,
    /// The first pause before a turn whose stream broke is attempted again.
    ///
    /// Doubles per attempt up to [`MAX_TURN_RETRY_BACKOFF`]. A field rather than a constant so a
    /// test can exercise the exhaustion path without spending the 3.5 seconds the shipped default
    /// would take, and so an embedder driving many short runs can choose its own patience.
    pub retry_backoff: Duration,
    /// The shape the run's answer must take, when the caller wants one.
    ///
    /// Published as a tool the model calls to finish (design 0002 § 1). [`None`] is a run that
    /// answers in prose, which is what every run did before this existed.
    pub output_schema: Option<OutputSchema>,
    /// Whether the model may hand a sub-task to a fresh context (design 0002 § 2).
    ///
    /// [`None`] publishes no `delegate`, which is what every run did before this existed.
    pub delegation: Option<Delegation>,

    /// The profiles that configured this run, for its record. Acted on nowhere: what they set was
    /// already applied by the caller, and this is the sentence saying which file said it.
    pub profiles: Vec<ProfileRef>,

    /// Where this run's credential came from, for the record: `named`, or `provider:<name>`.
    pub credential_source: String,

    /// A credential this run renewed before it started, when it did. Acted on nowhere.
    ///
    /// The renewal happened in the caller — it is a file read, an HTTP POST and a file write, none
    /// of which this crate may do (`AGENTS.md` invariant 3) — and arrives here for the same reason
    /// `profiles` and `withheld` do: the loop owns the event stream, so a fact that belongs in the
    /// record has to be handed to it. [`None`] is the ordinary run, which renewed nothing.
    pub credential_renewal: Option<CredentialRenewal>,

    /// The named agents a delegate may be run as, or [`None`] for the generic delegate only.
    ///
    /// Loaded by the caller from the vendor's `agents/<name>.md` format. Each carries its own
    /// standing instruction and its own declared toolset, and the toolset can only ever narrow
    /// what this run was admitted — see [`Agent::admitted`].
    pub agents: Option<Agents>,

    /// Which of the things this run can **reach** it may use, or [`None`] for all of them.
    ///
    /// **A narrowing and never a widening.** The value is a declaration already intersected with
    /// what the parent was admitted, so a child cannot reach a tool its parent did not have by
    /// naming one. `None` is what every run a caller starts has. **Every** delegate is given
    /// `Some`, agent or no agent: an agentless child handed `None` would be handed the whole port,
    /// which is how a narrowed run used to climb back out by delegating again and naming nobody.
    ///
    /// # Written in the names of entries, not of published tools
    ///
    /// The two are the same list under a flat surface and are not under a verb surface, where what
    /// is published is `tool_search`, `tool_describe`, `tool_invoke` and what is *reached* is the
    /// catalogue behind them. [`harness_wire::ToolPort::reachable`] is the port's answer to which
    /// vocabulary is which, and [`AgentLoop::routes`] is the difference: while a run's grant holds
    /// a name its port does not publish, the port's published names are admitted as routes — an
    /// agent granted `file_read` and refused `tool_invoke` would have been granted nothing at all.
    ///
    /// # An empty grant admits nothing, routes included
    ///
    /// A route exists to carry a call to a granted entry. With no entry granted it leads nowhere,
    /// and all it can still do is let a child that was admitted none of the catalogue enumerate it.
    /// So [`AgentLoop::needs_routes`] is false for an empty grant and such a run publishes nothing
    /// and admits nothing — which is the answer a flat surface already gave, and the acceptance
    /// this whole narrowing is written against forbids the two surfaces differing.
    ///
    /// Enforced by one predicate, *admitted or a route*, asked at every site that can put a call
    /// on the port: [`AgentLoop::port_specs`] filters the published toolset by it, and
    /// [`AgentLoop::unadmitted`] answers it for [`AgentLoop::invoke`], [`AgentLoop::batchable`] and
    /// [`AgentLoop::run_batch`] alike, reading [`harness_wire::ToolPort::invoked`] so that the name
    /// judged is the entry's and not the verb's. Two chokepoints would be two chances to disagree,
    /// and so would two vocabularies — and a batched call that skipped the check was, for one
    /// commit, exactly such a second chance.
    pub admits: Option<Vec<harness_wire::ToolName>>,

    /// The skills this run may load, or [`None`] to publish no `skill` tool.
    ///
    /// Loaded by the caller and never discovered: a skill directory this loop went and looked for
    /// would be instructions the model follows that nobody declared.
    pub skills: Option<Skills>,
    /// Tools this run asked for that its machine would not admit, as whoever built the tool port
    /// found them.
    ///
    /// Reported once, in [`LoopEvent::Started`], beside what the run *does* have. It changes
    /// nothing the loop does: the withheld tool is not published, no call can name it, and no
    /// approval or refusal involves it. It exists because the publication gate works by absence,
    /// and an absence nobody states is indistinguishable from a run that never wanted the tool.
    ///
    /// **Configured rather than asked of the port**, because the fact is not the port's to know.
    /// `ToolPort` lives in `harness-wire`, which reads no machine and performs no I/O
    /// (`AGENTS.md` invariant 3); what a machine refused to confine is `harness_substrate`'s
    /// answer, and the shell that assembled the two is the one place that has both. A delegate
    /// inherits it with the rest of the config, because it runs over the same port.
    ///
    /// Empty is a run that got everything it asked for, and is the default.
    pub withheld: Vec<Withheld>,
}

impl LoopConfig {
    pub fn new(model: impl Into<String>, instructions: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            instructions: instructions.into(),
            budget: Budget::default(),
            sampling: Sampling::default(),
            prices: None,
            unattended_ceiling: Risk::Low,
            context_window: None,
            retry_backoff: TURN_RETRY_BACKOFF,
            output_schema: None,
            delegation: None,
            agents: None,
            profiles: Vec::new(),
            credential_source: "named".to_owned(),
            credential_renewal: None,
            admits: None,
            skills: None,
            withheld: Vec::new(),
        }
    }

    /// States which declared tools this run's machine would not admit.
    ///
    /// Additive to the record and to nothing else: it is reported at [`LoopEvent::Started`] and no
    /// decision is taken on it.
    #[must_use]
    pub fn with_withheld(mut self, withheld: Vec<Withheld>) -> Self {
        self.withheld = withheld;
        self
    }

    /// Asks for the run's answer in one shape, by publishing it as a tool the model calls.
    #[must_use]
    pub fn with_output_schema(mut self, schema: Option<OutputSchema>) -> Self {
        self.output_schema = schema;
        self
    }

    /// Lets the model delegate a sub-task to a fresh context on the same gate.
    #[must_use]
    pub fn with_delegation(mut self, delegation: Option<Delegation>) -> Self {
        self.delegation = delegation;
        self
    }

    /// Names the profiles this run was configured by, for the record.
    #[must_use]
    pub fn with_profiles(mut self, profiles: Vec<ProfileRef>) -> Self {
        self.profiles = profiles;
        self
    }

    /// Says a provider's default supplied the credential, rather than the operator naming one.
    #[must_use]
    pub fn with_credential_source(mut self, source: String) -> Self {
        self.credential_source = source;
        self
    }

    /// States that the caller renewed this run's credential, and rewrote the file holding it.
    #[must_use]
    pub fn with_credential_renewal(mut self, renewal: Option<CredentialRenewal>) -> Self {
        self.credential_renewal = renewal;
        self
    }

    /// Lets a delegate be run as one of the operator's named agents.
    #[must_use]
    pub fn with_agents(mut self, agents: Option<Agents>) -> Self {
        self.agents = agents;
        self
    }

    /// Narrows this run to a subset of what its port publishes.
    #[must_use]
    pub fn with_admitted(mut self, admits: Option<Vec<harness_wire::ToolName>>) -> Self {
        self.admits = admits;
        self
    }

    /// Lets the model load the operator's skills by name, one call each.
    #[must_use]
    pub fn with_skills(mut self, skills: Option<Skills>) -> Self {
        self.skills = skills;
        self
    }

    #[must_use]
    pub fn with_sampling(mut self, sampling: Sampling) -> Self {
        self.sampling = sampling;
        self
    }

    #[must_use]
    pub fn with_budget(mut self, budget: Budget) -> Self {
        self.budget = budget;
        self
    }

    #[must_use]
    pub fn with_prices(mut self, prices: Option<RateCard>) -> Self {
        self.prices = prices;
        self
    }

    #[must_use]
    pub fn with_unattended_ceiling(mut self, ceiling: Risk) -> Self {
        self.unattended_ceiling = ceiling;
        self
    }

    /// Declares the model's context window, in tokens, and makes compaction token-aware.
    ///
    /// [`None`] is not a window of zero: it is a run whose window nobody stated, and it keeps the
    /// byte rule.
    #[must_use]
    pub fn with_context_window(mut self, tokens: Option<u64>) -> Self {
        self.context_window = tokens;
        self
    }

    #[must_use]
    pub fn with_retry_backoff(mut self, backoff: Duration) -> Self {
        self.retry_backoff = backoff;
        self
    }
}

/// How many further attempts a turn gets after a **retriable** wire failure.
///
/// Three, and then the run ends with the failure it already had. The cost of one more attempt is a
/// whole replay of the conversation — quadratic in run length — so this is not a number to raise
/// without measuring what a fourth attempt actually recovers.
pub const MAX_TURN_RETRIES: u32 = 3;

/// The first pause between attempts at a turn, doubling per attempt.
pub const TURN_RETRY_BACKOFF: Duration = Duration::from_millis(500);

/// The longest pause between attempts, however many have been made.
///
/// A ceiling rather than unbounded doubling: past a few seconds the person watching has already
/// decided whether to wait, and a run that pauses for a minute reads as one that has hung.
///
/// **On a shipped run it never binds, and that is deliberate.** The default
/// [`LoopConfig::retry_backoff`] is 500 ms and [`MAX_TURN_RETRIES`] is three, so the pauses are
/// 0.5 s, 1 s and 2 s and the largest is a quarter of this. It is a guard for an embedder that
/// raises `retry_backoff` — at 5 s the third pause would otherwise be 20 s — not a number the
/// defaults reach.
pub const MAX_TURN_RETRY_BACKOFF: Duration = Duration::from_secs(8);

/// How often a pause looks at the cancellation token.
///
/// The pause is slept in slices of this, so a Ctrl-C during an eight-second back-off is honoured
/// within a frame rather than after it. Sleeping the whole pause in one call would make the loop
/// deaf for exactly as long as it is least useful to be.
const CANCEL_POLL: Duration = Duration::from_millis(25);

fn cancelled() -> LoopStop {
    LoopStop::Cancelled {
        reason: "the caller cancelled".to_owned(),
    }
}

fn terminal_stop(reason: StopReason) -> LoopStop {
    match reason {
        StopReason::MaxOutputTokens => LoopStop::ProviderIncomplete {
            reason: "max_output_tokens".to_owned(),
        },
        StopReason::Incomplete { reason } => LoopStop::ProviderIncomplete { reason },
        StopReason::EndTurn | StopReason::ToolCalls => LoopStop::Completed,
    }
}

/// Everything one run accumulates.
struct RunState {
    items: Vec<Item>,
    usage: Vec<Usage>,
    turns: u64,
    input_total: u64,
    output_total: u64,
    /// Absent until a turn is priced, so a run nobody could price never reports a figure.
    cost_total: Option<u64>,
    /// What the last **conversation** turn reported as its input, for the compaction trigger.
    ///
    /// Not `usage.last()`: a summary turn is charged to the same run but its input count measures
    /// the prefix it was handed, not the conversation. Reading it would tell the next compaction
    /// the window had emptied and stop it firing again.
    reported_input: Option<u64>,
    text: String,
    /// The `answer` call's arguments, once the model has made it.
    structured: Option<serde_json::Value>,
    /// How many times the model was told to call the answer tool ([`MAX_ANSWER_NUDGES`]).
    nudged: u32,
    /// How many times a stop hook kept the run going ([`MAX_STOP_HOOK_CONTINUES`]).
    stop_continues: u32,
}

impl RunState {
    /// A run that continues a conversation somebody else was holding.
    ///
    /// An empty `items` is a first run, and is how [`AgentLoop::run`] starts one.
    ///
    /// The prior items come first and the new input is one more user item after them — the same
    /// shape a first run has, which is why nothing below this line can tell a resumed run from a
    /// fresh one.
    fn resuming(mut items: Vec<Item>, input: impl Into<String>) -> Self {
        items.push(Item::user(input));
        Self {
            items,
            usage: Vec::new(),
            turns: 0,
            input_total: 0,
            output_total: 0,
            cost_total: None,
            reported_input: None,
            text: String::new(),
            structured: None,
            nudged: 0,
            stop_continues: 0,
        }
    }

    /// Counts what one turn reported against the run's totals, its budget and its bill.
    ///
    /// Separate from [`RunState::absorb`] because a summary turn spends tokens without adding
    /// anything to the conversation: what it produced replaces items rather than joining them. It
    /// is still a turn the provider charged for, so it is counted here exactly like any other —
    /// a compaction that priced itself at nothing would understate every long run.
    ///
    /// Absent usage produces no event and no zero, for the reason [`LoopEvent::Cost`] gives.
    fn absorb_usage(
        &mut self,
        usage: Option<Usage>,
        prices: Option<&RateCard>,
        sink: &mut dyn LoopSink,
    ) {
        let Some(reported) = usage else {
            return;
        };
        self.input_total = self.input_total.saturating_add(reported.input_tokens);
        self.output_total = self.output_total.saturating_add(reported.output_tokens);
        sink.emit(LoopEvent::Usage(reported.clone()));
        if let Some(micro_usd) = prices.and_then(|card| card.price(&reported)) {
            self.cost_total = Some(self.cost_total.unwrap_or(0).saturating_add(micro_usd));
            sink.emit(LoopEvent::Cost {
                model: reported.model.clone(),
                micro_usd,
            });
        }
        self.usage.push(reported);
    }

    /// Folds one turn into the run and returns the calls it asked for.
    fn absorb(
        &mut self,
        outcome: TurnOutcome,
        prices: Option<&RateCard>,
        sink: &mut dyn LoopSink,
    ) -> (Vec<ToolCall>, StopReason) {
        if let Some(reported) = outcome.usage.as_ref() {
            self.reported_input = Some(reported.input_tokens);
        }
        self.absorb_usage(outcome.usage, prices, sink);

        let turn_text: String = outcome
            .items
            .iter()
            .filter_map(|item| match item {
                Item::AssistantText { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        if !turn_text.is_empty() {
            self.text = turn_text;
        }

        let calls: Vec<ToolCall> = outcome
            .items
            .iter()
            .filter_map(Item::as_tool_call)
            .cloned()
            .collect();
        self.items.extend(outcome.items.into_iter().map(recordable));
        (calls, outcome.stop_reason)
    }

    /// Takes a child run's spend into this one.
    ///
    /// A delegate spends the run's budget rather than one of its own, so what it spent is the
    /// parent's — on **every** exit path the child has, an answer and a wire failure alike. A
    /// child that broke on turn four still bought three turns, and its [`LoopEvent::Usage`] and
    /// [`LoopEvent::Cost`] events have already gone out (wrapped) by the time it breaks: a parent
    /// that absorbed nothing from a failed child would report totals smaller than the record it
    /// emitted, carve the next delegate a remainder that is already gone, and never let
    /// [`AgentLoop::stop_after_tokens`] see the spend at all.
    ///
    /// Absent cost stays absent — a child nobody could price does not turn an unpriced parent into
    /// a run that cost zero.
    fn absorb_child(&mut self, child: &mut Self) {
        self.input_total = self.input_total.saturating_add(child.input_total);
        self.output_total = self.output_total.saturating_add(child.output_total);
        if let Some(spent) = child.cost_total {
            self.cost_total = Some(self.cost_total.unwrap_or(0).saturating_add(spent));
        }
        self.usage.append(&mut child.usage);
    }

    /// What this run has spent so far, in the shape the caller reads it in.
    ///
    /// Borrowing rather than consuming: a run that answered still owes its caller a
    /// [`LoopOutcome`] built from the same state, and the two records of one run must agree.
    fn ledger(&self) -> RunLedger {
        RunLedger {
            usage: self.usage.clone(),
            cost_micro_usd: self.cost_total,
            turns: self.turns,
        }
    }

    fn into_outcome(self, stop: LoopStop) -> LoopOutcome {
        LoopOutcome {
            stop,
            text: self.text,
            items: self.items,
            turns: self.turns,
            usage: self.usage,
            cost_micro_usd: self.cost_total,
            structured: self.structured,
        }
    }
}

/// How many bytes of conversation a turn may carry before the oldest tool results are elided.
///
/// A stateless loop replays everything every turn, so the cost of a run is quadratic in its length
/// and the context window is a hard ceiling on it. Nothing here summarises: what is dropped is
/// **bytes of old tool output**, which is where the weight is — a file read whose contents were
/// then edited is dead weight from the moment the edit landed.
pub const MAX_CONVERSATION_BYTES: usize = 192 * 1024;

/// How far below the bound a compaction elides, rather than stopping the moment it fits.
///
/// # The measurement that put this here
///
/// Without it, compaction stops at the bound — so the *next* large result crosses it again and the
/// prefix is rewritten a second time. A live 24-turn run on 2026-08-24 compacted at turn 18 (one
/// result, 22,707 bytes freed) and again at turn 22, and the two turns after those replayed 43,203
/// and 58,448 tokens **uncached**: about $0.39 of a $1.19 run, a third of the bill, for a cache the
/// run had already paid to build.
///
/// Eliding to a low-water mark makes the rewrite rare and deep instead of frequent and shallow. The
/// bytes dropped are the same bytes either way; what changes is how many times the cache is thrown
/// away to drop them.
pub const COMPACTED_TARGET_BYTES: usize = 96 * 1024;

/// How many **bytes** of the most recent tool results are never elided.
///
/// The model is usually working from what it just read. Eliding that would make it read the file
/// again, which costs more than the elision saved.
///
/// # Why this is a size and not a count
///
/// It was a count of six, and a live run on 2026-08-24 showed what that costs: six recent results
/// came to about 130kB, so a compaction that could only touch the rest freed 45,860 bytes and left
/// 177,915 — above the low-water mark, and barely under the bound. The conversation then crossed
/// the bound again on the next result, and again on the one after: four compactions, two of them on
/// consecutive turns, each one a full uncached replay.
///
/// A count cannot bound bytes when one result can be 64kB. A size can, so the floor can never sit
/// above the target it is meant to leave room under.
pub const KEPT_RESULT_BYTES: usize = 48 * 1024;

/// How many bytes of conversation one token is taken to be, where nothing has counted them.
///
/// An **estimate**, and named as one. Four bytes per token is the ratio usually quoted for English
/// prose; a conversation here is JSON — escaped paths, braces, quoted source — which tokenizes
/// denser, so this under-counts and fires later than the truth. Two things keep that safe: the
/// trigger reads the **larger** of this estimate and the provider's own reported input count, so
/// neither a provider that reports nothing nor an estimate that under-counts can delay a
/// compaction — whichever figure is nearer the wall is the one that fires; and the trigger sits at
/// 80 % of the window rather than at it, so the margin absorbs the error. It is never used to
/// state what a turn cost.
pub const ESTIMATED_BYTES_PER_TOKEN: u64 = 4;

/// How full the window has to be before a token-aware compaction fires.
///
/// Below the wall, not at it: a compaction that fires at 100 % has already lost, because the turn
/// that would have carried it is the one the provider refuses.
pub const COMPACTION_TRIGGER_PERCENT: u64 = 80;

/// How full the window a token-aware compaction aims to leave it.
///
/// The same argument as [`COMPACTED_TARGET_BYTES`]: stopping at the trigger leaves the next result
/// to cross it again, and every crossing rewrites the prefix a prompt cache is keyed on.
pub const COMPACTION_TARGET_PERCENT: u64 = 50;

/// The fewest bytes worth spending a whole model turn to fold into a summary.
///
/// A summary turn costs a replay of what it folds plus the tokens it writes, so folding a few
/// hundred bytes spends more than it recovers. Below this the loop keeps the elided conversation
/// and says nothing, which is also what stops a run whose weight is all in the protected tail from
/// buying a summary turn every turn.
pub const SUMMARY_MIN_FOLD_BYTES: usize = 8 * 1024;

/// The first line of the item a summary is folded into.
///
/// Fixed text, so a reader of a transcript — and the model on the next turn — can tell a summary
/// the harness wrote from something a person said. Never removed and never translated.
pub const SUMMARY_MARKER: &str =
    "[Earlier turns were summarised by the harness; the summary follows.]";

/// What the model is asked for when the harness folds the earlier part of a run into one item.
///
/// It is deliberately not the run's own standing instruction: this turn has no tools, no task and
/// no person to answer — it is being asked to compress a record. Naming what must survive (paths,
/// commands, outcomes, what is still open) is what keeps the summary usable as *working memory*
/// rather than as a readable paragraph: a summary that says "the tests were fixed" and drops the
/// file it happened in costs the next turn a search.
const SUMMARY_INSTRUCTION: &str = "\
You are compressing the earlier part of an agent's own working record so it can be dropped from \
its context. Write a dense summary of what was asked, what was decided, what was done and what is \
still open. Preserve exact file paths, identifiers, commands and their outcomes; a detail you drop \
is one the agent will have to spend a tool call to recover. Do not add anything that is not in the \
record you were given, do not address anybody, and write no preamble.";

/// The first item is the task, and a summary never folds it.
///
/// Everything after it is a record of working on the task and can be compressed. The task itself
/// is what the run is *for*: folded into a summary of its own execution, a long run would end up
/// answering a paraphrase of the question it was asked.
const FIRST_KEPT_ITEM: usize = 1;

/// How many bytes of rendered transcript one summary turn may carry.
///
/// The request that asks for a summary is itself a request, and it is built from the part of the
/// conversation that grew too large — so without a bound the turn meant to fit the run back inside
/// the window is the one the provider refuses. 128 KiB is roughly 32k tokens by
/// [`ESTIMATED_BYTES_PER_TOKEN`], which fits inside every window this loop has been pointed at
/// while still holding most of a long run.
///
/// What goes when it binds is the **oldest** items, because the newest are what the next turn is
/// about — and a line saying how many and how many bytes goes in their place. Never silently.
///
/// The bound is on the rendered items. The two lines that report what was cut and what is not
/// shown, and the instruction after them, sit outside it: a few hundred bytes on a figure with
/// three orders of magnitude of headroom.
pub const SUMMARY_TRANSCRIPT_BYTES: usize = 128 * 1024;

/// How many bytes of one call's arguments, or one result's output, the transcript shows.
///
/// A single 64 KiB file read would otherwise be half the transcript on its own and crowd out the
/// twenty turns around it, which are what a summary is actually for. What is cut is stated in the
/// line itself, with the figure, so the model reads a shortened result as shortened.
pub const SUMMARY_ITEM_BYTES: usize = 4 * 1024;

/// The items a summary turn sends, rendered from the part of the conversation being folded.
///
/// # Why the fold is rendered rather than replayed
///
/// Replaying `folded` as items was a request neither wire could accept. The fold begins after the
/// task, so its first item is assistant-side — and the Messages route requires the first message
/// to be `user`. It carries `tool_use` and `tool_result` blocks, and that route rejects those when
/// the request publishes no tools, which a summary turn deliberately does not. And it carries
/// [`Item::Opaque`] reasoning items, which are a provider's own encrypted state: replayable
/// verbatim in the conversation they came from, and meaningless in a request that is not that
/// conversation.
///
/// So the fold is rendered to text and sent as **one** [`Item::user`]: one message, `user`-first,
/// no tool blocks, no opaque items. That is wire-neutral by construction rather than by each wire
/// happening to tolerate it, which is why both wire crates project this function's output in their
/// own tests.
///
/// # What the rendering says out loud
///
/// Opaque items are dropped — there is nothing to render, and the model is told once how many.
/// Arguments and outputs over [`SUMMARY_ITEM_BYTES`] carry the count of what was cut, and a
/// transcript over [`SUMMARY_TRANSCRIPT_BYTES`] loses its oldest lines behind a line that says how
/// many. A shortened record the model reads as complete is exactly what invariant 8 forbids.
///
/// The ask itself comes **after** the record, where the model reads last, which is why it is here
/// and in the turn's standing instruction both: this item has to be a complete prompt on its own,
/// since a wire places a standing instruction wherever its route wants it.
#[must_use]
pub fn summary_request_items(folded: &[Item]) -> Vec<Item> {
    let mut lines: Vec<String> = Vec::with_capacity(folded.len());
    let mut opaque = 0_usize;
    for item in folded {
        match transcript_line(item) {
            Some(line) => lines.push(line),
            None => opaque += 1,
        }
    }
    // One for the newline each line is joined with, so the bound covers what is actually sent.
    let mut rendered: usize = lines.iter().map(|line| line.len() + 1).sum();
    let mut cut_lines = 0_usize;
    let mut cut_bytes = 0_usize;
    while rendered > SUMMARY_TRANSCRIPT_BYTES && cut_lines < lines.len() {
        let size = lines[cut_lines].len() + 1;
        rendered -= size;
        cut_bytes += size;
        cut_lines += 1;
    }

    let mut transcript: Vec<String> = Vec::new();
    if cut_lines > 0 {
        transcript.push(format!(
            "[The {cut_lines} oldest item(s) of this record, {cut_bytes} bytes, were cut to keep \
             it inside the {SUMMARY_TRANSCRIPT_BYTES} byte bound on one summary request. What \
             follows is the rest of it.]"
        ));
    }
    if opaque > 0 {
        transcript.push(format!(
            "[{opaque} provider reasoning item(s) are not shown: they are opaque to this harness, \
             which replays them verbatim and never reads them.]"
        ));
    }
    transcript.extend(lines.drain(cut_lines..));
    vec![Item::user(format!(
        "{}\n\n{SUMMARY_INSTRUCTION}",
        transcript.join("\n")
    ))]
}

/// One conversation item as one line of the transcript, or [`None`] for one there is nothing to
/// render.
fn transcript_line(item: &Item) -> Option<String> {
    Some(match item {
        Item::UserText { text } => format!("[user] {text}"),
        Item::AssistantText { text } => format!("[assistant] {text}"),
        Item::ToolCall(call) => format!(
            "[tool call {} {}]",
            call.name,
            bounded_json(&call.arguments)
        ),
        Item::ToolResult {
            call_id,
            output,
            failed,
        } => format!(
            "[tool result {call_id}{} {}]",
            if *failed { " failed" } else { "" },
            bounded_json(output)
        ),
        Item::Opaque { .. } => return None,
    })
}

/// One payload as compact JSON, cut to [`SUMMARY_ITEM_BYTES`] and saying so when it was.
fn bounded_json(value: &serde_json::Value) -> String {
    let json = serde_json::to_string(value)
        .unwrap_or_else(|_| "\"this payload could not be rendered\"".to_owned());
    if json.len() <= SUMMARY_ITEM_BYTES {
        return json;
    }
    let mut at = SUMMARY_ITEM_BYTES;
    while at > 0 && !json.is_char_boundary(at) {
        at -= 1;
    }
    format!(
        "{} …({cut} of {total} bytes were cut)",
        &json[..at],
        cut = json.len() - at,
        total = json.len()
    )
}

/// Elides the oldest tool results until the conversation fits, and says how much went.
///
/// # What is dropped, and what never is
///
/// Only **tool results**, and only their payload. The result item stays, because a `function_call`
/// replayed without its output is a provider error on the next turn — the same rule
/// [`recordable`] follows for an oversized call. User text, assistant text, the calls themselves
/// and opaque reasoning items are never touched: the first three are the record of what was asked
/// and decided, and dropping the fourth costs the model its own chain of thought across every tool
/// round trip, which is the whole reason this loop carries them.
///
/// # Why this is monotone, and why that matters for the bill
///
/// Compaction **rewrites the prefix**, and the prefix is what a prompt cache is keyed on: the turn
/// after a compaction pays full rate for everything. That is a real cost, bounded by doing this
/// rarely and never undoing it — an item elided once stays elided, so the prefix settles again
/// immediately and every later turn caches against the smaller conversation.
///
/// Trading one uncached turn for a permanently shorter conversation is worth it at these sizes;
/// trading one every turn would not be, which is why the threshold is a bound rather than a target.
///
/// This is the **byte** rule, which is what a run with no declared context window still gets. With
/// one, [`AgentLoop::compact_run`] decides in tokens and may summarise what elision cannot reach.
fn compact(items: &mut [Item], sink: &mut dyn LoopSink) {
    let before = measure(items);
    if before <= MAX_CONVERSATION_BYTES {
        return;
    }
    let elided = elide(
        items,
        COMPACTED_TARGET_BYTES,
        protected_bytes(COMPACTED_TARGET_BYTES),
    );
    if elided.count == 0 {
        return;
    }
    let after = measure(items);
    // Said out loud: a model that suddenly cannot see a file it read has a right to a reason,
    // and so does anyone reading the record afterwards.
    sink.emit(LoopEvent::Warning {
        code: "conversation-compacted".to_owned(),
        message: format!(
            "the conversation passed {MAX_CONVERSATION_BYTES} bytes, so {count} old tool \
             result(s) were elided, freeing {freed} bytes and leaving {after}. The most recent \
             {protected} result(s), {kept} bytes, are untouched.",
            count = elided.count,
            freed = elided.freed,
            protected = elided.protected,
            kept = elided.kept,
        ),
    });
    sink.emit(LoopEvent::Compacted {
        elided_results: elided.count,
        elided_bytes: elided.freed,
        summarised_items: 0,
        bytes_before: before,
        bytes_after: after,
        summary_turn: false,
    });
}

/// The conversation's size, the way every threshold here measures it.
fn measure(items: &[Item]) -> usize {
    items.iter().map(measure_one).sum()
}

fn measure_one(item: &Item) -> usize {
    serde_json::to_string(item).map_or(0, |json| json.len())
}

/// What that many bytes of conversation are estimated to be in tokens.
fn estimated_tokens(bytes: usize) -> u64 {
    u64::try_from(bytes).unwrap_or(u64::MAX) / ESTIMATED_BYTES_PER_TOKEN
}

/// The newest bytes of conversation that neither elision nor a summary may touch.
///
/// [`KEPT_RESULT_BYTES`] wherever the run can afford it, and half the target where it cannot: a
/// floor above the target it is meant to leave room under can never be met, so a small window
/// would protect the whole conversation and compaction would do nothing at all. At the byte rule's
/// own target the two are the same figure, so this changes nothing for a run with no window.
fn protected_bytes(target: usize) -> usize {
    KEPT_RESULT_BYTES.min(target / 2)
}

/// What one elision pass dropped, and what it left standing.
struct Elided {
    /// Tool results whose payload was replaced.
    count: usize,
    freed: usize,
    /// The newest results left whole, and their size.
    protected: usize,
    kept: usize,
}

/// Elides the oldest tool-result payloads until the conversation is under `target`.
///
/// `protect` bytes of the newest results are never touched — see [`protected_bytes`] — and at
/// least one result always survives, because a model that cannot see the result of the call it
/// just made is stuck.
fn elide(items: &mut [Item], target: usize, protect: usize) -> Elided {
    let results: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| matches!(item, Item::ToolResult { output, .. } if !is_elided(output)))
        .map(|(index, _)| index)
        .collect();
    // The newest results, whole, until they come to `protect` bytes — and always at least one,
    // because a model that cannot see the result of the call it just made is stuck.
    let mut kept = 0_usize;
    let mut protected = 0_usize;
    for &index in results.iter().rev() {
        let size = measure_one(&items[index]);
        if protected > 0 && kept + size > protect {
            break;
        }
        kept += size;
        protected += 1;
    }
    let elidable = results.len().saturating_sub(protected);
    let mut freed = 0_usize;
    let mut count = 0_usize;
    for index in results.into_iter().take(elidable) {
        // Down to the low-water mark, not to the bound: stopping at the bound leaves the next
        // result to cross it again, and the second rewrite costs a whole uncached replay.
        if measure(items) <= target {
            break;
        }
        let Item::ToolResult { output, .. } = &mut items[index] else {
            continue;
        };
        let was = serde_json::to_string(output).map_or(0, |json| json.len());
        *output = serde_json::json!({
            "elided": format!(
                "{was} bytes of this result were dropped to keep the conversation inside its \
                 bound. The call it answered is still above; read it again if you need it."
            ),
        });
        freed += was;
        count += 1;
    }

    Elided {
        count,
        freed,
        protected,
        kept,
    }
}

/// Where the **turn group** starting at `start` ends, exclusive.
///
/// A turn group is the smallest run of items that is still replayable on its own: the
/// [`Item::Opaque`] reasoning item(s) a turn emitted before it called anything, the run of
/// [`Item::ToolCall`]s it then made, and the [`Item::ToolResult`]s that answer exactly those
/// calls. A [`Item::UserText`] or [`Item::AssistantText`] is a group of one, and so is an opaque
/// item no call follows.
///
/// Results are matched by `call_id` rather than by position, because the two shapes a provider
/// produces — all the calls then all the results, or each call followed by its own result — are
/// both in use and the group has to close at the right place for either.
fn group_end(items: &[Item], start: usize) -> usize {
    let mut index = start;
    while index < items.len() && matches!(items[index], Item::Opaque { .. }) {
        index += 1;
    }
    if index >= items.len() || !matches!(items[index], Item::ToolCall(_)) {
        // Reasoning that no call follows stands alone, and so does a text item.
        return if index > start { index } else { start + 1 };
    }
    let mut awaiting: Vec<harness_wire::CallId> = Vec::new();
    while let Some(Item::ToolCall(call)) = items.get(index) {
        awaiting.push(call.call_id.clone());
        index += 1;
    }
    while !awaiting.is_empty() {
        let Some(Item::ToolResult { call_id, .. }) = items.get(index) else {
            break;
        };
        let Some(answered) = awaiting.iter().position(|id| id == call_id) else {
            break;
        };
        awaiting.remove(answered);
        index += 1;
    }
    index
}

/// Which indices a fold may stop at: `legal[i]` is true when nothing straddles the gap before `i`.
///
/// One pass over the conversation, so the boundaries are the group boundaries by construction
/// rather than by a walk-back that has to guess what it is standing in the middle of.
fn group_boundaries(items: &[Item]) -> Vec<bool> {
    let mut legal = vec![false; items.len() + 1];
    let mut index = 0_usize;
    while index < items.len() {
        legal[index] = true;
        index = group_end(items, index).max(index + 1);
    }
    legal[items.len()] = true;
    legal
}

/// Where a summary's fold stops, or [`None`] when there is nothing worth folding.
///
/// Three things are never folded: the task ([`FIRST_KEPT_ITEM`]), the newest `protect` bytes of the
/// conversation, and — whatever `protect` works out to — the **newest item**, because a model that
/// cannot see the result of the call it just made is stuck.
///
/// # The boundary may only fall between turn groups
///
/// Both halves of a tool round trip are provider errors on their own: a `function_call` replayed
/// without its output is one, an output replayed without its call is worse because nothing says
/// what it answers, and on the Responses wire a reasoning item that its call does not follow is a
/// third. Walking back over `ToolCall`s alone was not enough to avoid any of them —
/// `[…, call A, call B, result A, result B, …]` has a byte boundary between the two results that
/// stands over no `ToolCall` at all, and folding there orphans B's call from B's result **and**
/// puts a result at the head of the tail. So the boundary is snapped back to the nearest
/// [`group_boundaries`] index instead, which cannot fall inside a group of any of those shapes.
///
/// Snapping only ever makes the fold smaller, so it can end at [`FIRST_KEPT_ITEM`] and leave
/// nothing to fold. That is [`None`] and a skipped summary, which is the right answer: a
/// conversation whose whole tail is one unbreakable group has nothing a summary could take.
fn fold_end(items: &[Item], protect: usize) -> Option<usize> {
    let mut kept = 0_usize;
    let mut end = items.len().saturating_sub(1);
    while end > FIRST_KEPT_ITEM {
        let size = measure_one(&items[end - 1]);
        if kept + size > protect {
            break;
        }
        kept += size;
        end -= 1;
    }
    let legal = group_boundaries(items);
    while end > FIRST_KEPT_ITEM && !legal[end] {
        end -= 1;
    }
    // What is worth folding is decided in bytes, by `SUMMARY_MIN_FOLD_BYTES`: one 40kB assistant
    // argument is worth a turn and six short items are not.
    (end > FIRST_KEPT_ITEM).then_some(end)
}

/// What a summary attempt did.
enum Summarised {
    /// Nothing was folded: too little to be worth a turn, or all of it protected.
    Skipped,
    /// The prefix is now one summary item, in place of this many.
    Folded(usize),
    /// A turn was spent and produced nothing usable. The elided conversation stands.
    Failed,
    /// The caller cancelled. The run is over.
    Cancelled,
}

/// Whether this output has already been elided, so nothing is counted or elided twice.
fn is_elided(output: &serde_json::Value) -> bool {
    output.get("elided").is_some()
}

/// Refuses an oversized result by name rather than cutting it down.
///
/// A truncated result reads to the model exactly like a complete one, so it would answer from a
/// file it believes it saw the whole of. The same check for a call run on its own and for one run
/// in a batch: a bound that a faster path could get around is not a bound.
fn within_result_bound(call: &ToolCall, result: ToolOutcome) -> ToolOutcome {
    if exceeds(&result.output, MAX_TOOL_RESULT_BYTES) {
        return ToolOutcome::failed(format!(
            "the result of `{}` is over the {MAX_TOOL_RESULT_BYTES} byte bound; narrow the request",
            call.name
        ));
    }
    result
}

/// How a refusal names the call it is about.
///
/// The **invoked** entry, because that is what would have run and what a person — or a hook —
/// decided on, and the verb it came through when the two differ. A refusal that named only the
/// verb told the model `tool_invoke` was refused and never said which entry, so it either stopped
/// using the verb at all or retried the same entry against the same answer.
fn refused_name(call: &ToolCall, invoked: &ToolSpec) -> String {
    if invoked.name == call.name {
        format!("`{}`", call.name)
    } else {
        format!("`{}` (called through `{}`)", invoked.name, call.name)
    }
}

/// Puts an after-call hook's note where the model reads it: beside the result, never instead of it.
///
/// An object result grows a `hook_notes` array. Anything else — a string, a list, a number — is
/// wrapped as `{"output": <what the tool said>, "hook_notes": [<note>]}`, because a note appended
/// to a string would read to the model exactly like something the tool itself said.
///
/// An object that already carries a `hook_notes` which is **not** an array is wrapped too, rather
/// than having it overwritten: the tool said that, and replacing it would destroy an answer to
/// make room for a comment on it.
fn with_hook_note(result: ToolOutcome, note: String) -> ToolOutcome {
    let ToolOutcome {
        mut output,
        failed,
        refusal,
    } = result;
    let note = serde_json::Value::String(note);
    let joins = output.as_object().is_some_and(|fields| {
        fields
            .get("hook_notes")
            .is_none_or(serde_json::Value::is_array)
    });
    if joins {
        let fields = output.as_object_mut().expect("checked immediately above");
        match fields.get_mut("hook_notes") {
            Some(serde_json::Value::Array(notes)) => notes.push(note),
            _ => {
                fields.insert(
                    "hook_notes".to_owned(),
                    serde_json::Value::Array(vec![note]),
                );
            }
        }
    } else {
        let said = std::mem::take(&mut output);
        output = serde_json::json!({"output": said, "hook_notes": [note]});
    }
    // `failed` is the tool's own, untouched: a note is not a verdict, and a hook cannot make a
    // call that happened read as one that did not. Nor is the refusal: a note about a refused call
    // is a comment on the refusal, not a second opinion about whether it happened.
    ToolOutcome {
        output,
        failed,
        refusal,
    }
}

/// One delegate, resolved: everything the parent worked out before the child could be started.
///
/// The budget is deliberately absent. It is the one thing that cannot be decided per child in
/// isolation — a group divides the run's remainder between them ([`AgentLoop::carve`]) — so it is
/// set where the child's loop is built, from a figure that knows how many children there are.
struct Child {
    /// What the child is asked to do, which is also the whole of its first conversation.
    task: String,
    /// The child's configuration, carrying the parent's budget until the caller replaces it.
    config: LoopConfig,
}

/// How a delegate ended, as the parent has to report it.
///
/// Two variants rather than a `Result<LoopStop, _>` because the second one is not an error of the
/// parent's: a child that broke is a **tool result the model reads**, and the run it belongs to
/// goes on. What the parent must never do is let a child's failure end it — the model would be
/// left believing a sub-task it never got an answer to had succeeded.
enum ChildEnd {
    /// The child's own loop reached a stop, whether or not that stop is a completion.
    Stopped(LoopStop),
    /// The child could not run, or stopped being able to. The words go to the model as written.
    Broke(String),
}

impl ChildEnd {
    /// How a child that ran on this thread ended.
    fn of(ran: Result<LoopStop, LoopError>) -> Self {
        match ran {
            Ok(stop) => Self::Stopped(stop),
            Err(error) => Self::Broke(error.to_string()),
        }
    }
}

/// What one child's thread hands back.
///
/// Two layers of [`std::thread::Result`], and they are not the same failure. The inner one is the
/// child's own run panicking, caught inside the thread so that its siblings and the parent survive
/// it. The outer one is the thread itself failing outside that guard — the harness, not the child —
/// and it carries no state, because there was no run to have any.
type ChildJoin = std::thread::Result<(RunState, std::thread::Result<Result<LoopStop, LoopError>>)>;

/// Takes what a group of children spent, and turns each of them into the result the model reads.
///
/// In the order the model asked for them, whatever order they finished in: `answering[i]` says
/// which of the turn's calls the `i`th running child answers. What each contributes to the parent
/// is absorbed **however it ended**, for the reason [`AgentLoop::delegate`] gives — a child that
/// broke on its fourth turn still spent four.
fn absorb_children(
    calls: &[&ToolCall],
    answering: &[usize],
    ended: Vec<ChildJoin>,
    results: &mut [Option<ToolOutcome>],
    state: &mut RunState,
    sink: &mut dyn LoopSink,
) {
    for (&at, joined) in answering.iter().zip(ended) {
        let call = calls[at];
        let (end, turns) = match joined {
            Ok((mut child_state, ran)) => {
                state.absorb_child(&mut child_state);
                let end = match ran {
                    Ok(ran) => ChildEnd::of(ran),
                    Err(payload) => ChildEnd::Broke(format!(
                        "it panicked while running: {}. Whatever it did before that, it did.",
                        parallel::panic_words(payload.as_ref())
                    )),
                };
                (child_result(call, end, &child_state), child_state.turns)
            }
            Err(payload) => (
                child_result(
                    call,
                    ChildEnd::Broke(format!(
                        "it could not be run at all: {}",
                        parallel::panic_words(payload.as_ref())
                    )),
                    // Nothing ran, so there is nothing to report but the absence.
                    &RunState::resuming(Vec::new(), String::new()),
                ),
                0,
            ),
        };
        let (stop, result) = end;
        sink.emit(LoopEvent::DelegateFinished {
            call_id: call.call_id.clone(),
            stop,
            turns,
        });
        results[at] = Some(result);
    }
}

/// One delegate's outcome: the stop the record carries and the result the model reads.
///
/// Written once because two paths produce it — one child inside a tool call, and one of several
/// running side by side — and a model that could tell them apart from the result would be reading
/// something about the harness rather than about its sub-task.
fn child_result(call: &ToolCall, end: ChildEnd, child: &RunState) -> (LoopStop, ToolOutcome) {
    match end {
        ChildEnd::Stopped(stop) => {
            let result = ToolOutcome {
                output: serde_json::json!({
                    "stop": stop,
                    "turns": child.turns,
                    "text": child.text,
                }),
                // A bound the child hit, a wire error, a cancellation: the parent has to learn the
                // sub-task did not finish, or it reads a half-answer as a whole one.
                failed: !stop.is_completed(),
                // A delegate is not refused by a rule of the run's; whatever refusals happened
                // inside it were already reported as the child's own events.
                refusal: None,
            };
            // The same bound every result meets. The preamble is what tells the child to report
            // well inside it; this is what happens when it did not.
            (stop, within_result_bound(call, result))
        }
        // A run that could not proceed at all never reached a stop of its own, and
        // `DelegateFinished` has to carry one. `ProviderIncomplete` is the variant that already
        // means *this run ended early, and here is the reason in words* — the reason is what a
        // reader of the record needs, and inventing a variant would put a state in `LoopStop` that
        // no run can actually stop in.
        ChildEnd::Broke(reason) => (
            LoopStop::ProviderIncomplete {
                reason: reason.clone(),
            },
            ToolOutcome::failed(format!("the delegate could not run: {reason}")),
        ),
    }
}

/// One child's share of a remainder, or the ceiling's name when it will not divide that far.
///
/// [`None`] in is [`None`] out, as for [`remainder`]: an unset ceiling divides into unset ceilings.
/// A share of zero is refused rather than rounded up to one, because rounding up is how `share`
/// children each get the last token of a run that had one left.
fn divided(left: Option<u64>, share: u64, name: &'static str) -> Result<Option<u64>, &'static str> {
    let Some(left) = left else {
        return Ok(None);
    };
    match left / share {
        0 => Err(name),
        each => Ok(Some(each)),
    }
}

/// What is left of one ceiling once what has been spent against it is taken off, or the ceiling's
/// name when nothing is left.
///
/// [`None`] in is [`None`] out: a ceiling the parent never set is one the child does not get
/// either, rather than one carved down to zero.
fn remainder(
    limit: Option<u64>,
    spent: u64,
    name: &'static str,
) -> Result<Option<u64>, &'static str> {
    let Some(limit) = limit else {
        return Ok(None);
    };
    match limit.saturating_sub(spent) {
        0 => Err(name),
        left => Ok(Some(left)),
    }
}

/// Puts one call's answer in the record and in the conversation.
///
/// # A refusal the run made by rule is said out loud, before the result
///
/// A refusal is a failed outcome so the model learns the effect did not happen (invariant 9), and
/// on the record that made it `ToolCompleted { failed: true }` — the same shape as a compile error
/// or a missing file. *Did the surface refuse what is outside it?* was therefore unanswerable
/// without matching the sentence's text, and an evaluation asking it read `0 refusal(s)` on a run
/// where the refusal plainly happened.
///
/// So the named ones get a `Warning`, exactly as an unpublished tool does — and in the same order
/// as that one, which emits before the outcome it refuses with: **the warning, then the
/// `ToolCompleted`**. The code and the words are the refusal's own
/// ([`harness_wire::Refusal::code`], [`harness_wire::Refusal::message`]), so the record's sentence
/// and the model's are one string. Nothing about the call changes: it still failed, and it still
/// says so.
fn complete(call: &ToolCall, result: ToolOutcome, state: &mut RunState, sink: &mut dyn LoopSink) {
    completed(call, &result, sink);
    state.items.push(Item::result(call.call_id.clone(), result));
}

/// What the record hears when a call is over: the refusal it met by name, if one, then that it
/// completed and whether it failed. Shared by a turn's calls and by [`AgentLoop::call`], so a call
/// made outside a turn leaves exactly the events a model's would.
fn completed(call: &ToolCall, result: &ToolOutcome, sink: &mut dyn LoopSink) {
    if let Some(refusal) = &result.refusal {
        sink.emit(LoopEvent::Warning {
            code: refusal.code().to_owned(),
            message: refusal.message(),
        });
    }
    sink.emit(LoopEvent::ToolCompleted {
        call_id: call.call_id.clone(),
        failed: result.failed,
    });
}

/// What every other call of a turn that carried the run's answer is told.
///
/// The same sentence whether the call sat before the answer or after it: the answer tool's own
/// description is what the model read before it made the call — *call it alone, as the last thing:
/// any other call in the same turn is refused* — so it is what it is told after.
///
/// It says the calls were made beside the answer and **not** that the run ended, because the
/// siblings are refused before the answer's own outcome is known and an `answer` can still fail:
/// arguments that are not an object, arguments over [`MAX_TOOL_ARGUMENT_BYTES`], a `before-call`
/// hook that blocked it. Each of those leaves the run turning, and a model told *the run ended*
/// for its `file_write` and *call it again* for its `answer` in the same turn reads two things
/// that cannot both be true — and may redo the write, or stop.
fn refused_beside_the_answer(name: &harness_wire::ToolName) -> String {
    format!("refused: made in the same turn as `{name}`, which must be called alone")
}

/// Answers every call that will not run, so the conversation stays replayable.
///
/// Each one enters the **record** as well as the conversation: a refused call is still a call the
/// model made, and a reader counting [`LoopEvent::ToolRequested`] against the [`Item::ToolCall`]s
/// of a finished run would otherwise find calls in the conversation the record never mentions, with
/// no way to tell which of them went unreported.
fn refuse_rest(calls: &[ToolCall], why: &str, state: &mut RunState, sink: &mut dyn LoopSink) {
    for skipped in calls {
        sink.emit(LoopEvent::ToolRequested(skipped.clone()));
        complete(skipped, ToolOutcome::failed(why), state, sink);
    }
}

/// The one refusal a narrowing gives, wherever it is decided.
///
/// A failed [`ToolOutcome`] and never an error (`AGENTS.md` invariant 9): the model has to read
/// that the effect did not happen, and a run ended instead would leave it believing the call
/// landed. `name` is the **invoked entry's** — `file_write`, not the `tool_invoke` it arrived
/// through — so the message a verb surface gives is the message a flat one gives.
///
/// Free rather than a method so that every site enforcing the narrowing produces the same two
/// sentences from the same place: the warning code a consumer filters on and the wording the model
/// learns from are the parts most easily made to disagree by being written twice.
fn refuse_unadmitted(name: &harness_wire::ToolName, sink: &mut dyn LoopSink) -> ToolOutcome {
    sink.emit(LoopEvent::Warning {
        code: "unpublished-tool".to_owned(),
        message: format!("the model called `{name}`, which this run was not admitted"),
    });
    ToolOutcome::failed(format!(
        "`{name}` is not one of this run's tools; call only what was published"
    ))
}

/// The call a run asks its tool port about to find out whether it would answer to `name`.
///
/// A question, never an invocation: [`ToolPort::invoked`] only reads a call to say which entry
/// *would* run, so nothing happens on the machine and no argument is needed. The id says as much
/// to any port that logs what it was asked, and empty arguments are what a call that will not be
/// made carries.
fn owned_name_probe(name: &harness_wire::ToolName) -> ToolCall {
    ToolCall {
        call_id: CallId::new("loop-owned-name-probe").expect("a constant probe id is legal"),
        name: name.clone(),
        arguments: serde_json::json!({}),
    }
}

/// Keeps an oversized call out of the conversation that gets replayed.
///
/// The call still happened and the model still has to see that it did, so the item stays. Its
/// payload does not: the conversation is resent whole on every turn, so retaining it would make
/// the next turn refuse for the same reason, and the run could never recover from one bad call.
fn recordable(item: Item) -> Item {
    match item {
        Item::ToolCall(call) if exceeds(&call.arguments, MAX_TOOL_ARGUMENT_BYTES) => {
            Item::ToolCall(ToolCall {
                arguments: serde_json::json!({
                    "omitted": format!(
                        "these arguments were over the {MAX_TOOL_ARGUMENT_BYTES} byte bound and \
                         were not retained"
                    ),
                }),
                ..call
            })
        }
        other => other,
    }
}

pub struct AgentLoop<'a> {
    model: &'a mut dyn ModelPort,
    tools: &'a mut dyn ToolPort,
    approvals: &'a mut dyn ApprovalPort,
    /// The operator's hooks, when a shell attached any. [`None`] behaves as [`NoHooks`].
    hooks: Option<&'a mut dyn HookPort>,
    config: LoopConfig,
    cancel: LoopCancel,
    /// Whether this loop is a delegate's, running inside another loop's tool call.
    ///
    /// Set by [`AgentLoop::delegate`] and by nothing else — it is not a caller's choice, because
    /// *being somebody's sub-task* is not something a caller can declare about a run it starts
    /// itself. What it changes is [`AgentLoop::stop_hook`]; the other two hook points fire in a
    /// child exactly as in a parent.
    nested: bool,
}

/// Projects the wire's live stream into the loop's own event stream.
struct Forward<'s>(&'s mut dyn LoopSink);

impl StreamSink for Forward<'_> {
    fn emit(&mut self, event: StreamEvent) {
        self.0.emit(match event {
            StreamEvent::TextDelta { text } => LoopEvent::TextDelta { text },
            StreamEvent::ToolArgumentsDelta { call_id, delta } => {
                LoopEvent::ToolArgumentsDelta { call_id, delta }
            }
            StreamEvent::ReasoningDelta { text } => LoopEvent::ReasoningDelta { text },
            StreamEvent::Warning { code, message } => LoopEvent::Warning { code, message },
        });
    }
}

/// Swallows the live stream of a turn nobody asked for.
///
/// A summary turn is the harness talking to the model about its own record, and its text is not an
/// answer to anybody: forwarded as [`LoopEvent::TextDelta`] it would appear in a terminal exactly
/// like the assistant replying, and in a bridge as an agent message the client would show a person.
/// Nothing is lost by dropping it — the text is read from the turn's items and put in the
/// conversation. A warning still comes through, because a wire that saw something it did not
/// understand has to say so wherever it happened.
struct Quiet<'s>(&'s mut dyn LoopSink);

impl StreamSink for Quiet<'_> {
    fn emit(&mut self, event: StreamEvent) {
        if let StreamEvent::Warning { code, message } = event {
            self.0.emit(LoopEvent::Warning { code, message });
        }
    }
}

/// Everything a delegate emits, wrapped, so that nothing of the child's arrives bare.
///
/// A child's [`LoopEvent::TextDelta`] is not the parent's answer and its [`LoopEvent::Usage`] is
/// not one of the parent's turns; forwarded unwrapped, a renderer would show the sub-task's
/// working as the run's reply and a reader summing top-level `Usage` events would count the
/// child's turns twice — once here and once inside [`LoopOutcome::usage`], which does include
/// them. Wrapping is what lets a renderer indent, a JSONL record nest, and a bridge ignore.
struct Wrapped<'s> {
    call_id: CallId,
    sink: &'s mut dyn LoopSink,
}

impl LoopSink for Wrapped<'_> {
    fn emit(&mut self, event: LoopEvent) {
        self.sink.emit(LoopEvent::Delegated {
            call_id: self.call_id.clone(),
            event: Box::new(event),
        });
    }
}

/// One turn attempt's result: the turn, or a reason the run is over.
enum Attempt {
    Turn(TurnOutcome),
    Stopped(LoopStop),
}

/// Which of the loop's own tools a call named.
///
/// Resolved before the port is consulted at all, because neither is a port call: `answer` ends the
/// run and `delegate` holds the model port for the length of a whole second run.
enum Owned {
    /// The schema this run publishes, carried for the same reason [`Owned::Delegate`] carries its
    /// delegation: what the call needs is read once, here, rather than out of `self.config` at
    /// every step of running it.
    Answer(OutputSchema),
    /// The delegation this run is configured with, carried so the call does not have to read it
    /// back out of `self.config` while the child holds every one of the loop's ports.
    Delegate(Delegation),
    /// The skills this run publishes, carried for the same reason as the two above.
    Skill(Skills),
}

impl Owned {
    /// The spec the loop published for this tool.
    ///
    /// What a hook is shown and what a refusal names, exactly as [`ToolPort::invoked`]'s answer is
    /// for a port call: an operator whose hook file says `"tools": ["delegate"]` has to be shown
    /// `delegate`, and a refusal has to name what did not run.
    fn spec(&self) -> ToolSpec {
        match self {
            Self::Answer(schema) => schema.spec(),
            Self::Delegate(delegation) => delegation.spec(),
            Self::Skill(skills) => skills.spec(),
        }
    }
}

/// Answers the run's own `skill` call with what the skill says.
///
/// Reads nothing from a filesystem: the bodies were loaded before the run started, by the caller,
/// so a skill cannot change under a run that is already using it and a loop cannot be made to read
/// a path the model named.
///
/// A name this run does not have is a **failed outcome, not a stop**: the model asked for
/// something that is not there, the list it should have chosen from is in its own instructions,
/// and it can choose again on the next turn. The refusal names what is available rather than only
/// what was wrong, because a model told "no" and not "these" spends the next turn guessing.
fn load_skill(skills: &Skills, call: &ToolCall) -> ToolOutcome {
    let Some(name) = call
        .arguments
        .get("name")
        .and_then(serde_json::Value::as_str)
    else {
        return ToolOutcome::failed(format!(
            "`{}` takes a `name`, a string, naming one of: {}.",
            call.name,
            skills.names().join(", ")
        ));
    };
    match skills.body(name) {
        Some(body) => ToolOutcome::ok(serde_json::Value::String(body.to_owned())),
        None => ToolOutcome::failed(format!(
            "`{name}` is not a skill this run has. It has: {}.",
            skills.names().join(", ")
        )),
    }
}

/// Answers the run's own `answer` call: the arguments **are** the answer.
///
/// Nothing here parses or validates them against the schema — what the provider accepted as tool
/// arguments is what the caller gets (design 0002 § 1, M3).
///
/// Returns the outcome the model reads and, with it, whether the run ends here. Recording is the
/// caller's, because an owned call meets the `after-call` hook between the outcome and the record
/// exactly as a port call does.
fn accept_answer(call: &ToolCall, state: &mut RunState) -> (ToolOutcome, Option<LoopStop>) {
    if !call.arguments.is_object() {
        // A failed outcome rather than a stop: the run has not answered, and the model is the one
        // that can fix it on the next turn.
        return (
            ToolOutcome::failed(format!(
                "the arguments of `{}` must be a JSON object in the shape its schema gives, and \
                 these are not an object; call it again with one",
                call.name
            )),
            None,
        );
    }
    state.structured = Some(call.arguments.clone());
    // Answered like any other call, because a `function_call` replayed without its output is a
    // provider error on the next turn — and a run that answered can still be resumed.
    (
        ToolOutcome::ok(serde_json::json!({"accepted": true})),
        Some(LoopStop::Completed),
    )
}

impl<'a> AgentLoop<'a> {
    pub fn new(
        model: &'a mut dyn ModelPort,
        tools: &'a mut dyn ToolPort,
        approvals: &'a mut dyn ApprovalPort,
        config: LoopConfig,
    ) -> Self {
        Self {
            model,
            tools,
            approvals,
            hooks: None,
            config,
            cancel: LoopCancel::new(),
            nested: false,
        }
    }

    pub fn cancel_handle(&self) -> LoopCancel {
        self.cancel.clone()
    }

    #[must_use]
    pub fn with_cancel(mut self, cancel: LoopCancel) -> Self {
        self.cancel = cancel;
        self
    }

    /// Attaches the operator's hooks (design 0002 § 3). Without this the loop consults none.
    #[must_use]
    pub fn with_hooks(mut self, hooks: &'a mut dyn HookPort) -> Self {
        self.hooks = Some(hooks);
        self
    }

    /// Runs **one call outside any turn**, through the gate a model's call meets.
    ///
    /// # What this is for
    ///
    /// A workflow's `command` step is a program the document names — a verifier, a validator — and
    /// not a question for a model (`harness-cli` design 0003 § 6, M2). Nothing is sent to the
    /// provider. The call is the caller's, and it runs through [`AgentLoop::invoke`]'s stages in
    /// their order — published or routed, the argument bound, the approver, the operator's
    /// `before-call` hook, the tool, the result bound, the `after-call` hook — and leaves the
    /// record a model's call leaves: `ToolRequested`, then `ToolCompleted`, with a `Warning` naming
    /// any refusal between them. A caller that reached the port directly would be a second answer
    /// to *what may this run do*, and the approver and the hooks would never hear of it.
    ///
    /// The clock is the budget's `max_duration_ms`, measured from now: this is one call and not a
    /// run, so there is no earlier start to measure from. The conversation is the caller's to keep
    /// — the result comes back and is pushed nowhere — because a call no turn asked for belongs
    /// wherever the caller files it.
    pub fn call(&mut self, call: &ToolCall, sink: &mut dyn LoopSink) -> ToolOutcome {
        sink.emit(LoopEvent::ToolRequested(call.clone()));
        let deadline = self
            .config
            .budget
            .max_duration_ms
            .map(|millis| Instant::now() + Duration::from_millis(millis));
        let result = self.invoke(call, deadline, sink);
        completed(call, &result, sink);
        result
    }

    /// Runs until the model answers, a budget binds, or the caller cancels.
    ///
    /// # Errors
    ///
    /// Returns [`LoopError::Config`] and [`LoopError::Budget`] before the first request, for a
    /// run that could not be described and for one whose bound is unusable, and
    /// [`LoopError::Wire`] when a turn could not be obtained at all. A budget that *binds* is an
    /// outcome, not an error.
    pub fn run(
        &mut self,
        input: impl Into<String>,
        sink: &mut dyn LoopSink,
    ) -> Result<LoopOutcome, LoopError> {
        // A caller who did not lend a conversation did not ask for a spend either: when this run
        // answers, both are in the outcome, and when it fails there is nobody holding either.
        self.run_in(&mut Vec::new(), &mut RunLedger::default(), input, sink)
    }

    /// [`run`](Self::run), continuing a conversation the caller is holding and reporting what the
    /// run spent.
    ///
    /// `items` is the conversation so far — empty for a first run, a resumed session's items for a
    /// following one. The new input is appended to it and the run proceeds exactly as
    /// [`run`](Self::run) does. `spend` is where this run's usage, cost and turns are written.
    ///
    /// # Why the caller's vector and the caller's ledger, rather than the outcome
    ///
    /// **This is what lets a shell persist what a failed run had, and what it cost.**
    /// [`LoopError`] carries neither items nor usage: a turn that broke on the wire returns an
    /// error, the [`LoopOutcome`] is never built, and every turn before it is gone. That is the
    /// twenty-turn run a network blip threw away. So both are taken out of the caller's hands into
    /// the run's state and written back on **every** exit path — an unusable budget before the
    /// first request, a wire failure on turn twenty, or an answer — and the caller can save
    /// whatever there is.
    ///
    /// The spend is handed back for the same reason the conversation is, and this is the same
    /// defect [`RunState::absorb_child`] fixes one level down for a delegate: those nineteen turns
    /// were billed, their [`LoopEvent::Usage`] and [`LoopEvent::Cost`] events are already in the
    /// record the caller streamed, and a session file holding the conversation but not the figures
    /// would report a failed run as free.
    ///
    /// Items replace rather than append: the loop replays the whole conversation each turn and its
    /// state holds all of it, so appending would store every earlier turn twice. [`RunLedger`] is
    /// replaced too and holds **this** run only — a resumed run is not billed again for the turns
    /// it replays, so folding one run's figures into a session's is the caller's own arithmetic.
    ///
    /// # Errors
    ///
    /// As [`run`](Self::run).
    pub fn run_in(
        &mut self,
        items: &mut Vec<Item>,
        spend: &mut RunLedger,
        input: impl Into<String>,
        sink: &mut dyn LoopSink,
    ) -> Result<LoopOutcome, LoopError> {
        let mut state = RunState::resuming(std::mem::take(items), input);
        let ended = self.run_over(&mut state, sink);
        // Written before the two arms part, so that neither can be the one that forgets. The
        // conversation still has to be handed back inside them — one arm owns the state and the
        // other the outcome — but the spend is the same figure whichever way the run ended.
        *spend = state.ledger();
        match ended {
            Ok(stop) => {
                let outcome = state.into_outcome(stop);
                items.clone_from(&outcome.items);
                Ok(outcome)
            }
            Err(error) => {
                *items = state.items;
                Err(error)
            }
        }
    }

    /// One whole run over a [`RunState`] the caller keeps hold of, ending in `Finished`.
    ///
    /// The seam a delegate runs its child over. [`run_in`](Self::run_in) exists to hand the
    /// **conversation** back on every exit path; this exists to hand the *whole* state back, which
    /// is what a parent needs from a child that failed: the child's [`LoopEvent::Usage`] and
    /// [`LoopEvent::Cost`] events have already been emitted, so the spend behind them has to be
    /// readable however the child ended (see [`RunState::absorb_child`]). Nothing outside this
    /// crate can reach it — [`run`](Self::run) and [`run_in`](Self::run_in) are unchanged.
    fn run_over(
        &mut self,
        state: &mut RunState,
        sink: &mut dyn LoopSink,
    ) -> Result<LoopStop, LoopError> {
        let stop = self.drive_run(state, sink)?;
        sink.emit(LoopEvent::Finished {
            stop: stop.clone(),
            turns: state.turns,
        });
        Ok(stop)
    }

    /// One run, from the budget check to the stop, over a state the caller owns.
    ///
    /// Separate from [`run_in`](Self::run_in) so that every way out of a run — including the two
    /// that are errors — passes through one place that hands the conversation back.
    fn drive_run(
        &mut self,
        state: &mut RunState,
        sink: &mut dyn LoopSink,
    ) -> Result<LoopStop, LoopError> {
        // First of all, because it decides whether the run can be *described* at all: a duplicate
        // tool name is a request no wire can accept, and finding that out on turn one costs a
        // turn to learn something knowable before the first.
        self.check_owned_names()?;
        // The run's own model is what the ceiling is judged against, because it is the only model
        // known before the first turn. An endpoint that answers as something else is caught by
        // `RateCard::price` and reported as an unpriced turn.
        let priced = self
            .config
            .prices
            .as_ref()
            .is_some_and(|card| card.rates_for(&self.config.model).is_some());
        self.config.budget.validate(priced)?;
        let deadline = self
            .config
            .budget
            .max_duration()
            .map(|span| Instant::now() + span);

        // Before `Started`, because it happened before the run: the credential was stale, it was
        // renewed, and somebody's file on disk was rewritten — all of it upstream of the first
        // request. A reader of the record sees the acts in the order they occurred.
        if let Some(renewal) = self.config.credential_renewal.clone() {
            sink.emit(LoopEvent::CredentialRenewed(renewal));
        }
        sink.emit(LoopEvent::Started {
            model: self.config.model.clone(),
            // The loop's own tools among them: the event answers *what was the model offered*,
            // and a reader who saw `delegate` in the record but not here would have to guess
            // where it came from.
            // **After narrowing, not before.** This answers *what was the model offered*, and a
            // narrowed child offered four of its parent's seven must not be recorded as having
            // had seven — that is the record saying a run could do things it could not.
            published_tools: self
                .port_specs()
                .into_iter()
                .map(|spec| spec.name)
                .chain(self.owned_specs().into_iter().map(|spec| spec.name))
                .collect(),
            operations: self
                .tools
                .operations()
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
            // The other half of the same question. The two lists above say what the run has; a
            // reader cannot tell an absence that was refused from an absence nobody wanted
            // without this one.
            withheld: self.config.withheld.clone(),
            // What the run was given to reach for beyond its tools. Named here rather than left
            // to be inferred from `published_tools`: `skill` appearing there says a skill tool
            // exists, not which skills it can load, and those are the question an evaluation
            // comparing two harnesses actually asks.
            skills: self
                .config
                .skills
                .as_ref()
                .map(Skills::names)
                .unwrap_or_default(),
            agents: self
                .config
                .agents
                .as_ref()
                .map(Agents::names)
                .unwrap_or_default(),
            profiles: self.config.profiles.clone(),
            credential_source: self.config.credential_source.clone(),
        });
        self.announce_prices(priced, sink);

        self.drive(state, deadline, sink)
    }

    /// Puts the rates in the record, and says so by name when they miss this model.
    ///
    /// The warning is the point. Without it a run against an unpriced model reports no cost, and a
    /// reader has no way to tell that from a run that cost nothing — so the one number an
    /// evaluation compares arms on would quietly be missing.
    fn announce_prices(&self, priced: bool, sink: &mut dyn LoopSink) {
        let Some(card) = self.config.prices.as_ref() else {
            return;
        };
        sink.emit(LoopEvent::Rates {
            source: card.source.clone(),
            as_of: card.as_of.clone(),
        });
        if priced {
            return;
        }
        let known: Vec<&str> = card.priced_models().collect();
        sink.emit(LoopEvent::Warning {
            code: "unpriced-model".to_owned(),
            message: format!(
                "the rate card of {} does not price `{}`, so this run reports no cost; it prices: {}",
                card.as_of,
                self.config.model,
                if known.is_empty() {
                    "nothing".to_owned()
                } else {
                    known.join(", ")
                }
            ),
        });
    }

    /// Turns, tool calls, turns again, until something says stop.
    fn drive(
        &mut self,
        state: &mut RunState,
        deadline: Option<Instant>,
        sink: &mut dyn LoopSink,
    ) -> Result<LoopStop, LoopError> {
        loop {
            if let Some(stop) = self.stop_before_turn(state, deadline) {
                return Ok(stop);
            }

            // Before the request is built, so the turn that pays for a compaction is the one
            // that benefits from it.
            if let Some(stop) = self.compact_run(state, sink) {
                return Ok(stop);
            }
            // A summary turn is charged to the run like any other, so a ceiling it crosses binds
            // here rather than after the next turn's tool calls. Checked only after the ceilings
            // are counted, this was the one turn whose spend nothing looked at: a run could
            // overshoot `max_cost` or `max_input_tokens` by a summary turn *and* a full
            // conversation turn before anything stopped it.
            if let Some(stop) = self.stop_after_tokens(state) {
                return Ok(stop);
            }
            state.turns += 1;
            sink.emit(LoopEvent::TurnStarted { turn: state.turns });
            let outcome = match self.attempt_turn(state, deadline, sink)? {
                Attempt::Turn(outcome) => outcome,
                Attempt::Stopped(stop) => return Ok(stop),
            };

            let (calls, stop_reason) = state.absorb(outcome, self.config.prices.as_ref(), sink);
            let stop = if calls.is_empty() {
                Some(terminal_stop(stop_reason))
            } else {
                self.run_calls(&calls, state, deadline, sink)
            };
            // A run ending `Completed` is the one ending something may still have a say in: the
            // answer nudge (design 0002 § 1), then the stop hook (§ 3). Both are bounded, and
            // either turns the run again.
            if let Some(stop) = stop
                && let Some(stop) = self.ending(stop, state, sink)
            {
                return Ok(stop);
            }
            if let Some(stop) = self.stop_after_tokens(state) {
                return Ok(stop);
            }
        }
    }

    /// Runs one turn, attempting it again when the wire says the failure was retriable.
    ///
    /// # Why the retry lives here and not in the wire
    ///
    /// A wire deliberately stops retrying the moment it has emitted anything: a second attempt
    /// would append a second copy of text a person has already read, and its `WitnessedSink` makes
    /// that impossible to get wrong. The consequence was that a network blip on turn 20 of a long
    /// run ended the run — the loop mapped any `Err` to [`LoopError::Wire`] and exited, and nothing
    /// persisted the conversation to resume from.
    ///
    /// The decision belongs one level up, because up here the thing a retry would duplicate is
    /// known to be discardable: `state.items` is **unchanged** by a failed turn, so the second
    /// attempt is not a resumption but the same request again. What a person already saw is the
    /// one thing that cannot be taken back, so [`LoopEvent::TurnRetried`] says out loud that the
    /// turn's stream is to be discarded, and a renderer acts on it.
    ///
    /// [`MAX_TURN_RETRIES`] extra attempts, then the failure the run already had stands. A
    /// non-retriable failure — a rejected key, a protocol error — is not attempted again at all;
    /// nothing about waiting would change it.
    fn attempt_turn(
        &mut self,
        state: &RunState,
        deadline: Option<Instant>,
        sink: &mut dyn LoopSink,
    ) -> Result<Attempt, LoopError> {
        let request = self.request(state);
        let mut retries = 0_u32;
        loop {
            let error = {
                let mut forward = Forward(sink);
                match self.model.turn(&request, &mut forward) {
                    Ok(outcome) => return Ok(Attempt::Turn(outcome)),
                    // A read that stopped because the caller cancelled is them getting what they
                    // asked for. Reporting it as a failure would tell a person who pressed Ctrl-C
                    // that something went wrong.
                    Err(error) if error.code == WireErrorCode::Cancelled => {
                        return Ok(Attempt::Stopped(cancelled()));
                    }
                    Err(error) => error,
                }
            };
            if !error.retriable || retries >= MAX_TURN_RETRIES {
                return Err(error.into());
            }
            if self.cancel.is_cancelled() {
                return Ok(Attempt::Stopped(cancelled()));
            }
            retries += 1;
            sink.emit(LoopEvent::TurnRetried {
                turn: state.turns,
                attempt: retries,
                reason: error.message.clone(),
            });
            if let Some(stop) = self.back_off(retries, deadline) {
                return Ok(Attempt::Stopped(stop));
            }
        }
    }

    /// Waits before the next attempt, and says when the run ended inside the wait instead.
    ///
    /// Doubling from [`LoopConfig::retry_backoff`] and capped at [`MAX_TURN_RETRY_BACKOFF`],
    /// because a wire that just failed on transport is usually one whose other side is briefly
    /// unwell and an immediate second request is the least likely to be answered.
    ///
    /// Slept in [`CANCEL_POLL`] slices so a Ctrl-C is honoured inside the pause, and the deadline
    /// is read each slice: an attempt is never *started* past the wall clock the caller bought,
    /// because a request begun at the deadline runs to the wire's timeout, not to the budget's.
    fn back_off(&self, attempt: u32, deadline: Option<Instant>) -> Option<LoopStop> {
        let pause = self
            .config
            .retry_backoff
            .saturating_mul(2_u32.saturating_pow(attempt.saturating_sub(1)))
            .min(MAX_TURN_RETRY_BACKOFF);
        let until = Instant::now() + pause;
        loop {
            if self.cancel.is_cancelled() {
                return Some(cancelled());
            }
            if let Some(stop) = self.deadline_passed(deadline) {
                return Some(stop);
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            std::thread::sleep(left.min(CANCEL_POLL));
        }
    }

    fn request(&self, state: &RunState) -> TurnRequest {
        let mut tools = self.port_specs();
        tools.extend(self.owned_specs());
        let tool_choice = self.held_to_the_answer(state);
        TurnRequest {
            model: self.config.model.clone(),
            instructions: self.config.instructions.clone(),
            items: state.items.clone(),
            tools,
            max_output_tokens: self.config.budget.max_output_tokens_per_turn,
            sampling: self.config.sampling.clone(),
            tool_choice,
        }
    }

    /// Holds the **turn after a nudge** to the answer tool, and no other turn.
    ///
    /// # Why after the nudge and not from the start
    ///
    /// A tool choice naming a tool means *call it now*. From the first turn that is a run that
    /// answers before it does anything: the model would call `answer` on turn one and the work the
    /// run was for would never happen. The nudge is the only moment where *call it now* is exactly
    /// what the loop means — the model has already said it is finished, in prose, and the only
    /// thing left is to say it in the shape the caller asked for.
    ///
    /// So this is asking twice, not asking harder: the first ask is the tool's description, the
    /// second is the provider's own constraint. The seventh paid native walk (2026-08-30) ended in
    /// prose on three of four attempts at one section under the nudge alone, which is what
    /// `ROADMAP.md` Phase 7 said would decide whether this is worth a contract version.
    ///
    /// **It costs the nudge turn's cache**, on a route where changing this field invalidates the
    /// cached prefix — one turn per run at most, and only on a run that was otherwise about to
    /// report nothing. Nothing else in the run sees a different request.
    fn held_to_the_answer(&self, state: &RunState) -> harness_wire::ToolChoice {
        match self.config.output_schema.as_ref() {
            Some(schema) if state.nudged > 0 && state.structured.is_none() => {
                harness_wire::ToolChoice::Named(schema.name.clone())
            }
            _ => harness_wire::ToolChoice::Auto,
        }
    }

    /// The tools the loop owns, in the order they are appended after the port's.
    ///
    /// After, not before, so that the catalogue the run is actually for reads first and a
    /// published set stays byte-identical to what it was on a run that asked for neither. Both are
    /// inside [`harness_wire::MAX_TOOLS`] and the duplicate-name check for free, because
    /// [`TurnRequest::validate`] sees exactly this list.
    ///
    /// A delegation at depth 0 publishes nothing: that is the child of a one-level delegation, and
    /// what makes the tree one level deep rather than unbounded.
    fn owned_specs(&self) -> Vec<ToolSpec> {
        let mut specs = Vec::new();
        if let Some(schema) = self.config.output_schema.as_ref() {
            specs.push(schema.spec());
        }
        if let Some(delegation) = self.config.delegation.as_ref()
            && delegation.depth > 0
        {
            // The names this run actually has, so the `agent` argument exists only where it can
            // be answered and carries an `enum` a provider can refuse against.
            let agents = self
                .config
                .agents
                .as_ref()
                .map(Agents::names)
                .unwrap_or_default();
            specs.push(delegation.spec_with_agents(&agents));
        }
        // A set with nothing in it publishes nothing: a `skill` tool whose only legal argument is
        // an empty enum is a tool the model can only be refused by.
        if let Some(skills) = self.config.skills.as_ref()
            && !skills.is_empty()
        {
            specs.push(skills.spec());
        }
        specs
    }

    /// Refuses, by name, a run whose port already answers to a name the loop's own tools need.
    ///
    /// Before any byte goes out, because the alternative is a request carrying the same tool name
    /// twice: the wire refuses it as a protocol error on the first turn, having already been
    /// paid for nothing, and the message names the duplicate rather than the mistake.
    ///
    /// # `specs()` is not the whole of what a port answers to
    ///
    /// Under the three-verb surface `specs()` is three verbs, and the port still resolves a bare
    /// **entry** name — the routed path [`AgentLoop::invoke`] warns about, 12 % of calls in a
    /// measured run. A catalogue entry called `answer` or `delegate` would put nothing in the
    /// request twice, so nothing would refuse it; it would simply be unreachable for ever, because
    /// the loop resolves its own tools first and the call would never reach the port. That is the
    /// silent shadowing this second probe is for, and it is why the probe asks
    /// [`ToolPort::invoked`] rather than reading a published list.
    ///
    /// # Errors
    ///
    /// [`LoopError::Config`], naming the tool two things want.
    fn check_owned_names(&self) -> Result<(), LoopError> {
        let owned = self.owned_specs();
        for (index, spec) in owned.iter().enumerate() {
            if self
                .tools
                .specs()
                .iter()
                .any(|published| published.name == spec.name)
            {
                return Err(LoopError::Config(format!(
                    "this run's tool port publishes `{name}`, which is the name the loop's own \
                     `{name}` needs; the model could address neither, so rename one of them",
                    name = spec.name
                )));
            }
            if self.tools.invoked(&owned_name_probe(&spec.name)).is_some() {
                return Err(LoopError::Config(format!(
                    "this run's tool port resolves `{name}` to an entry of its own, which is the \
                     name the loop's own `{name}` needs; the loop resolves its own tools first, so \
                     that entry could never be reached — rename one of them",
                    name = spec.name
                )));
            }
            // And the owned ones against each other, because a caller may name both: two tools of
            // one name is the same unaddressable request, with nobody else to blame for it.
            if owned[..index].iter().any(|other| other.name == spec.name) {
                return Err(LoopError::Config(format!(
                    "this run's answer schema and its delegation are both published as `{}`; the \
                     model could address neither, so rename one of them",
                    spec.name
                )));
            }
        }
        Ok(())
    }

    fn stop_before_turn(&self, state: &RunState, deadline: Option<Instant>) -> Option<LoopStop> {
        if self.cancel.is_cancelled() {
            return Some(cancelled());
        }
        if let Some(limit) = self.config.budget.max_turns
            && state.turns >= limit
        {
            return Some(LoopStop::MaxTurns { limit });
        }
        self.deadline_passed(deadline)
    }

    /// The deadline as a stop, once the clock has passed it. One reading of the budget, so the
    /// check between turns and the check between calls cannot disagree about what binds.
    fn deadline_passed(&self, deadline: Option<Instant>) -> Option<LoopStop> {
        let (Some(deadline), Some(limit_ms)) = (deadline, self.config.budget.max_duration_ms)
        else {
            return None;
        };
        (Instant::now() >= deadline).then_some(LoopStop::Deadline { limit_ms })
    }

    /// Token ceilings bind after a turn, because that is when the provider reports.
    fn stop_after_tokens(&self, state: &RunState) -> Option<LoopStop> {
        if let (Some(limit), Some(spent)) =
            (self.config.budget.max_cost_microunits, state.cost_total)
            && spent > limit
        {
            return Some(LoopStop::MaxCost {
                limit_micro_usd: limit,
                spent_micro_usd: spent,
            });
        }
        if let Some(limit) = self.config.budget.max_input_tokens
            && state.input_total > limit
        {
            return Some(LoopStop::MaxInputTokens {
                limit,
                reported: state.input_total,
            });
        }
        if let Some(limit) = self.config.budget.max_output_tokens
            && state.output_total > limit
        {
            return Some(LoopStop::MaxOutputTokens {
                limit,
                reported: state.output_total,
            });
        }
        None
    }

    /// Makes the conversation smaller before the next request is built.
    ///
    /// # Two rules, and which one a run gets
    ///
    /// With no [`LoopConfig::context_window`] declared, the byte rule stands unchanged: past
    /// [`MAX_CONVERSATION_BYTES`], elide the oldest tool-result payloads down to
    /// [`COMPACTED_TARGET_BYTES`]. That rule was fixed at 192 KiB — about 50k tokens — so about
    /// 60 % of a 128k window was never reachable and a run whose weight was not in tool results
    /// had no strategy at all: it met the provider's wall as a hard error.
    ///
    /// With a window declared, the trigger is tokens. It fires at
    /// [`COMPACTION_TRIGGER_PERCENT`] of the window, measured by the provider's own last reported
    /// input count where there is one and by [`ESTIMATED_BYTES_PER_TOKEN`] where there is not.
    /// Elision goes first, because it costs nothing; a summary turn is spent only where the weight
    /// is in things elision may not touch — user and assistant text, and the opaque reasoning items
    /// this loop carries verbatim across every tool round trip.
    ///
    /// Returns a stop only when the caller cancelled inside the summary turn. A summary that fails
    /// on the wire is a warning: the conversation is merely larger than wanted, and ending the run
    /// over that would reintroduce the defect this exists to remove.
    fn compact_run(&mut self, state: &mut RunState, sink: &mut dyn LoopSink) -> Option<LoopStop> {
        let Some(window) = self.config.context_window else {
            compact(&mut state.items, sink);
            return None;
        };
        let before = measure(&state.items);
        // The provider's own count wherever there is one: it includes the instruction and the tool
        // schemas, which are not in `items` at all, so it is the larger figure and the one nearer
        // the wall. The estimate is the fallback for a provider that reports nothing.
        let occupied = estimated_tokens(before).max(state.reported_input.unwrap_or(0));
        if occupied < window.saturating_mul(COMPACTION_TRIGGER_PERCENT) / 100 {
            return None;
        }
        // Expressed as *how many bytes to free* rather than as a size to reach, because what this
        // loop can shrink is `items` and the count that triggered may have measured more than
        // that. Freeing the overshoot is the same arithmetic in both cases, and with no reported
        // count it is exactly "leave the conversation at half the window".
        let free = occupied
            .saturating_sub(window.saturating_mul(COMPACTION_TARGET_PERCENT) / 100)
            .saturating_mul(ESTIMATED_BYTES_PER_TOKEN);
        let target = before.saturating_sub(usize::try_from(free).unwrap_or(usize::MAX));
        let elided = elide(&mut state.items, target, protected_bytes(target));

        let mut summarised = 0_usize;
        let mut summary_turn = false;
        if measure(&state.items) > target {
            match self.summarise(state, target, sink) {
                Summarised::Cancelled => return Some(cancelled()),
                Summarised::Folded(count) => {
                    summarised = count;
                    summary_turn = true;
                }
                Summarised::Failed => summary_turn = true,
                Summarised::Skipped => {}
            }
        }
        if elided.count == 0 && summarised == 0 && !summary_turn {
            return None;
        }

        let after = measure(&state.items);
        // Said out loud, for the same reason the byte rule says it: a model that suddenly cannot
        // see a file it read has a right to a reason, and so does anyone reading the record.
        sink.emit(LoopEvent::Warning {
            code: "conversation-compacted".to_owned(),
            message: format!(
                "the conversation reached {occupied} tokens of the {window} declared, so {count} \
                 old tool result(s) were elided and {summarised} item(s) folded into a summary; \
                 {before} bytes became {after}.",
                count = elided.count,
            ),
        });
        sink.emit(LoopEvent::Compacted {
            elided_results: elided.count,
            elided_bytes: elided.freed,
            summarised_items: summarised,
            bytes_before: before,
            bytes_after: after,
            summary_turn,
        });
        None
    }

    /// Spends one turn folding the earlier part of the conversation into a single item.
    ///
    /// # Why this is a turn and not a truncation
    ///
    /// Elision drops tool-result payloads, which is where the weight usually is. Where it is not —
    /// a long argument in user and assistant text, or the opaque reasoning items carried verbatim
    /// so the model keeps its own chain of thought across a round trip — elision reaches its floor
    /// with the conversation still over the target, and the only alternatives are to drop that
    /// text unread or to compress it. Dropping it silently is what invariant 8 forbids; asking the
    /// model to compress it costs one turn and keeps the facts.
    ///
    /// What survives, always: the task ([`FIRST_KEPT_ITEM`]), the newest [`protected_bytes`] of
    /// conversation, and every [`Item::ToolCall`] beside its result. What replaces the rest is one
    /// [`Item::user`] beginning with [`SUMMARY_MARKER`], so nothing downstream mistakes the
    /// harness's own words for a person's.
    ///
    /// The turn is counted against the run's tokens, budget and bill like any other, because the
    /// provider charged for it like any other. It does **not** advance `turns`: that number bounds
    /// the model's progress on the task, and compaction is overhead the loop chose.
    fn summarise(
        &mut self,
        state: &mut RunState,
        target: usize,
        sink: &mut dyn LoopSink,
    ) -> Summarised {
        let Some(end) = fold_end(&state.items, protected_bytes(target)) else {
            return Summarised::Skipped;
        };
        let folded: Vec<Item> = state.items[FIRST_KEPT_ITEM..end].to_vec();
        if measure(&folded) < SUMMARY_MIN_FOLD_BYTES {
            return Summarised::Skipped;
        }
        if self.cancel.is_cancelled() {
            return Summarised::Cancelled;
        }

        let request = TurnRequest {
            model: self.config.model.clone(),
            instructions: SUMMARY_INSTRUCTION.to_owned(),
            // The fold **rendered**, not replayed: one user item, no tool blocks, no opaque
            // items. Sent as items it was a request neither wire could accept, and every
            // compaction paid for a turn that was going to be a 400 — see
            // [`summary_request_items`].
            items: summary_request_items(&folded),
            // No tools. This turn has nothing to do but read what it was handed, and a tool call
            // from a turn a person cannot see would be an effect nobody asked for.
            tools: Vec::new(),
            max_output_tokens: self.config.budget.max_output_tokens_per_turn,
            sampling: self.config.sampling.clone(),
            // Publishing nothing and holding it to something would be a request no provider can
            // satisfy; `validate` refuses exactly that.
            tool_choice: harness_wire::ToolChoice::Auto,
        };
        let outcome = {
            let mut quiet = Quiet(sink);
            match self.model.turn(&request, &mut quiet) {
                Ok(outcome) => outcome,
                Err(error) if error.code == WireErrorCode::Cancelled => {
                    return Summarised::Cancelled;
                }
                Err(error) => {
                    sink.emit(LoopEvent::Warning {
                        code: "summary-failed".to_owned(),
                        message: format!(
                            "the summary turn failed ({error}), so the conversation keeps its \
                             elided form and the run goes on"
                        ),
                    });
                    return Summarised::Failed;
                }
            }
        };
        state.absorb_usage(outcome.usage, self.config.prices.as_ref(), sink);
        if self.cancel.is_cancelled() {
            return Summarised::Cancelled;
        }

        let summary: String = outcome
            .items
            .iter()
            .filter_map(|item| match item {
                Item::AssistantText { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        if summary.trim().is_empty() {
            sink.emit(LoopEvent::Warning {
                code: "summary-failed".to_owned(),
                message: "the summary turn answered with no text, so nothing could replace the \
                          conversation it was asked to fold"
                    .to_owned(),
            });
            return Summarised::Failed;
        }

        let tail = state.items.split_off(end);
        state.items.truncate(FIRST_KEPT_ITEM);
        state
            .items
            .push(Item::user(format!("{SUMMARY_MARKER}\n{summary}")));
        state.items.extend(tail);
        Summarised::Folded(end - FIRST_KEPT_ITEM)
    }

    /// What a run that would stop with `stop` actually does: ends with it, or turns again.
    ///
    /// Only a `Completed` stop is open to question — a bound that bound, a cancellation, a
    /// provider that gave up are final. The answer nudge speaks first, because a run asked for a
    /// structured answer has not answered yet; the stop hook speaks last, on whatever the run's
    /// answer turned out to be. [`None`] means the conversation has one more user item and the
    /// loop turns again.
    fn ending(
        &mut self,
        stop: LoopStop,
        state: &mut RunState,
        sink: &mut dyn LoopSink,
    ) -> Option<LoopStop> {
        if !stop.is_completed() {
            return Some(stop);
        }
        let stop = self.answer_or_nudge(stop, state, sink)?;
        if !stop.is_completed() {
            return Some(stop);
        }
        self.stop_hook(stop, state, sink)
    }

    /// The answer nudge (design 0002 § 1): a run asked for a structured answer that ended in
    /// prose is told once to call the answer tool, and after that stops
    /// [`LoopStop::Unstructured`]. Returns `None` to turn again.
    fn answer_or_nudge(
        &mut self,
        stop: LoopStop,
        state: &mut RunState,
        sink: &mut dyn LoopSink,
    ) -> Option<LoopStop> {
        let Some(schema) = self.config.output_schema.as_ref() else {
            return Some(stop);
        };
        if state.structured.is_some() {
            return Some(stop);
        }
        if state.nudged >= MAX_ANSWER_NUDGES {
            return Some(LoopStop::Unstructured {
                asked_again: state.nudged,
            });
        }
        state.nudged += 1;
        sink.emit(LoopEvent::Warning {
            code: "answer-nudged".to_owned(),
            message: format!(
                "the model ended in prose where `{}` was required; told once more to call it \
                 ({} of {MAX_ANSWER_NUDGES})",
                schema.name, state.nudged
            ),
        });
        state.items.push(Item::user(format!(
            "Finish by calling `{}` with the result; nothing else is read.",
            schema.name
        )));
        // And the turn this opens is *held* to that tool at the provider, not only asked for it:
        // see [`AgentLoop::held_to_the_answer`], which reads `state.nudged`.
        None
    }

    /// Runs the turn's calls in order, stopping the moment the caller cancels or time runs out.
    ///
    /// # A turn that answers is the answer and nothing else
    ///
    /// The answer tool's published description promises it: *call it alone, as the last thing: any
    /// other call in the same turn is refused*. So where the turn carries an `answer` call, every
    /// other call in that turn is refused by [`refused_beside_the_answer`] and **none of them runs**
    /// — the
    /// ones before it as much as the ones after. Refusing only the tail kept the promise for half
    /// the turn: a `[file_write, answer]` ran the write, which is exactly the effect-after-the-end
    /// the sentence says did not happen. They are still answered in the order the model asked, so
    /// the conversation reads back in it.
    fn run_calls(
        &mut self,
        calls: &[ToolCall],
        state: &mut RunState,
        deadline: Option<Instant>,
        sink: &mut dyn LoopSink,
    ) -> Option<LoopStop> {
        // Found before anything runs, because *running the write and refusing it afterwards* is
        // the failure this answers. The sentence its siblings are refused with is built here, once
        // per turn rather than once per call: it names the run's answer tool, which is fixed for
        // the run.
        let answering = self.config.output_schema.as_ref().and_then(|schema| {
            let at = calls.iter().position(|call| call.name == schema.name)?;
            Some((at, refused_beside_the_answer(&schema.name)))
        });
        let mut next = 0_usize;
        while next < calls.len() {
            if self.cancel.is_cancelled() {
                // Every call the model made needs an answer in the conversation, even one that
                // never ran: a `function_call` replayed without its output is a provider error on
                // the next turn, so a cancelled run could not be resumed at all.
                refuse_rest(
                    &calls[next..],
                    "the run was cancelled before this call ran",
                    state,
                    sink,
                );
                return Some(cancelled());
            }
            // Between calls as well as between turns: one call can block for minutes, so a
            // deadline checked only at the turn boundary overshoots by a whole call — and a turn
            // asking for six of them overshoots by six. A call already running still runs to its
            // own timeout; nothing here reaches into it. Every skipped call still gets an
            // outcome, for the reason the cancellation branch above gives.
            if let Some(stop) = self.deadline_passed(deadline) {
                refuse_rest(
                    &calls[next..],
                    "the run's deadline passed before this call ran",
                    state,
                    sink,
                );
                return Some(stop);
            }

            let call = &calls[next];
            // Everything a turn that answered asked for beside the answer, in the order it asked.
            if let Some((at, refusal)) = answering.as_ref()
                && *at != next
            {
                refuse_rest(std::slice::from_ref(call), refusal, state, sink);
                next += 1;
                continue;
            }

            // The loop's own tools resolve first, before batching, before the published set is
            // consulted and before anything reaches the port. Neither is a port call: `answer`
            // ends the run and `delegate` runs a whole second loop over these same ports.
            if let Some(owned) = self.owned(call) {
                // One call, or the run of neighbouring `delegate` calls this one starts: those may
                // run side by side, and deciding it here is what lets `run_owned` ask the
                // `before-call` hook about every one of them before any of them starts.
                let span = self.delegate_span(&owned, &calls[next..]);
                let group = &calls[next..next + span];
                for call in group {
                    sink.emit(LoopEvent::ToolRequested(call.clone()));
                }
                if let Some(stop) = self.run_owned(&owned, group, state, deadline, sink) {
                    // Only the answer ends a turn from in here, and nothing after it is read. The
                    // same sentence the calls before it got: they were refused for the same
                    // reason, and `answering` is `Some` here because only an answer stops a turn.
                    // An answer is never grouped, so `span` is one and the tail starts after it.
                    if let Some((_, refusal)) = answering.as_ref() {
                        refuse_rest(&calls[next + span..], refusal, state, sink);
                    }
                    return Some(stop);
                }
                next += span;
                continue;
            }

            // A maximal run of neighbours that are all pure, so the port may run them side by
            // side. Consecutive rather than gathered from the whole turn: a write between two
            // reads is a barrier, because the second read may be reading what the write wrote.
            let group = calls[next..]
                .iter()
                .take_while(|neighbour| self.batchable(neighbour))
                .count();
            if group > 1 {
                self.run_batch(&calls[next..next + group], state, deadline, sink);
                next += group;
                continue;
            }

            sink.emit(LoopEvent::ToolRequested(call.clone()));
            let result = self.invoke(call, deadline, sink);
            complete(call, result, state, sink);
            next += 1;
        }
        None
    }

    /// Runs the loop's own tools, under the bounds and the hooks a port call meets.
    ///
    /// `calls` is one call, except for a run of neighbouring `delegate` calls, which arrive
    /// together because they may run side by side ([`AgentLoop::delegate_span`]).
    ///
    /// # The same gate, minus the one stage that has nothing to decide
    ///
    /// The argument bound first, for the reason [`recordable`] gives from the other side: it
    /// replaces oversized arguments with `{"omitted": …}` in the conversation, so an `answer`
    /// accepted past the bound would put a value in [`LoopOutcome::structured`] that the record of
    /// the run does not carry, and a `delegate` past it would hand a child a task nobody can read
    /// back. Then the operator's `before-call` hook, whose block is the same failed outcome
    /// [`AgentLoop::invoke`] produces, naming this tool. Then the tool. Then `after-call`, whose
    /// note lands beside the result exactly as it does for a port call. An owned call went through
    /// neither hook before this: a `before-call` declaration with no `tools` filter — which design
    /// 0002 § 3 says means *every* call — was never consulted about the two calls an operator is
    /// most likely to want a word on, and left nothing in the record to say so.
    ///
    /// The approver is the stage that is missing, and it is missing because it would decide
    /// nothing: both specs declare no effect at `Risk::Low`, so `needs_approval` is false at every
    /// ceiling a run can have. What a delegate then does is asked about call by call, inside the
    /// child, on each entry's own envelope.
    ///
    /// # Every stage runs over the whole group before the next one starts
    ///
    /// The three stages are three passes and not one loop, because the middle one is where a group
    /// stops being sequential. A `before-call` hook is asked about every child before any child
    /// runs — which is the only order in which a hook can still prevent one — and the results go
    /// into the conversation in the order the model asked for them, whatever order they finished
    /// in. A call the gate refused takes its place in that order with the others and never reaches
    /// `after-call`, exactly as a single refused call does: there is no outcome a tool produced.
    ///
    /// [`Some`] is the run ending here, which only the answer can ask for.
    fn run_owned(
        &mut self,
        owned: &Owned,
        calls: &[ToolCall],
        state: &mut RunState,
        deadline: Option<Instant>,
        sink: &mut dyn LoopSink,
    ) -> Option<LoopStop> {
        let invoked = owned.spec();
        // Stage one: what each call has to get past before anything runs. `Err` is the refusal it
        // is answered with, held rather than completed so that the conversation still reads in the
        // order the model asked.
        let gated: Vec<Result<(), ToolOutcome>> = calls
            .iter()
            .map(|call| self.gate_owned(call, &invoked, sink))
            .collect();
        let admitted: Vec<&ToolCall> = calls
            .iter()
            .zip(&gated)
            .filter_map(|(call, gate)| gate.is_ok().then_some(call))
            .collect();

        // Stage two: the tools themselves, over the calls the gate let through.
        let mut ran: VecDeque<(ToolOutcome, Option<LoopStop>)> = match owned {
            Owned::Answer(_) => admitted
                .iter()
                .map(|call| accept_answer(call, state))
                .collect(),
            Owned::Skill(skills) => admitted
                .iter()
                .map(|call| (load_skill(skills, call), None))
                .collect(),
            Owned::Delegate(delegation) => self
                .run_delegates(delegation, &admitted, state, deadline, sink)
                .into_iter()
                .map(|result| (result, None))
                .collect(),
        };

        // Stage three: `after-call`, and the conversation, in the order the model asked.
        let mut stopped = None;
        for (call, gate) in calls.iter().zip(gated) {
            let (result, stop) = if let Err(refusal) = gate {
                (refusal, None)
            } else {
                let (result, stop) = ran
                    .pop_front()
                    .expect("one outcome per call the gate admitted");
                (self.after_call_hook(call, &invoked, result, sink), stop)
            };
            complete(call, result, state, sink);
            if stop.is_some() {
                // After the result is in the conversation, so a reader of the record sees the
                // answer announced by a run that had already recorded it.
                sink.emit(LoopEvent::Answered {
                    call_id: call.call_id.clone(),
                    value: call.arguments.clone(),
                });
                stopped = stop;
            }
        }
        stopped
    }

    /// What one owned call must get past before the tool behind it runs: the argument bound, then
    /// the operator's `before-call` hook.
    ///
    /// `Err` is the refusal the model reads. Neither refusal reaches `after-call`, because no tool
    /// ran and that point is about what a tool did.
    fn gate_owned(
        &mut self,
        call: &ToolCall,
        invoked: &ToolSpec,
        sink: &mut dyn LoopSink,
    ) -> Result<(), ToolOutcome> {
        if exceeds(&call.arguments, MAX_TOOL_ARGUMENT_BYTES) {
            return Err(ToolOutcome::failed(format!(
                "the arguments for `{}` are over the {MAX_TOOL_ARGUMENT_BYTES} byte bound",
                call.name
            )));
        }
        match self.before_call_hook(call, invoked, sink) {
            Some(refusal) => Err(refusal),
            None => Ok(()),
        }
    }

    /// How many of the turn's remaining calls this one owned resolution covers.
    ///
    /// One, for everything except a `delegate` on a run that may run delegates side by side. There
    /// it is the **maximal run of neighbouring** `delegate` calls, capped at
    /// [`Delegation::max_parallel`].
    ///
    /// Neighbouring rather than gathered from the whole turn, for the reason a batch of reads is
    /// neighbouring: a call between two delegates is a barrier, because the second child may be
    /// there to look at what that call did. A turn asking for more than the cap runs them in
    /// groups of the cap, in order.
    ///
    /// `answer` is never grouped — it ends the turn, and what follows it is refused rather than
    /// run — and neither is `skill`, which reads a document this process already holds.
    fn delegate_span(&self, owned: &Owned, rest: &[ToolCall]) -> usize {
        let Owned::Delegate(delegation) = owned else {
            return 1;
        };
        if !delegation.runs_side_by_side() {
            return 1;
        }
        rest.iter()
            .take_while(|call| matches!(self.owned(call), Some(Owned::Delegate(_))))
            .take(usize::try_from(delegation.max_parallel).unwrap_or(usize::MAX))
            .count()
    }

    /// Whether this call is the run's own answer tool, without cloning a schema to find out.
    fn answers(&self, call: &ToolCall) -> bool {
        self.config
            .output_schema
            .as_ref()
            .is_some_and(|schema| schema.name == call.name)
    }

    /// Which of the loop's own tools this call names, if any.
    ///
    /// Read from the run's own configuration and never from the port, and unambiguous because
    /// [`AgentLoop::check_owned_names`] refused the run if the port wanted either name. A
    /// delegation at depth 0 owns nothing: it published no `delegate`, so a call naming one is an
    /// unpublished tool and is refused as one.
    fn owned(&self, call: &ToolCall) -> Option<Owned> {
        if let Some(schema) = self.config.output_schema.as_ref()
            && self.answers(call)
        {
            return Some(Owned::Answer(schema.clone()));
        }
        if let Some(delegation) = self.config.delegation.as_ref()
            && delegation.depth > 0
            && delegation.name == call.name
        {
            return Some(Owned::Delegate(delegation.clone()));
        }
        if let Some(skills) = self.config.skills.as_ref()
            && !skills.is_empty()
            && skills.name == call.name
        {
            return Some(Owned::Skill(skills.clone()));
        }
        None
    }

    /// Which named agent a delegate call asked for, and the toolset it leaves the child.
    ///
    /// Split out of [`Self::prepare_child`] because that function is the whole of building a child
    /// and resolving *which* child is a separate question with its own failure modes.
    ///
    /// # Errors
    ///
    /// A [`ToolOutcome`] the caller returns as-is. An unknown name is **not** silently the generic
    /// delegate: the model was given the list in its own instructions, and running something other
    /// than what it asked for is worse than telling it the name was wrong.
    #[allow(clippy::type_complexity)]
    fn agent_for(
        &self,
        call: &ToolCall,
    ) -> Result<(Option<Agent>, Vec<String>, Vec<Withheld>), ToolOutcome> {
        let named = call
            .arguments
            .get("agent")
            .and_then(serde_json::Value::as_str);
        let agent = match (named, self.config.agents.as_ref()) {
            (None, _) => None,
            (Some(name), Some(agents)) => match agents.get(name) {
                Some(agent) => Some(agent.clone()),
                None => {
                    return Err(ToolOutcome::failed(format!(
                        "`{name}` is not an agent this run has. It has: {}.",
                        agents.names().join(", ")
                    )));
                }
            },
            (Some(name), None) => {
                return Err(ToolOutcome::failed(format!(
                    "`{name}` is not an agent this run has; this run has none."
                )));
            }
        };
        // **The parent's own admitted set, never the port's whole list.** A child of a narrowed
        // run must not be able to climb back out by naming an agent.
        //
        // Its *reach* and not what it publishes, because that is the vocabulary an agent document
        // is written in: a `tools: [Read, Grep]` intersected against `tool_search`,
        // `tool_describe`, `tool_invoke` meets nothing, and the child would be handed an empty
        // grant with two `withheld` entries claiming this run's catalogue has no reader. Under a
        // flat surface the two lists are the same list and this is the same intersection it was.
        let parent_tools: Vec<String> = self
            .tools
            .reachable()
            .into_iter()
            .filter(|name| {
                self.config
                    .admits
                    .as_ref()
                    .is_none_or(|admitted| admitted.contains(name))
            })
            .map(|name| name.as_str().to_owned())
            .collect();
        let (admitted, refused) = agent.as_ref().map_or_else(
            || (parent_tools.clone(), Vec::new()),
            |agent| agent.admitted(&parent_tools),
        );
        Ok((agent, admitted, refused))
    }

    /// Everything the parent has to work out before a child can be started.
    ///
    /// Resolved **before** the budget is carved and before anything is emitted, because how much
    /// budget a group of children gets each depends on how many of them turn out to be startable
    /// at all: a call that names no task, or an agent this run does not have, takes no share of
    /// the run's remainder and no `DelegateStarted` in the record.
    ///
    /// The budget is deliberately not set here. It is the one thing that cannot be decided per
    /// child in isolation — see [`AgentLoop::carve`] — so the caller sets it as it builds the
    /// loop, which is also the only place it could be got wrong in one child and right in another.
    ///
    /// # Errors
    ///
    /// The failed [`ToolOutcome`] the model reads, naming what about the call could not be used.
    fn prepare_child(&self, call: &ToolCall) -> Result<Child, ToolOutcome> {
        let task = call
            .arguments
            .get("task")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if task.trim().is_empty() {
            return Err(ToolOutcome::failed(format!(
                "`{}` needs a `task`: one non-empty string saying everything the delegate needs, \
                 because it cannot see this conversation",
                call.name
            )));
        }
        let (agent, admitted, refused) = self.agent_for(call)?;
        let config = LoopConfig {
            // The parent's standing instruction whole, so the delegate knows where it is and what
            // its tools are for, and the preamble after it — which is everything that is true only
            // of a child. A named agent's own body goes after both: it is the most specific thing
            // this child is, and it must not be able to argue away the preamble in front of it.
            instructions: match &agent {
                None => format!("{}\n\n{DELEGATE_PREAMBLE}", self.config.instructions),
                Some(agent) => format!(
                    "{}\n\n{DELEGATE_PREAMBLE}\n\n{}",
                    self.config.instructions, agent.instructions
                ),
            },
            // What the agent asked for and did not get, in the child's own record rather than
            // nowhere: an agent whose author granted it a tool this machine never admitted is a
            // fact about the run, and an absence would read as one that never wanted it.
            withheld: refused,
            // **Unconditionally, and not only when an agent was named.** `agent_for` answers the
            // parent's own admitted set for the agentless arm, and handing the child `None`
            // instead threw it away: a child narrowed to `file_read` delegated again, named no
            // agent, and its grandchild got the whole port. `delegate.rs` says delegation widens
            // nothing and `agent_for` says a child must not climb back out by naming an agent —
            // it climbed out by not naming one.
            admits: Some(
                admitted
                    .iter()
                    .filter_map(|name| harness_wire::ToolName::new(name).ok())
                    .collect(),
            ),
            // A delegate publishes no agents of its own, for the reason it publishes no delegate:
            // one level, so a tree nobody can read afterwards cannot be built by accident.
            agents: None,
            // Its report is its text; a schema for a child is milestone M2 of design 0002.
            output_schema: None,
            delegation: self
                .config
                .delegation
                .as_ref()
                .and_then(Delegation::for_child),
            ..self.config.clone()
        };
        Ok(Child {
            task: task.to_owned(),
            config,
        })
    }

    /// Runs a turn's `delegate` calls and answers one outcome per call, in order.
    ///
    /// # Side by side is an optimisation and never a difference in what a run can do
    ///
    /// The group runs concurrently when everything lines up: more than one call, a run configured
    /// for it ([`Delegation::max_parallel`]), ports that will hand out a second handle on
    /// themselves ([`ModelPort::fork`], [`ToolPort::fork`]), and a remainder of the run's budget
    /// that divides. When any of those does not hold, the same calls run one after another and
    /// **nothing else about the run changes** — the same children, the same tools, the same gate,
    /// the same results in the same order. That is what makes this safe to have on by default: the
    /// worst a port that cannot fork costs is the wall-clock a run has always paid.
    ///
    /// Running in order is not merely the fallback, it is the more *accurate* accounting: each
    /// child is carved on what the one before it actually spent, where a group has to divide the
    /// remainder up front. That is the cost of concurrency and it is paid in budget precision, not
    /// in reach.
    fn run_delegates(
        &mut self,
        delegation: &Delegation,
        calls: &[&ToolCall],
        state: &mut RunState,
        deadline: Option<Instant>,
        sink: &mut dyn LoopSink,
    ) -> Vec<ToolOutcome> {
        if calls.len() > 1
            && delegation.runs_side_by_side()
            && let Some(results) =
                self.delegates_side_by_side(delegation, calls, state, deadline, sink)
        {
            return results;
        }
        calls
            .iter()
            .map(|call| self.delegate(delegation, call, state, deadline, sink))
            .collect()
    }

    /// Runs a second loop to completion inside this tool call, and returns what it reported.
    ///
    /// The child starts from an empty conversation: it sees the parent's standing instruction and
    /// [`DELEGATE_PREAMBLE`], and the `task` string, and nothing of the conversation the call came
    /// from. That is the point — a sub-tree that reads forty files to answer one question costs
    /// the parent one tool result rather than forty reads of context.
    ///
    /// # What is shared, and what the parent gets back
    ///
    /// The model port (the parent is blocked in here, so it is idle), the tool port (delegation
    /// widens nothing: the child can do exactly what the parent's catalogue admits), the approver
    /// (a person is asked about the child's write exactly as about the parent's), the hooks — for
    /// `before-call` and `after-call`, an operator's hook on `run` firing in a delegate too or it
    /// was not a hook on `run`, while `stop` belongs to the run and is not consulted at a child's
    /// end ([`AgentLoop::stop_hook`]) — and the cancellation token (Ctrl-C reaches the innermost
    /// blocked read). What comes back is the child's usage and cost, added to the parent's totals
    /// **however the child ended**, and one result carrying its stop, its turn count and its final
    /// text.
    ///
    /// Every event the child emits reaches the sink inside [`LoopEvent::Delegated`] — see
    /// [`Wrapped`] — so the `Usage` and `Cost` events for the turns absorbed here have already
    /// been seen, and none is emitted a second time.
    ///
    /// # A cancelled child ends the parent, and nothing here does that
    ///
    /// The token is shared, so a child that stopped [`LoopStop::Cancelled`] leaves it cancelled
    /// and the parent's own check — the one at the top of [`AgentLoop::run_calls`], before the
    /// next call of this same turn — refuses the rest and ends the run. Special-casing it here
    /// would be a second implementation of that.
    fn delegate(
        &mut self,
        delegation: &Delegation,
        call: &ToolCall,
        state: &mut RunState,
        deadline: Option<Instant>,
        sink: &mut dyn LoopSink,
    ) -> ToolOutcome {
        let child = match self.prepare_child(call) {
            Ok(child) => child,
            Err(refusal) => return refusal,
        };
        // Before the child exists, so a run with nothing left never spends a turn learning it.
        let budget = match self.carve(delegation, state, deadline, 1) {
            Ok(budget) => budget,
            Err(ceiling) => {
                return ToolOutcome::failed(format!(
                    "the run's budget has no room for a delegate: {ceiling}"
                ));
            }
        };
        sink.emit(LoopEvent::DelegateStarted {
            call_id: call.call_id.clone(),
            task: child.task.clone(),
        });

        let mut wrapped = Wrapped {
            call_id: call.call_id.clone(),
            sink,
        };
        // The child's own state, held here rather than inside the run, because the parent has to
        // read what it spent however it ended — see the absorption below.
        let mut child_state = RunState::resuming(Vec::new(), child.task);
        let ran = {
            let mut child_loop = AgentLoop::new(
                &mut *self.model,
                &mut *self.tools,
                &mut *self.approvals,
                LoopConfig {
                    budget,
                    ..child.config
                },
            )
            .with_cancel(self.cancel.clone());
            if let Some(hooks) = self.hooks.as_deref_mut() {
                child_loop = child_loop.with_hooks(hooks);
            }
            // The one thing a child is that a run started by a caller never is.
            child_loop.nested = true;
            child_loop.run_over(&mut child_state, &mut wrapped)
        };
        // Before anything branches on how the child ended, because *how it ended* is exactly what
        // used to decide whether the parent paid for it. A delegate spends the run's budget, never
        // one of its own: the parent's ceilings bind on the sum and `stop_after_tokens` fires
        // before the parent's next turn.
        state.absorb_child(&mut child_state);

        let (stop, result) = child_result(call, ChildEnd::of(ran), &child_state);
        sink.emit(LoopEvent::DelegateFinished {
            call_id: call.call_id.clone(),
            stop,
            // The turns the child actually started, on the failed path as on the answered one —
            // the same count `turns` carries everywhere else in this loop, which is why a child
            // that broke on its fourth reports four and not nothing.
            turns: child_state.turns,
        });
        result
    }

    /// Runs a group of delegates at the same time, one thread each, and answers in call order.
    ///
    /// [`None`] is *this group cannot be run this way* — and never a refusal. The caller runs the
    /// same calls in order instead and the run reaches the same place; see
    /// [`AgentLoop::run_delegates`]. There are three reasons it can happen, and each is a fact
    /// about the run rather than about the model's request: fewer than two children turned out to
    /// be startable, a port would not fork, or the remainder of the budget would not divide.
    ///
    /// # What each child gets, and what stays on this thread
    ///
    /// Forked, so the children genuinely run at once: the model port and the tool port. A fork of
    /// each is the same endpoint and the same catalogue — delegation widens nothing here either.
    ///
    /// Not forked, because there is one of them by nature: the approver (one person, asked one
    /// question at a time), the operator's hooks (one file of programs, run one at a time) and the
    /// run's event sink (one ordered record). A child reaches all three by asking this thread,
    /// which sits in [`parallel::answer_children`] for exactly as long as any child is running.
    ///
    /// # What a reader of the record sees
    ///
    /// Two `DelegateStarted` before either `DelegateFinished`, and the two children's `Delegated`
    /// events interleaved. That interleaving *is* the evidence the group ran side by side: it
    /// cannot occur in a run that delegated in order, so no new event is needed to say so.
    ///
    /// # Cancel and the deadline are not checked between the children
    ///
    /// They are checked before the group and after it, exactly as for a batch of tool calls, and
    /// for the same reason: there is no "between". What is different, and better, is that a cancel
    /// **does** reach inside — the token is the run's own and each child checks it between its own
    /// turns and its own calls, so Ctrl-C stops four children rather than being noticed after the
    /// last one finishes.
    fn delegates_side_by_side(
        &mut self,
        delegation: &Delegation,
        calls: &[&ToolCall],
        state: &mut RunState,
        deadline: Option<Instant>,
        sink: &mut dyn LoopSink,
    ) -> Option<Vec<ToolOutcome>> {
        // Every child resolved before any budget is divided, because a call that cannot start
        // takes no share of the run's remainder.
        let prepared: Vec<Result<Child, ToolOutcome>> =
            calls.iter().map(|call| self.prepare_child(call)).collect();
        let share = prepared.iter().filter(|child| child.is_ok()).count();
        if share < 2 {
            // One child left, or none. Nothing to run side by side, and the ordinary path carves
            // the remainder whole rather than dividing it by a group that is not there.
            return None;
        }
        let budget = self.carve(delegation, state, deadline, share).ok()?;

        let mut results: Vec<Option<ToolOutcome>> = calls.iter().map(|_| None).collect();
        let mut running: Vec<(usize, Child)> = Vec::with_capacity(share);
        for (at, (_, child)) in calls.iter().zip(prepared).enumerate() {
            match child {
                Err(refusal) => results[at] = Some(refusal),
                Ok(child) => running.push((at, child)),
            }
        }

        // Reborrowed field by field rather than taken through `self`, because the forked ports
        // borrow two of them for as long as the children run while the approver, the hooks and the
        // sink are in use on this thread the whole time. Four disjoint borrows of one struct,
        // which is exactly what they are.
        let hooked = self.hooks.is_some();
        let cancel = self.cancel.clone();
        // **Before a single event is emitted**, because every way out of this function above and
        // including here is one where the caller runs the same children in order — and it will
        // emit their `DelegateStarted` itself. A group that announced two children and then handed
        // them back wrote two starts for each of them into the record, which is what a shipped
        // `ToolPort` wrapper that forgot to delegate `fork` actually produced.
        let mut forked: Vec<(
            Box<dyn ModelPort + Send + '_>,
            Box<dyn ToolPort + Send + '_>,
        )> = Vec::with_capacity(running.len());
        for _ in &running {
            forked.push(((*self.model).fork()?, (*self.tools).fork()?));
        }
        // Committed: from here the group runs, so this is where the children are announced.
        for (at, child) in &running {
            sink.emit(LoopEvent::DelegateStarted {
                call_id: calls[*at].call_id.clone(),
                task: child.task.clone(),
            });
        }
        let approvals: &mut dyn ApprovalPort = &mut *self.approvals;
        // Which call of the turn each child answers, kept beside the group because the group is
        // indexed by *what is running* and the conversation by *what the model asked*.
        let answering: Vec<usize> = running.iter().map(|(at, _)| *at).collect();
        let call_ids: Vec<CallId> = answering
            .iter()
            .map(|at| calls[*at].call_id.clone())
            .collect();

        let ended = std::thread::scope(|scope| {
            let (tx, rx) = std::sync::mpsc::channel::<parallel::FromChild>();
            let handles: Vec<_> = running
                .drain(..)
                .zip(forked)
                .enumerate()
                .map(|(at, ((_, child), (mut model, mut tools)))| {
                    let tx = tx.clone();
                    let cancel = cancel.clone();
                    let budget = budget.clone();
                    scope.spawn(move || {
                        let mut sink = parallel::ChildSink { at, tx: tx.clone() };
                        let mut approvals = parallel::ChildApprovals { tx: tx.clone() };
                        let mut hooks = hooked.then(|| parallel::ChildHooks { tx: tx.clone() });
                        let mut state = RunState::resuming(Vec::new(), child.task);
                        // A child that panicked must not take the run with it: the parent has
                        // three more children in flight and a conversation that needs an outcome
                        // for every call in it. The same containment a batch of tool calls has.
                        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let mut child_loop = AgentLoop::new(
                                &mut *model,
                                &mut *tools,
                                &mut approvals,
                                LoopConfig {
                                    budget,
                                    ..child.config
                                },
                            )
                            .with_cancel(cancel);
                            if let Some(hooks) = hooks.as_mut() {
                                child_loop = child_loop.with_hooks(hooks);
                            }
                            child_loop.nested = true;
                            child_loop.run_over(&mut state, &mut sink)
                        }));
                        (state, ran)
                    })
                })
                .collect();
            // The parent's own handle on the channel, dropped so that the receive loop below ends
            // when the last child's does and not one message later.
            drop(tx);
            parallel::answer_children(
                &rx,
                &call_ids,
                approvals,
                self.hooks.as_deref_mut(),
                &mut *sink,
            );
            handles
                .into_iter()
                .map(std::thread::ScopedJoinHandle::join)
                .collect::<Vec<_>>()
        });

        absorb_children(calls, &answering, ended, &mut results, state, sink);

        Some(
            results
                .into_iter()
                .map(|result| result.expect("every call in the group is answered exactly once"))
                .collect(),
        )
    }

    /// The remainder of this run's budget, as the budget of one of `share` delegates.
    ///
    /// Every ceiling the parent set — turns and the clock excepted — is carved to
    /// `(limit − spent so far) / share`, because a delegate spends the run's budget rather than
    /// one of its own and the parent absorbs what it spent when the call returns.
    ///
    /// Turns are the first exception: the child gets [`Delegation::max_turns`] of its own, so a
    /// child that loops does not spend the parent's remaining fifty turns finding out. The
    /// per-turn output offer is passed through rather than carved, because it bounds one turn and
    /// is not a total to divide.
    ///
    /// # The clock is the second exception, and dividing it would be wrong
    ///
    /// Tokens add up: four children that each spend a thousand have spent four thousand of the
    /// run's. Wall clock does not — four children that each take a minute, running at the same
    /// moment, take a minute. So each is told the whole of what is left, which is the same figure
    /// [`AgentLoop::run_batch`] hands a group of tool calls and for the same reason. The deadline
    /// is read again the moment the group returns, which is where the overshoot is bounded.
    ///
    /// # Errors
    ///
    /// The name of the first ceiling with nothing left, or with nothing left once divided.
    /// Starting a child on a remainder of zero would spend a turn to be told what is knowable here
    /// — and [`Budget::validate`] refuses a zero bound by name anyway, so it would come back as an
    /// error rather than as the refusal the model can act on. For a group, the caller reads it as
    /// *do not run these side by side* and runs them in order, where each child is carved whole
    /// against what the one before it actually spent.
    fn carve(
        &self,
        delegation: &Delegation,
        state: &RunState,
        deadline: Option<Instant>,
        share: usize,
    ) -> Result<Budget, &'static str> {
        let share = u64::try_from(share).unwrap_or(u64::MAX).max(1);
        // Already a remainder rather than a ceiling, so nothing is taken off it. `deadline` is
        // `Some` exactly when `max_duration_ms` is, both being built from the same field.
        let left_ms = deadline.map(|deadline| {
            u64::try_from(
                deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis(),
            )
            .unwrap_or(u64::MAX)
        });
        Ok(Budget {
            max_turns: Some(delegation.max_turns),
            max_input_tokens: divided(
                remainder(
                    self.config.budget.max_input_tokens,
                    state.input_total,
                    "max_input_tokens",
                )?,
                share,
                "max_input_tokens",
            )?,
            max_output_tokens: divided(
                remainder(
                    self.config.budget.max_output_tokens,
                    state.output_total,
                    "max_output_tokens",
                )?,
                share,
                "max_output_tokens",
            )?,
            max_output_tokens_per_turn: self.config.budget.max_output_tokens_per_turn,
            max_duration_ms: remainder(left_ms, 0, "max_duration_ms")?,
            max_cost_microunits: divided(
                remainder(
                    self.config.budget.max_cost_microunits,
                    state.cost_total.unwrap_or(0),
                    "max_cost_microunits",
                )?,
                share,
                "max_cost_microunits",
            )?,
        })
    }

    /// Whether this call may run beside its neighbours.
    ///
    /// # What makes a call safe to run side by side
    ///
    /// It has to be one the loop would have run anyway — published, inside
    /// [`MAX_TOOL_ARGUMENT_BYTES`] — and one whose **invoked** envelope neither mutates nor asks
    /// anybody at this run's ceiling. Mutation is the barrier: two reads of the same file in
    /// either order read the same bytes, and a write between them does not. Approval is the other:
    /// a batch cannot ask a person about its third call halfway through, and a gate that ran the
    /// call first and asked afterwards would not be a gate.
    ///
    /// A call routed to an entry it did not name (see [`AgentLoop::invoke`]) is deliberately not
    /// batchable: the routing is warned about once per call in `invoke`, and keeping it on that
    /// one path is worth more than the latency of the 12 % of calls that take it.
    ///
    /// # A run with hooks attached batches nothing
    ///
    /// [`AgentLoop::invoke`] is the one path a hook fires on, so a run whose operator attached
    /// hooks sends every call down it: the hook then fires exactly once per call, and never twice
    /// on a group the port answered a different number of outcomes for. The alternative — firing
    /// per group, or per call inside a group the loop cannot see into — would make *how many times
    /// my guard ran* depend on how the model happened to order its reads.
    ///
    /// What it costs is the round trips batching saves, and only a run that asked for hooks pays
    /// it: hooks are opt-in per run (design 0002 § 3).
    ///
    /// Neither is a call naming one of the loop's own tools. `answer` ends the run, so what
    /// follows it is refused rather than run; `delegate` takes the model port for a whole second
    /// run, and a port asked to run two of those side by side would be running two loops through
    /// one model client. [`AgentLoop::run_calls`] resolves those before this is ever reached, and
    /// the check here is what stops one being swept into the group of a *neighbour* that started
    /// it.
    fn batchable(&self, call: &ToolCall) -> bool {
        if self.hooks.is_some() {
            return false;
        }
        if self.owned(call).is_some() {
            return false;
        }
        let Some(published) = self.published(&call.name) else {
            return false;
        };
        // **A batched call never goes down `invoke`, so the narrowing has to be asked here too.**
        // Under a flat surface `published` above already answered it — `port_specs` had taken an
        // unadmitted tool out of the list. Under a verb surface it cannot: the published name is
        // `tool_invoke`, which stays published for every narrowing however tight, and the entry is
        // an argument. Without this, an agent granted `[Grep]` read any file it liked by asking for
        // the read beside a search — two neighbouring pure calls are a group, and a group goes
        // straight to the port. Returning false rather than refusing here keeps one refusal path:
        // the call leaves the group, reaches `invoke`, and is refused there in the usual words.
        if self.unadmitted(call).is_some() {
            return false;
        }
        if exceeds(&call.arguments, MAX_TOOL_ARGUMENT_BYTES) {
            return false;
        }
        let invoked = self
            .tools
            .invoked(call)
            .unwrap_or_else(|| published.clone());
        !invoked.envelope.mutates() && !self.asks(Some(&published), &invoked)
    }

    /// Hands a group of pure calls to the port at once, and checks every answer it gets back.
    ///
    /// A turn asking for six reads paid six round trips of tool latency for no reason: nothing
    /// about a read requires the one before it to have finished. The port decides whether it
    /// actually runs them side by side — the default [`ToolPort::call_batch`] runs them in order —
    /// and the loop cannot tell except by the clock.
    ///
    /// Every outcome is checked against [`MAX_TOOL_RESULT_BYTES`] exactly as a single call is, so
    /// batching cannot become the way an oversized result gets in. A port that answers a different
    /// number of outcomes than it was given calls is one whose answers cannot be matched to
    /// anything, so nothing it said is used: the loop says so by name and runs every call itself.
    ///
    /// # Every call in the group is told the same time is left, and that is the right figure
    ///
    /// [`AgentLoop::invoke`] reads the clock per call because its calls run one after another, so
    /// the fourth genuinely has less time than the first. A group does not run that way: the port
    /// is free to run all of it side by side, and the shipped one does — a thread per call — so
    /// every call in it starts at the same moment and the time left at that moment is what each
    /// of them has. Dividing it, or re-reading the clock per call before handing the whole group
    /// over at once, would tell later calls they had less time than they do. The deadline is read
    /// again the moment the group returns, in [`AgentLoop::run_calls`], which is where the
    /// overshoot a group can cause is actually bounded.
    ///
    /// # Cancel and the deadline are not checked *inside* a group
    ///
    /// They are checked before it and after it, never between its calls, because there is no
    /// "between": the group is handed over in one call. What that costs is bounded by what a group
    /// is allowed to contain — pure reads, published, inside every bound, none of which asks a
    /// person — so the worst a cancel raised mid-group can do is let some reads finish. No effect
    /// happens that the run had not already admitted. The next group, and every call after it,
    /// sees the cancel.
    fn run_batch(
        &mut self,
        group: &[ToolCall],
        state: &mut RunState,
        deadline: Option<Instant>,
        sink: &mut dyn LoopSink,
    ) {
        // **Asked again on the door and not only in front of it.** `batchable` has already kept an
        // unadmitted call out of every group this loop builds, and this is the one line in the
        // crate that hands a set of calls to the port without any of them passing `invoke`. A
        // future caller assembling a group by some other rule would otherwise reopen exactly the
        // hole `batchable`'s check closes, silently and with the record still claiming a narrowing.
        // The whole group goes the slow way, so the refusal is the same refusal and the calls
        // beside it are unaffected.
        if group.iter().any(|call| self.unadmitted(call).is_some()) {
            for call in group {
                sink.emit(LoopEvent::ToolRequested(call.clone()));
                let result = self.invoke(call, deadline, sink);
                complete(call, result, state, sink);
            }
            return;
        }
        for call in group {
            sink.emit(LoopEvent::ToolRequested(call.clone()));
        }
        // With the time left on the clock, for the reason `invoke` gives: the deadline check
        // between groups cannot reach into a group already running.
        let remaining = deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
        let outcomes = self.tools.call_batch(group, remaining);
        if outcomes.len() == group.len() {
            for (call, outcome) in group.iter().zip(outcomes) {
                complete(call, within_result_bound(call, outcome), state, sink);
            }
            return;
        }

        sink.emit(LoopEvent::Warning {
            code: "batch-miscounted".to_owned(),
            message: format!(
                "the tool port answered {answered} outcome(s) for {asked} call(s); outcomes are \
                 positional, so none of them could be matched to a call and every call was run \
                 again on its own. Whatever of the {asked} the port already ran, it ran — so a \
                 call here happens a second time. A group is only ever pure reads, which is the \
                 only reason running one again is safe",
                answered = outcomes.len(),
                asked = group.len(),
            ),
        });
        for call in group {
            let result = self.invoke(call, deadline, sink);
            complete(call, result, state, sink);
        }
    }

    /// Whether a person has to say yes before this call runs.
    ///
    /// `approval` stays in the disjunction while the ports that set it are migrated — it can only
    /// add asking. `published` is [`None`] for a call routed to an entry the run did not publish,
    /// which has no verb spec of its own to consult.
    fn asks(&self, published: Option<&ToolSpec>, invoked: &ToolSpec) -> bool {
        published.is_some_and(|spec| spec.approval == Approval::Required)
            || invoked.approval == Approval::Required
            || invoked
                .envelope
                .needs_approval(self.config.unattended_ceiling)
    }

    /// Runs one call, or explains to the model why it did not run.
    ///
    /// Every refusal here comes back as a failed outcome rather than an error, because the model
    /// has to learn that the effect did not happen. Ending the run instead would leave it
    /// believing the call succeeded.
    ///
    /// # The order the checks run in is the safety argument
    ///
    /// Published or routed, then the argument bound, then the approver, then the operator's
    /// `before-call` hook, then the tool, then the result bound, then the `after-call` hook. Each
    /// stage can only remove calls the stage before it admitted, so the gate is the narrowest of
    /// them and never the widest: a call a person refused never reaches a hook — a hook that said
    /// yes to it would be approving what a person said no to — and a hook's block is one more
    /// refusal on top of theirs. A hook that could not decide blocks too (design 0002 § 3): a hook
    /// that could not run did not say yes.
    fn invoke(
        &mut self,
        call: &ToolCall,
        deadline: Option<Instant>,
        sink: &mut dyn LoopSink,
    ) -> ToolOutcome {
        // **Narrowing is checked here and not only in what was published.** `self.tools.invoked`
        // below answers for a call the *port* recognises, and it does not know this run was
        // narrowed — so without this guard a tool filtered out of the toolset was still reachable
        // by naming it, which a model can do from its own instructions without guessing. A
        // permission boundary that only hides is not one. Found by the test that asserts it.
        //
        // **What is judged is the entry, never the spelling.** Under the `verbs` surface the call
        // names `tool_invoke` and the entry is an argument, so a check reading `call.name` saw the
        // verb on every call: it refused the entry the agent's author *did* grant and it never
        // named the one it did not, leaving a record that claimed a narrowing which had not held.
        // So the port is asked what this call invokes — the same question the approver, the event
        // and the refusal text are already decided on, and the same rule design 0002 § 2 states for
        // a hook's `tools` filter — and the verb itself comes back a route, admitted whatever the
        // narrowing says because it is the only way to the entries that were granted.
        if let Some(invoked) = self.unadmitted(call) {
            return refuse_unadmitted(&invoked, sink);
        }
        let published = self.published(&call.name);
        // The spec that decides is the **invoked** one, not the published verb's: a verb over a
        // catalogue has one spec that must honestly declare every effect any entry can have, so
        // gating on that would ask a person about every read. The same spec is what the approver
        // is handed, what the event names and what the refusal says — a gate that decided on the
        // entry and then reported the verb told the model `tool_invoke` was refused and never
        // said which entry, and told an approver nothing it could decide on.
        let invoked = match (&published, self.tools.invoked(call)) {
            (_, Some(invoked)) => invoked,
            (Some(spec), None) => spec.clone(),
            (None, None) => {
                sink.emit(LoopEvent::Warning {
                    code: "unpublished-tool".to_owned(),
                    message: format!(
                        "the model called `{}`, which this run never published",
                        call.name
                    ),
                });
                return ToolOutcome::failed(format!(
                    "`{}` is not one of this run's tools; call only what was published",
                    call.name
                ));
            }
        };
        if published.is_none() {
            // A measured run under the three-verb surface spent 10 of 82 tool calls (12.2 %)
            // calling a catalogue entry by its bare name — `file_read`, `dir_list`, `run` — and
            // got `unpublished-tool` back each time, one dead turn apiece, re-learnt per state.
            //
            // Routing does not widen what the turn admits, which is what `ToolSpec`'s doc means by
            // the published set being the authority. The entry was already reachable, through the
            // verb, and it arrives here under exactly the same gate: the port's own `invoked` is
            // what names it, the approval decision below is the same decision, and the argument
            // and result bounds are the same bounds. What changes is only the spelling the model
            // used. The warning stays so the waste stays measurable.
            //
            // `published` is `None` on this path, so `asks` below sees no verb spec and reads only
            // the entry's own — its `ToolSpec::approval` and its envelope. That is deliberate and
            // it is not a hole: `ToolSpec::approval` is retired-in-progress (AGENTS.md § *Safety
            // envelope*), no shipped tool sets it, and it can only ever *add* asking, so a verb
            // that set it could not make the entry behind it any more dangerous than the entry's
            // own envelope already says it is. The envelope is what gates, and the entry's is the
            // one that describes what will actually happen. Do not reach for the verb's spec here
            // to close a gap: the gap is the field, and the field is going.
            sink.emit(LoopEvent::Warning {
                code: "unpublished-tool-routed".to_owned(),
                message: format!(
                    "the model called `{}` directly; this run publishes it behind a verb, so the \
                     call was routed to that entry under the same gate",
                    call.name
                ),
            });
        }

        if exceeds(&call.arguments, MAX_TOOL_ARGUMENT_BYTES) {
            return ToolOutcome::failed(format!(
                "the arguments for `{}` are over the {MAX_TOOL_ARGUMENT_BYTES} byte bound",
                call.name
            ));
        }

        if self.asks(published.as_ref(), &invoked) {
            sink.emit(LoopEvent::ApprovalRequired {
                call_id: call.call_id.clone(),
                name: invoked.name.clone(),
            });
            let decision = self.approvals.decide(call, &invoked);
            sink.emit(LoopEvent::ApprovalResolved {
                call_id: call.call_id.clone(),
                approved: decision.is_approved(),
            });
            if let ApprovalDecision::Denied { reason } = decision {
                let name = refused_name(call, &invoked);
                return ToolOutcome::failed(format!("{name} was not approved: {reason}"));
            }
        }

        if let Some(refusal) = self.before_call_hook(call, &invoked, sink) {
            return refusal;
        }

        // With the time left on the clock, so a call that starts something bounds it by that: the
        // deadline check between calls cannot reach into a call already running, and one `run` of
        // a suite is longer than most budgets.
        let remaining = deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
        let result = self.tools.call_within(call, remaining);
        let result = within_result_bound(call, result);
        self.after_call_hook(call, &invoked, result, sink)
    }

    /// The `before-call` hook (design 0002 § 3): the operator's last word before an effect.
    ///
    /// Consulted after the approver, and only ever narrowing — [`Some`] is the failed outcome the
    /// model reads instead of a result, and the tool port is not reached at all. A hook that could
    /// not decide is treated as a block, because the point of the hook is that nothing happens it
    /// has not seen; the model is told which of the two it was, so *the guard is broken* and *the
    /// guard said no* are not the same message.
    fn before_call_hook(
        &mut self,
        call: &ToolCall,
        invoked: &ToolSpec,
        sink: &mut dyn LoopSink,
    ) -> Option<ToolOutcome> {
        let hooks = self.hooks.as_deref_mut()?;
        let decision = hooks.before_call(call, invoked);
        sink.emit(LoopEvent::HookRan {
            point: HookPoint::BeforeCall,
            call_id: Some(call.call_id.clone()),
            decision: decision.clone(),
        });
        let name = refused_name(call, invoked);
        match decision {
            HookDecision::Proceed => None,
            HookDecision::Block { reason } => Some(ToolOutcome::failed(format!(
                "{name} was blocked by a hook: {reason}"
            ))),
            HookDecision::Failed { reason } => Some(ToolOutcome::failed(format!(
                "{name} did not run because a hook could not check it: {reason}"
            ))),
        }
    }

    /// The `after-call` hook (design 0002 § 3): the operator's programs read the outcome and may
    /// leave the model a note beside it — that a formatter ran, that a check failed.
    ///
    /// A note is not a verdict: `failed` stays exactly as the tool set it. Marking an outcome
    /// failed after the effect has already happened would tell the model nothing happened when
    /// something did; `before-call` is the point that speaks in time to prevent it.
    ///
    /// [`MAX_TOOL_RESULT_BYTES`] is checked a **second** time here, over the noted result, so a
    /// note cannot become the one way an oversized payload reaches the model. When the note is
    /// what crosses the bound the refusal says so by name: the tool's result was inside it, and a
    /// model told to *narrow the request* would narrow one that was never too large.
    fn after_call_hook(
        &mut self,
        call: &ToolCall,
        invoked: &ToolSpec,
        result: ToolOutcome,
        sink: &mut dyn LoopSink,
    ) -> ToolOutcome {
        let Some(hooks) = self.hooks.as_deref_mut() else {
            return result;
        };
        let AfterCall { note, decision } = hooks.after_call(call, invoked, &result);
        // Emitted whether or not there was a note: the record says the hook was consulted, and a
        // point that fired silently reads exactly like one that never ran. The decision is the
        // hook's own — a hook that crashed here says so — and never the outcome's: `failed` below
        // stays the tool's, because an after-call hook may not fail a result.
        sink.emit(LoopEvent::HookRan {
            point: HookPoint::AfterCall,
            call_id: Some(call.call_id.clone()),
            decision,
        });
        let Some(note) = note else {
            return result;
        };
        let noted = with_hook_note(result, note);
        if exceeds(&noted.output, MAX_TOOL_RESULT_BYTES) {
            return ToolOutcome::failed(format!(
                "the result of `{}` was inside the {MAX_TOOL_RESULT_BYTES} byte bound until an \
                 after-call hook's note was added to it; neither the result nor the note is \
                 forwarded, because a cut-down result reads exactly like a whole one",
                call.name
            ));
        }
        noted
    }

    /// Whether this run's grant reaches anything its port does not publish.
    ///
    /// **The one condition under which a route means anything.** A grant is built by
    /// [`AgentLoop::agent_for`] as an intersection with
    /// [`reachable`](harness_wire::ToolPort::reachable), so under a flat surface every name in it
    /// is a published tool and this is false: the run needs no indirection and gets none. Under a
    /// verb surface none of them is — the grant is `file_read`, `search`, and what is published is
    /// `tool_invoke` — so this is true and the verbs become reachable as routes.
    ///
    /// False for an **empty** grant, which is what closes the disclosure a narrowed-to-nothing run
    /// would otherwise keep: with no entry to reach, a route leads nowhere and all it can still do
    /// is enumerate the catalogue for a child that was admitted none of it. An empty grant
    /// publishes nothing and admits nothing under either surface, which is the answer flat already
    /// gave.
    fn needs_routes(&self, admitted: &[harness_wire::ToolName]) -> bool {
        admitted
            .iter()
            .any(|name| !self.tools.specs().iter().any(|spec| &spec.name == name))
    }

    /// The published names this run may call as **routes** rather than as capabilities.
    ///
    /// A route carries a call to something else and performs nothing itself: `tool_invoke` runs no
    /// entry until an argument says which, and taking it from a narrowed run would leave the run
    /// unable to reach the entries it *was* granted. So while a run needs indirection its port's
    /// published names are admitted as routes, and what the narrowing decides is the entry at the
    /// end of the route — which [`harness_wire::ToolPort::invoked`] names and
    /// [`AgentLoop::unadmitted`] judges.
    ///
    /// # Why this is not `specs()` minus `reachable()`
    ///
    /// It was, and that inverted the failure direction it claimed. Subtraction makes the exempt set
    /// **larger** the less a port says it can reach, so a port answering `reachable` with nothing
    /// turned every tool it published into a route and had them all admitted — the opposite of the
    /// safety this method's own doc promised, and reachable from outside this crate because
    /// `reachable` is defaulted. Asked positively it cannot widen: `needs_routes` is false unless
    /// the grant holds a name the port does not publish, and a grant is only ever an intersection
    /// with what the port said it can reach.
    fn routes(&self, admitted: &[harness_wire::ToolName]) -> Vec<harness_wire::ToolName> {
        if !self.needs_routes(admitted) {
            return Vec::new();
        }
        self.tools
            .specs()
            .iter()
            .map(|spec| spec.name.clone())
            .collect()
    }

    /// Whether this run may use `name`, which is the whole of the narrowing rule.
    ///
    /// *Admitted, or a route to something admitted.* [`AgentLoop::port_specs`] filters the
    /// published toolset by it and [`AgentLoop::unadmitted`] judges calls by it, so a tool taken
    /// out of the list is also refused when the model names it anyway and the two cannot drift.
    fn admits(&self, name: &harness_wire::ToolName) -> bool {
        let Some(admitted) = &self.config.admits else {
            return true;
        };
        admitted.contains(name) || self.routes(admitted).contains(name)
    }

    /// The name this call would run that this run was never admitted, or [`None`] if it may run.
    ///
    /// **The narrowing, as one question, asked at every site that can put a call on the port.**
    /// [`AgentLoop::invoke`] asks it and refuses; [`AgentLoop::batchable`] asks it and keeps the
    /// call out of a group so that it reaches `invoke` and is refused there; [`AgentLoop::run_batch`]
    /// asks it once more of a group it is handed, because that is the site that actually calls
    /// [`harness_wire::ToolPort::call_batch`] and a check standing in front of a door is not the
    /// same as a check on it. Three sites, one question, so there is nothing to drift.
    ///
    /// What is judged is the name [`harness_wire::ToolPort::invoked`] answers, never the spelling
    /// the model used: behind a verb the call is named `tool_invoke` and the entry is an argument.
    fn unadmitted(&self, call: &ToolCall) -> Option<harness_wire::ToolName> {
        // An unnarrowed run asks the port nothing: `invoked` is a question a port may answer by
        // walking a catalogue, and every call would pay for it to be told `None` is `None`.
        self.config.admits.as_ref()?;
        let invoked = self
            .tools
            .invoked(call)
            .map_or_else(|| call.name.clone(), |spec| spec.name);
        (!self.admits(&invoked)).then_some(invoked)
    }

    /// What this run's port publishes, after any narrowing the run was configured with.
    ///
    /// One half of the one place the filter is applied. [`AgentLoop::request`] renders this to the
    /// model and [`AgentLoop::published`] admits calls from it, so a tool a named agent was not
    /// given is absent from the toolset *and* refused if the model names it anyway — the same
    /// publication rule the machine's own capabilities already follow, applied to a second
    /// question, through the same [`AgentLoop::admits`] the other half asks.
    fn port_specs(&self) -> Vec<ToolSpec> {
        let specs = self.tools.specs();
        if self.config.admits.is_none() {
            return specs.to_vec();
        }
        specs
            .iter()
            .filter(|spec| self.admits(&spec.name))
            .cloned()
            .collect()
    }

    fn published(&self, name: &harness_wire::ToolName) -> Option<ToolSpec> {
        self.port_specs()
            .into_iter()
            .find(|spec| &spec.name == name)
    }

    /// The stop hook (design 0002 § 3): the operator's last word on a run that would end here.
    /// A block's reason becomes one more user item and the loop turns again, at most
    /// [`MAX_STOP_HOOK_CONTINUES`] times. Returns `None` to turn again.
    ///
    /// # It does not fire at the end of a delegate
    ///
    /// The point is *when the run would end*, and a child's ending is not the run's: the parent is
    /// still inside the tool call and will go on turning afterwards. A hook consulted there would
    /// be asked *has this run finished?* about a run nobody started, and a block would turn the
    /// child again — up to [`MAX_STOP_HOOK_CONTINUES`] times **per delegate**, each time on budget
    /// the parent carved, for an end the operator was never trying to hold. So a nested loop
    /// returns the stop unchanged. `before-call` and `after-call` still fire inside a child: they
    /// are about calls, and a delegate's calls are the run's calls.
    fn stop_hook(
        &mut self,
        stop: LoopStop,
        state: &mut RunState,
        sink: &mut dyn LoopSink,
    ) -> Option<LoopStop> {
        if self.nested {
            return Some(stop);
        }
        let Some(hooks) = self.hooks.as_deref_mut() else {
            return Some(stop);
        };
        // What the run is about to answer with: the structured answer where there is one, else
        // the prose. A hook reads the thing a consumer would have read.
        let text = state
            .structured
            .as_ref()
            .map_or_else(|| state.text.clone(), serde_json::Value::to_string);
        let decision = hooks.on_stop(&text);
        sink.emit(LoopEvent::HookRan {
            point: HookPoint::Stop,
            call_id: None,
            decision: decision.clone(),
        });
        match decision {
            HookDecision::Proceed => Some(stop),
            // Fail open, and say so: a hook that crashed must not keep a run alive for ever.
            HookDecision::Failed { reason } => {
                sink.emit(LoopEvent::Warning {
                    code: "hook-failed".to_owned(),
                    message: format!("the stop hook could not decide, so the run ends: {reason}"),
                });
                Some(stop)
            }
            HookDecision::Block { reason } => {
                if state.stop_continues >= MAX_STOP_HOOK_CONTINUES {
                    sink.emit(LoopEvent::Warning {
                        code: "stop-hook-exhausted".to_owned(),
                        message: format!(
                            "the stop hook blocked the end of this run {MAX_STOP_HOOK_CONTINUES} \
                             times and it ends anyway; its last reason: {reason}"
                        ),
                    });
                    return Some(stop);
                }
                state.stop_continues += 1;
                // A continuation is a **new** ending, so the answer nudge is owed again: this run
                // answered, the operator's hook sent it back to work, and what it does next is a
                // fresh chance to end in prose. Counting nudges per run instead spent the only one
                // on the first ending and stopped the second `Unstructured` without ever asking —
                // an empty stdout and exit 2 where one more sentence would almost certainly have
                // produced the answer.
                state.nudged = 0;
                // An answer already given is withdrawn: the model is being asked to go on, and a
                // later answer replaces this one.
                state.structured = None;
                state.items.push(Item::user(reason));
                None
            }
        }
    }
}

#[cfg(test)]
mod tests;
