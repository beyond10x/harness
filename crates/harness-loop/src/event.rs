use harness_wire::{CallId, ToolCall, ToolName, Usage};
use serde::{Deserialize, Serialize};

use crate::hook::{HookDecision, HookPoint};
use crate::{ContextManifestEntry, LoopStop};

/// One tool a run declared and its machine would not admit.
///
/// # Why this is a plain pair of strings and not a shared type
///
/// The fact is computed where the machine is known — `harness_substrate` reads substrate's own
/// capability facts and names the predicate that failed — and this crate depends only on
/// `harness_wire`, which performs no I/O and knows nothing about any machine (`AGENTS.md`
/// invariant 3). So the loop carries what it can honestly carry: the entry that does not exist and
/// the sentence saying why, both already written by whoever knew. Nothing here interprets either.
///
/// The `reason` is meant to be read, not matched on. It names the fact the machine stated — or did
/// not — in the vocabulary a substrate refusal already uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Withheld {
    /// The tool that is not published here — the catalogue **entry**, never the surface's verb.
    pub tool: String,
    /// The predicate that failed, as the machine stated it.
    pub reason: String,
}

/// One profile that contributed to how this run was configured.
///
/// **The condition on which a file is allowed to carry a permission.** A profile can set an
/// approval ceiling, an allow-list and a write scope — the things a person would otherwise type
/// and review — so a run whose limits came out of a file must name the file, or the record has
/// stopped explaining the run. The digest is over the profile's own table, so the attribution
/// survives somebody editing an unrelated profile beside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileRef {
    /// What it was called: `default`, or a `[[profiles]]` name.
    pub name: String,
    /// Where it was read from.
    pub source: String,
    /// SHA-256 of what it said, hex.
    pub sha256: String,
}

/// A credential this run renewed before it started, and wrote back where its owner keeps it.
///
/// **The record of a side effect on a file this harness does not own.** A run that renews a
/// subscription token rewrites a vendor's credential store — the strongest thing a harness does to
/// a machine short of the tools it publishes — and the only thing that separates that from a
/// program quietly editing your home directory is that the run says it did, names the file, and
/// says what the new credential is good for.
///
/// **Nothing here is derived from the token.** Not a prefix, not a length, not a digest: a digest
/// of a credential is an oracle for it, and the fields below are the ones a person actually needs
/// to answer *was this run's credential fresh, and what did it change*.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRenewal {
    /// The document that was rewritten.
    pub source: String,
    /// The provider whose renewal was used — the entry that named the endpoint and the client.
    pub provider: String,
    /// When the credential this run holds expires, from its own `exp`.
    ///
    /// [`None`] when the new token is not a JWT: the renewal still happened, and this build simply
    /// cannot date what it got. Absence here is *not* "it does not expire".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_unix: Option<u64>,
    /// Whether the authorization server retired the refresh token that was on disk.
    ///
    /// **The field that matters after something goes wrong.** When it is true, a backup of that
    /// file taken before this run no longer holds a credential that works, so restoring one is not
    /// the recovery it looks like — the owner has to log in again.
    pub refresh_token_rotated: bool,
    /// Whether every byte the renewal did not have to change survived it.
    ///
    /// `true` on the ordinary path: only the token values moved, and the owner's key order,
    /// indentation and any key this build never heard of are exactly where they were. `false` is a
    /// document that had to be re-serialised — still correct, still complete, and reordered. Said
    /// out loud because "your credential file looks different" is a thing somebody notices later
    /// and has no other way to explain.
    pub byte_preserving: bool,
}

/// Body-free provenance for one resolved toolchain provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolchainRef {
    pub name: String,
    pub source: String,
    pub sha256: String,
}

/// Body-free provenance for one outbound MCP connection frozen before the run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpRef {
    pub connection: String,
    pub registry_sha256: String,
    pub profile_sha256: String,
    pub snapshot_sha256: String,
    pub protocol_version: String,
}

/// What `credential_source` says when nothing named a provider.
fn named_credential() -> String {
    "named".to_owned()
}

/// Everything a person or a shell can observe while the loop runs.
///
/// Deliberately closed and vendor-neutral: the terminal renderer, the bridge shell and the test
/// assertions all read the same stream, so there is one description of what happened rather than
/// three that can disagree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum LoopEvent {
    Started {
        model: String,
        published_tools: Vec<ToolName>,
        /// Every neutral operation this run could perform — see [`harness_wire::ToolPort::operations`].
        ///
        /// Beside the tool names rather than instead of them, because they answer different
        /// questions and a reader needs both: what the model was **offered**, and what the run
        /// could **do**. Behind three verbs the first is the same on every run and only the second
        /// varies.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        operations: Vec<String>,
        /// Tools this run **asked for** that the machine would not admit, and why.
        ///
        /// The publication gate works by absence — a tool the machine cannot confine is never
        /// published, so the model never plans around it — and an absence in this record reads
        /// identically to a run that never wanted the tool. It is not: a driven session whose only
        /// legal route was running a program was published six entries instead of seven, no error
        /// and no warning, hand-wrote files instead, and the failure read as a model failure for
        /// weeks.
        ///
        /// Beside `published_tools` and `operations`, which say what the run *has*. This says what
        /// it was *denied*, which is the only field that can distinguish those two silences.
        ///
        /// Empty on every run that got what it asked for, and **written even then**.
        ///
        /// It was skipped when empty, for byte-identity with records written before the field
        /// existed. That cost more than it bought: absence then meant *nothing was withheld* or
        /// *a build that predates the field*, and a reader outside this process cannot tell those
        /// apart — `b10x-harness` reports `0.1.0` either way, so the version does not decide it.
        /// A driven run that withheld nothing was read as one that never said, which is exactly
        /// the silence this field was added to break.
        ///
        /// So it is always on the wire. Absence now means one thing only: a build older than this
        /// one. Observers may keep reading absence as *did not say* and be right.
        #[serde(default)]
        withheld: Vec<Withheld>,
        /// The skills this run offered, by name, and the agents it published.
        ///
        /// **Always written, both of them, empty included** — the rule `withheld` above was fixed
        /// to. A reader outside this process cannot tell *this run had none* from *this build does
        /// not say* unless the record says which, and "the model was never offered the guidance"
        /// and "we cannot tell whether it was" are different findings about a run.
        ///
        /// Names only. What a skill says is what the `skill` tool answers with, and putting a body
        /// in a session record would put it in every reader's face on every run.
        #[serde(default)]
        skills: Vec<String>,
        #[serde(default)]
        agents: Vec<String>,
        /// The profiles that configured this run, in the order they were applied.
        ///
        /// Always written, empty included, for the reason `withheld` is: skip-when-empty makes
        /// *no profile* and *this build does not say* the same bytes to a reader outside the
        /// process, and one of those is a run configured entirely by typed flags.
        #[serde(default)]
        profiles: Vec<ProfileRef>,
        /// Body-free provenance for every instruction layer sent to the model.
        #[serde(default)]
        context: Vec<ContextManifestEntry>,
        /// Declarative provider definitions used by the run. Bodies and installation paths stay out.
        #[serde(default)]
        toolchains: Vec<ToolchainRef>,
        /// Outbound MCP registry, policy and discovery facts frozen for this run.
        #[serde(default)]
        mcp: Vec<McpRef>,
        /// Where this run's credential came from — `named`, or `provider:<name>`.
        ///
        /// **This field is what pays for a provider being allowed to default a credential path at
        /// all.** `resolve_credential`'s own doc refuses an ambient fallback on the grounds that a
        /// harness which quietly picks up a key is one whose runs cannot be explained afterwards.
        /// A built-in provider naming a vendor directory is a default, and the difference between
        /// a default and an ambient fallback is entirely this: the record says which. If it ever
        /// stops being written, the default is no longer accountable and should go with it.
        #[serde(default = "named_credential")]
        credential_source: String,
    },
    /// A credential was renewed and written back, before the first turn.
    ///
    /// Emitted once, ahead of [`LoopEvent::Started`], because that is when it happened: the run
    /// held a stale token, renewed it, and rewrote the file its owner keeps it in — all before
    /// anything was sent to a model. A run that renewed nothing emits nothing here, which is the
    /// ordinary case and reads correctly as one: unlike the always-written lists on `Started`,
    /// this is an *act*, and an act that did not happen has no empty form.
    CredentialRenewed(CredentialRenewal),
    /// The actor-specific context used by following turns changed.
    ContextChanged {
        revision: String,
        /// Body-free provenance; prompt bodies never enter the event journal.
        context: Vec<ContextManifestEntry>,
    },
    /// The exact actor-specific tool inventory used by following turns changed.
    InventoryChanged {
        revision: String,
        published_tools: Vec<ToolName>,
    },
    TurnStarted {
        turn: u64,
    },
    /// A turn whose stream broke after it had already emitted something, being attempted again.
    ///
    /// **Whatever streamed for this turn is to be discarded.** A wire never retries once it has
    /// witnessed output — a second attempt would append a second copy of text a person already
    /// read — so the decision moves up here, where the conversation is known to be unchanged by
    /// the failed attempt. A renderer that shows deltas has to act on this: without it a person
    /// sees the first half of an answer, then a whole answer, and no reason for either.
    ///
    /// `attempt` counts the retries, so the first one is 1 and the last is
    /// [`crate::MAX_TURN_RETRIES`].
    TurnRetried {
        turn: u64,
        attempt: u32,
        /// What the wire said went wrong, verbatim.
        reason: String,
    },
    /// The conversation was made smaller before a turn, and by what means.
    ///
    /// Emitted once per compaction, whether it elided, summarised or both. A reader of a finished
    /// run needs it to explain a jump in the record: an assistant that suddenly cannot quote a file
    /// it read, or a turn whose input tokens fall by half.
    ///
    /// Counted rather than described, because the prose lives in the `conversation-compacted`
    /// warning beside it and a machine reading the record needs figures.
    Compacted {
        /// Tool results whose payload was replaced by a note saying how much went.
        elided_results: usize,
        elided_bytes: usize,
        /// Items folded into one summary item. Zero when no summary was made.
        summarised_items: usize,
        bytes_before: usize,
        bytes_after: usize,
        /// Whether a model turn was spent on a summary. True even when that turn failed, because
        /// it was still paid for.
        summary_turn: bool,
    },
    TextDelta {
        text: String,
    },
    ToolArgumentsDelta {
        call_id: CallId,
        delta: String,
    },
    /// A fragment of the model's reasoning summary, as the provider streamed it.
    ///
    /// Forwarded so a person watching a long think sees something happening. Never part of the
    /// answer and never replayed: the conversation carries the reasoning as an opaque item.
    ReasoningDelta {
        text: String,
    },
    ToolRequested {
        #[serde(flatten)]
        call: ToolCall,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operation: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        subjects: Vec<String>,
    },
    ApprovalRequired {
        call_id: CallId,
        /// What is being decided. For a verb over a catalogue this is the **entry** — `run`,
        /// `file_write` — and not the verb it came through; the `ToolRequested` event with the
        /// same `call_id` carries the verb and its arguments.
        name: ToolName,
    },
    ApprovalResolved {
        call_id: CallId,
        approved: bool,
    },
    ToolCompleted {
        call_id: CallId,
        failed: bool,
    },
    /// Reported token counts for one turn. Absent usage produces no event at all.
    Usage(Usage),
    /// The rate card in force, emitted once before the first turn.
    ///
    /// Carried in the record rather than left on the command line so that every cost figure below
    /// can be traced to the rates that produced it — and so that a run priced from a stale card is
    /// distinguishable from one priced this morning. A run with no card emits no event here, and
    /// then reports no cost at all.
    Rates {
        source: String,
        as_of: String,
    },
    /// What one turn's reported tokens cost, in millionths of a US dollar.
    ///
    /// Emitted only where the card prices the model the provider served. **A turn nobody could
    /// price produces no event**, never a zero: silence means no rate was supplied, and zero would
    /// mean the turn was free.
    ///
    /// Rounded per turn, so the events below a run sum exactly to the total reported with it.
    Cost {
        model: String,
        micro_usd: u64,
    },
    Warning {
        code: String,
        message: String,
    },
    /// The model called the answer tool: `value` is the run's structured answer.
    ///
    /// Carried in the event, not only in the outcome, so the JSONL record of a run has it.
    Answered {
        call_id: CallId,
        value: serde_json::Value,
    },
    /// A delegate started on `task`, inside the tool call `call_id`.
    DelegateStarted {
        call_id: CallId,
        task: String,
    },
    /// The delegate inside `call_id` ended, however it ended.
    DelegateFinished {
        call_id: CallId,
        stop: LoopStop,
        turns: u64,
    },
    /// One event of the delegate running inside `call_id`, wrapped.
    ///
    /// Everything a child emits arrives this way and nothing of it arrives bare: its text is not
    /// the parent's answer, and its `Usage` and `Cost` are not the parent's turns. The parent's
    /// [`crate::LoopOutcome::usage`] still includes them, so totals are right.
    Delegated {
        call_id: CallId,
        event: Box<LoopEvent>,
    },
    /// A hook was consulted and this is what it said (design 0002 § 3).
    HookRan {
        point: HookPoint,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_id: Option<CallId>,
        decision: HookDecision,
    },
    Finished {
        stop: LoopStop,
        /// How many model turns the run took, counting the first.
        ///
        /// Beside the stop rather than inside it, because only two `LoopStop` variants carry a
        /// turn count and both mean *a bound bound* — so a reader wanting "how long was this run"
        /// got an answer from a run that hit a ceiling and `null` from one that finished, which is
        /// backwards. An advisory bound on run length could not decide a single completed run.
        #[serde(default)]
        turns: u64,
    },
}

pub trait LoopSink {
    fn emit(&mut self, event: LoopEvent);
}

/// A sink that retains everything, for tests and for callers that only want the record.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct VecLoopSink {
    events: Vec<LoopEvent>,
}

impl VecLoopSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> &[LoopEvent] {
        &self.events
    }

    pub fn into_events(self) -> Vec<LoopEvent> {
        self.events
    }

    /// Returns the streamed text as a reader saw it grow.
    pub fn text(&self) -> String {
        self.events
            .iter()
            .filter_map(|event| match event {
                LoopEvent::TextDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn warnings(&self) -> impl Iterator<Item = (&str, &str)> {
        self.events.iter().filter_map(|event| match event {
            LoopEvent::Warning { code, message } => Some((code.as_str(), message.as_str())),
            _ => None,
        })
    }
}

impl LoopSink for VecLoopSink {
    fn emit(&mut self, event: LoopEvent) {
        self.events.push(event);
    }
}

/// Discards everything. Useful when the caller only wants the outcome.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NullLoopSink;

impl LoopSink for NullLoopSink {
    fn emit(&mut self, _: LoopEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vec_sink_reassembles_text_and_lists_warnings() {
        let mut sink = VecLoopSink::new();
        sink.emit(LoopEvent::TextDelta {
            text: "one ".to_owned(),
        });
        sink.emit(LoopEvent::Warning {
            code: "unknown-tool".to_owned(),
            message: "nope".to_owned(),
        });
        sink.emit(LoopEvent::TextDelta {
            text: "two".to_owned(),
        });
        assert_eq!(sink.text(), "one two");
        assert_eq!(
            sink.warnings().collect::<Vec<_>>(),
            vec![("unknown-tool", "nope")]
        );
    }

    #[test]
    fn a_started_event_states_what_the_machine_would_not_admit() {
        let event = LoopEvent::Started {
            model: "m".to_owned(),
            published_tools: vec![ToolName::new("tool_invoke").expect("valid")],
            operations: vec!["file.read".to_owned()],
            withheld: vec![Withheld {
                tool: "run".to_owned(),
                reason: "`exec.argv-only` must be true and this machine says nothing.".to_owned(),
            }],
            skills: Vec::new(),
            agents: Vec::new(),
            profiles: Vec::new(),
            context: Vec::new(),
            toolchains: Vec::new(),
            mcp: Vec::new(),
            credential_source: "named".to_owned(),
        };
        let encoded = serde_json::to_value(&event).expect("serializes");
        assert_eq!(encoded["withheld"][0]["tool"], serde_json::json!("run"));
        assert_eq!(
            serde_json::from_value::<LoopEvent>(encoded).expect("deserializes"),
            event
        );
    }

    #[test]
    fn a_run_that_was_refused_nothing_says_so_instead_of_saying_nothing() {
        // **This reverses a deliberate earlier choice, and the reason it was made is why it had to
        // go.** The field was skipped when empty so a driver reading this stream saw the same
        // bytes before and after it existed. But the driver is a separate binary observing an
        // unknown build: absence meant *nothing was withheld* or *older than the field*, and
        // nothing on the wire separates them — `b10x-harness` answers `0.1.0` either way. So a
        // driven run that withheld nothing was recorded, correctly, as one that never said, which
        // is the exact silence this field exists to break (`metaharness-b10x`'s `started` arm
        // argues the observer's half).
        //
        // Adding a key breaks no reader here: metaharness reads it with `get("withheld")`, which
        // answers `Some([])` now and answered `None` before.
        let started = LoopEvent::Started {
            model: "m".to_owned(),
            published_tools: Vec::new(),
            operations: Vec::new(),
            withheld: Vec::new(),
            skills: Vec::new(),
            agents: Vec::new(),
            profiles: Vec::new(),
            context: Vec::new(),
            toolchains: Vec::new(),
            mcp: Vec::new(),
            credential_source: "named".to_owned(),
        };
        let encoded = serde_json::to_string(&started).expect("serializes");
        assert_eq!(
            encoded,
            r#"{"kind":"started","model":"m","published_tools":[],"withheld":[],"skills":[],"agents":[],"profiles":[],"context":[],"toolchains":[],"mcp":[],"credential_source":"named"}"#,
            "a run refused nothing, offered no skill, published no agent and read no profile says \
             `[]` to each; only a build older than the field is silent"
        );

        // And a record written before the field existed still reads, as a run that withheld
        // nothing rather than as one nobody can parse.
        let old: LoopEvent =
            serde_json::from_str(r#"{"kind":"started","model":"m","published_tools":[]}"#)
                .expect("an older record deserializes");
        assert_eq!(old, started);
    }

    #[test]
    fn a_renewal_is_recorded_with_no_part_of_the_credential_in_it() {
        // **The whole safety property of this event.** It exists to say that a run rewrote
        // somebody's credential file; if saying so put any part of the credential in the record,
        // the JSONL a person forwards to explain a run would be a thing that has to be handled
        // like a secret. Not a prefix, not a length, not a digest — a digest of a token is an
        // oracle for it.
        let event = LoopEvent::CredentialRenewed(CredentialRenewal {
            source: "/home/you/.codex/auth.json".to_owned(),
            provider: "codex".to_owned(),
            expires_unix: Some(1_788_871_151),
            refresh_token_rotated: true,
            byte_preserving: true,
        });
        let encoded = serde_json::to_string(&event).expect("serializes");
        assert_eq!(
            encoded,
            r#"{"kind":"credential-renewed","source":"/home/you/.codex/auth.json","provider":"codex","expires_unix":1788871151,"refresh_token_rotated":true,"byte_preserving":true}"#
        );
        assert_eq!(
            serde_json::from_str::<LoopEvent>(&encoded).expect("deserializes"),
            event
        );
    }

    #[test]
    fn a_renewal_whose_new_token_cannot_be_dated_omits_the_expiry_rather_than_inventing_one() {
        // Absent means *this build could not read a date out of what it got*. A zero, or a
        // now-plus-a-guess, would read as a fact.
        let event = LoopEvent::CredentialRenewed(CredentialRenewal {
            source: "/home/you/.codex/auth.json".to_owned(),
            provider: "codex".to_owned(),
            expires_unix: None,
            refresh_token_rotated: false,
            byte_preserving: false,
        });
        let encoded = serde_json::to_string(&event).expect("serializes");
        assert!(!encoded.contains("expires_unix"), "{encoded}");
        assert_eq!(
            serde_json::from_str::<LoopEvent>(&encoded).expect("deserializes"),
            event
        );
    }

    #[test]
    fn events_round_trip() {
        let event = LoopEvent::ApprovalRequired {
            call_id: CallId::new("call-1").expect("valid"),
            name: ToolName::new("fs.write").expect("valid"),
        };
        let encoded = serde_json::to_value(&event).expect("serializes");
        assert_eq!(
            serde_json::from_value::<LoopEvent>(encoded).expect("deserializes"),
            event
        );
    }

    #[test]
    fn the_events_of_a_run_with_delegates_and_hooks_round_trip_with_the_nested_kind_intact() {
        // The JSONL record is a published interface — the metaharness reads it — so every shape a
        // run with sub-agents, an answer and hooks can write has to survive being read back. The
        // wrapped one is the shape that can quietly stop doing so: a `Box` inside a tagged enum
        // needs the inner event to keep its own `kind`, and a reader that lost it would see a
        // delegate's events as untagged noise.
        let call_id = CallId::new("call-1").expect("valid");
        let delegated = LoopEvent::Delegated {
            call_id: call_id.clone(),
            event: Box::new(LoopEvent::DelegateFinished {
                call_id: call_id.clone(),
                stop: LoopStop::MaxTurns { limit: 20 },
                turns: 20,
            }),
        };
        let events = vec![
            delegated.clone(),
            LoopEvent::HookRan {
                point: HookPoint::Stop,
                call_id: None,
                decision: HookDecision::block("the tests do not pass yet"),
            },
            LoopEvent::Answered {
                call_id: call_id.clone(),
                value: serde_json::json!({"verdict": "green"}),
            },
            LoopEvent::DelegateStarted {
                call_id,
                task: "survey every file under src/".to_owned(),
            },
        ];
        for event in events {
            let encoded = serde_json::to_value(&event).expect("serializes");
            assert_eq!(
                serde_json::from_value::<LoopEvent>(encoded).expect("deserializes"),
                event
            );
        }

        let encoded = serde_json::to_value(&delegated).expect("serializes");
        assert_eq!(encoded["kind"], serde_json::json!("delegated"));
        assert_eq!(
            encoded["event"]["kind"],
            serde_json::json!("delegate-finished"),
            "the wrapped event carries its own tag, or nothing can read it back: {encoded}"
        );
    }
}
