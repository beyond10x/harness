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

mod approval;
mod budget;
mod event;
mod price;

use std::time::Instant;

use harness_wire::{
    Approval, Item, MAX_TOOL_ARGUMENT_BYTES, MAX_TOOL_RESULT_BYTES, ModelPort, Risk, Sampling,
    StopReason, StreamEvent, StreamSink, ToolCall, ToolOutcome, ToolPort, ToolSpec, TurnRequest,
    Usage, WireError, WireErrorCode, exceeds,
};
use serde::{Deserialize, Serialize};

pub use approval::{ApprovalDecision, ApprovalPort, ApproveAll, DenyAll};
pub use budget::{Budget, BudgetError};
pub use event::{LoopEvent, LoopSink, NullLoopSink, VecLoopSink};
pub use price::{ModelRates, RateCard, RateCardError, Rates, micro_usd_as_decimal};

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
}

impl LoopOutcome {
    /// Returns the summed reported token counts, or `None` when no turn reported any.
    pub fn total_tokens(&self) -> Option<(u64, u64)> {
        if self.usage.is_empty() {
            return None;
        }
        Some(self.usage.iter().fold((0, 0), |(input, output), usage| {
            (
                input.saturating_add(usage.input_tokens),
                output.saturating_add(usage.output_tokens),
            )
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoopError {
    #[error("budget refused: {0}")]
    Budget(#[from] BudgetError),
    #[error("model wire refused: {0}")]
    Wire(#[from] WireError),
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
        }
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
}

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
    text: String,
}

impl RunState {
    fn new(input: impl Into<String>) -> Self {
        Self {
            items: vec![Item::user(input)],
            usage: Vec::new(),
            turns: 0,
            input_total: 0,
            output_total: 0,
            cost_total: None,
            text: String::new(),
        }
    }

    /// Folds one turn into the run and returns the calls it asked for.
    fn absorb(
        &mut self,
        outcome: harness_wire::TurnOutcome,
        prices: Option<&RateCard>,
        sink: &mut dyn LoopSink,
    ) -> (Vec<ToolCall>, StopReason) {
        if let Some(reported) = outcome.usage {
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

    fn into_outcome(self, stop: LoopStop) -> LoopOutcome {
        LoopOutcome {
            stop,
            text: self.text,
            items: self.items,
            turns: self.turns,
            usage: self.usage,
            cost_micro_usd: self.cost_total,
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
fn compact(items: &mut [Item], sink: &mut dyn LoopSink) {
    fn measure(items: &[Item]) -> usize {
        items
            .iter()
            .map(|item| serde_json::to_string(item).map_or(0, |json| json.len()))
            .sum()
    }
    if measure(items) <= MAX_CONVERSATION_BYTES {
        return;
    }

    let results: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| matches!(item, Item::ToolResult { output, .. } if !is_elided(output)))
        .map(|(index, _)| index)
        .collect();
    // The newest results, whole, until they come to [`KEPT_RESULT_BYTES`] — and always at least
    // one, because a model that cannot see the result of the call it just made is stuck.
    let mut kept = 0_usize;
    let mut protected = 0_usize;
    for &index in results.iter().rev() {
        let size = serde_json::to_string(&items[index]).map_or(0, |json| json.len());
        if protected > 0 && kept + size > KEPT_RESULT_BYTES {
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
        if measure(items) <= COMPACTED_TARGET_BYTES {
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

    if count > 0 {
        // Said out loud: a model that suddenly cannot see a file it read has a right to a reason,
        // and so does anyone reading the record afterwards.
        sink.emit(LoopEvent::Warning {
            code: "conversation-compacted".to_owned(),
            message: format!(
                "the conversation passed {MAX_CONVERSATION_BYTES} bytes, so {count} old tool \
                 result(s) were elided, freeing {freed} bytes and leaving {now}. The most recent \
                 {protected} result(s), {kept} bytes, are untouched.",
                now = measure(items)
            ),
        });
    }
}

/// Whether this output has already been elided, so nothing is counted or elided twice.
fn is_elided(output: &serde_json::Value) -> bool {
    output.get("elided").is_some()
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
    config: LoopConfig,
    cancel: LoopCancel,
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
            StreamEvent::Warning { code, message } => LoopEvent::Warning { code, message },
        });
    }
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
            config,
            cancel: LoopCancel::new(),
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

    /// Runs until the model answers, a budget binds, or the caller cancels.
    ///
    /// # Errors
    ///
    /// Returns [`LoopError::Budget`] before the first request when a bound is unusable, and
    /// [`LoopError::Wire`] when a turn could not be obtained at all. A budget that *binds* is an
    /// outcome, not an error.
    pub fn run(
        &mut self,
        input: impl Into<String>,
        sink: &mut dyn LoopSink,
    ) -> Result<LoopOutcome, LoopError> {
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
        let mut state = RunState::new(input);

        sink.emit(LoopEvent::Started {
            model: self.config.model.clone(),
            published_tools: self
                .tools
                .specs()
                .iter()
                .map(|spec| spec.name.clone())
                .collect(),
            operations: self
                .tools
                .operations()
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
        });
        self.announce_prices(priced, sink);

        let stop = self.drive(&mut state, deadline, sink)?;
        sink.emit(LoopEvent::Finished {
            stop: stop.clone(),
            turns: state.turns,
        });
        Ok(state.into_outcome(stop))
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
            compact(&mut state.items, sink);
            state.turns += 1;
            sink.emit(LoopEvent::TurnStarted { turn: state.turns });
            let outcome = {
                let request = self.request(state);
                let mut forward = Forward(sink);
                match self.model.turn(&request, &mut forward) {
                    Ok(outcome) => outcome,
                    // A read that stopped because the caller cancelled is them getting what they
                    // asked for. Reporting it as a failure would tell a person who pressed Ctrl-C
                    // that something went wrong.
                    Err(error) if error.code == WireErrorCode::Cancelled => return Ok(cancelled()),
                    Err(error) => return Err(error.into()),
                }
            };

            let (calls, stop_reason) = state.absorb(outcome, self.config.prices.as_ref(), sink);
            if calls.is_empty() {
                return Ok(terminal_stop(stop_reason));
            }
            if let Some(stop) = self.run_calls(calls, state, deadline, sink) {
                return Ok(stop);
            }
            if let Some(stop) = self.stop_after_tokens(state) {
                return Ok(stop);
            }
        }
    }

    fn request(&self, state: &RunState) -> TurnRequest {
        TurnRequest {
            model: self.config.model.clone(),
            instructions: self.config.instructions.clone(),
            items: state.items.clone(),
            tools: self.tools.specs().to_vec(),
            max_output_tokens: self.config.budget.max_output_tokens_per_turn,
            sampling: self.config.sampling.clone(),
        }
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

    /// Runs the turn's calls in order, stopping the moment the caller cancels or time runs out.
    fn run_calls(
        &mut self,
        calls: Vec<ToolCall>,
        state: &mut RunState,
        deadline: Option<Instant>,
        sink: &mut dyn LoopSink,
    ) -> Option<LoopStop> {
        let mut calls = calls.into_iter();
        for call in calls.by_ref() {
            if self.cancel.is_cancelled() {
                // Every call the model made needs an answer in the conversation, even one that
                // never ran: a `function_call` replayed without its output is a provider error on
                // the next turn, so a cancelled run could not be resumed at all.
                state.items.push(Item::result(
                    call.call_id,
                    ToolOutcome::failed("the run was cancelled before this call ran"),
                ));
                for skipped in calls {
                    state.items.push(Item::result(
                        skipped.call_id,
                        ToolOutcome::failed("the run was cancelled before this call ran"),
                    ));
                }
                return Some(cancelled());
            }
            // Between calls as well as between turns: one call can block for minutes, so a
            // deadline checked only at the turn boundary overshoots by a whole call — and a turn
            // asking for six of them overshoots by six. A call already running still runs to its
            // own timeout; nothing here reaches into it. Every skipped call still gets an
            // outcome, for the reason the cancellation branch above gives.
            if let Some(stop) = self.deadline_passed(deadline) {
                for skipped in std::iter::once(call).chain(calls) {
                    state.items.push(Item::result(
                        skipped.call_id,
                        ToolOutcome::failed("the run's deadline passed before this call ran"),
                    ));
                }
                return Some(stop);
            }
            sink.emit(LoopEvent::ToolRequested(call.clone()));
            let result = self.invoke(&call, sink);
            sink.emit(LoopEvent::ToolCompleted {
                call_id: call.call_id.clone(),
                failed: result.failed,
            });
            state.items.push(Item::result(call.call_id, result));
        }
        None
    }

    /// Runs one call, or explains to the model why it did not run.
    ///
    /// Every refusal here comes back as a failed outcome rather than an error, because the model
    /// has to learn that the effect did not happen. Ending the run instead would leave it
    /// believing the call succeeded.
    fn invoke(&mut self, call: &ToolCall, sink: &mut dyn LoopSink) -> ToolOutcome {
        let Some(spec) = self.published(&call.name) else {
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
        };

        if exceeds(&call.arguments, MAX_TOOL_ARGUMENT_BYTES) {
            return ToolOutcome::failed(format!(
                "the arguments for `{}` are over the {MAX_TOOL_ARGUMENT_BYTES} byte bound",
                call.name
            ));
        }

        // The spec that decides is the **invoked** one, not the published verb's: a verb over a
        // catalogue has one spec that must honestly declare every effect any entry can have, so
        // gating on that would ask a person about every read. The same spec is what the approver
        // is handed, what the event names and what the refusal says — a gate that decided on the
        // entry and then reported the verb told the model `tool_invoke` was refused and never
        // said which entry, and told an approver nothing it could decide on. `approval` stays in
        // the disjunction while the ports that set it are migrated — it can only add asking.
        let invoked = self.tools.invoked(call).unwrap_or_else(|| spec.clone());
        let asks = spec.approval == Approval::Required
            || invoked.approval == Approval::Required
            || invoked
                .envelope
                .needs_approval(self.config.unattended_ceiling);

        if asks {
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
                return ToolOutcome::failed(if invoked.name == call.name {
                    format!("`{}` was not approved: {reason}", call.name)
                } else {
                    format!(
                        "`{}` (called through `{}`) was not approved: {reason}",
                        invoked.name, call.name
                    )
                });
            }
        }

        let result = self.tools.call(call);
        if exceeds(&result.output, MAX_TOOL_RESULT_BYTES) {
            // Not truncated: a truncated result reads to the model exactly like a complete one.
            return ToolOutcome::failed(format!(
                "the result of `{}` is over the {MAX_TOOL_RESULT_BYTES} byte bound; narrow the \
                 request",
                call.name
            ));
        }
        result
    }

    fn published(&self, name: &harness_wire::ToolName) -> Option<ToolSpec> {
        self.tools
            .specs()
            .iter()
            .find(|spec| &spec.name == name)
            .cloned()
    }
}

#[cfg(test)]
mod tests;
