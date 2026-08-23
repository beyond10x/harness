//! What a tool declares about itself, and what one call actually touches.
//!
//! # Why a tool declares anything at all
//!
//! The published toolset is this harness's entire safety boundary — `README.md` says so: *"this
//! harness's effects are exactly what its toolset admits and nothing constrains it further."* While
//! every tool was read-only that boundary needed no vocabulary: nothing could go wrong. A `write`
//! and a `run` change that, and something has to be able to answer *may this call happen* without
//! reading the tool's source.
//!
//! The vocabulary is `flux-spec`'s, taken rather than invented, because it has been through the
//! argument once and the terms already mean something to everyone who works on these components.
//!
//! # A spec is a claim; the subjects are the fact
//!
//! [`ToolSpec`](crate::ToolSpec) says what a tool *can* do. [`ToolPort::subjects`](crate::ToolPort)
//! says what *this call* does. They are checked separately and the second is the one that stops
//! things: a tool that honestly declares [`Effect::Write`] and is handed a path outside the
//! workspace is refused on the **subject**, because the declaration was right and the call was not.
//!
//! Without that split, declaration is a promise nobody checks — worse than no declaration, because
//! it reads like a boundary.
//!
//! # The vocabulary invariant, taken unchanged
//!
//! From `flux-spec` (C-184): **a variant names a consequence class — what could go wrong, who sees
//! it, whether it can be undone — never an application domain.** *Runs the test suite*, *creates a
//! planning artifact* and *opens a pull request* are [`Effect::Process`], [`Effect::Write`] and
//! [`Effect::Network`] consequences of particular domains. Giving each domain a variant grows an
//! unbounded catalog on a wire enum, and flux already carries one deprecated variant that proves
//! the point.

use serde::{Deserialize, Serialize};

/// What class of thing a tool does to the host.
///
/// The resource tier, not the meaning tier: *what is touched*, never *what it was for*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// Observes without changing.
    Read,
    /// Changes something that persists after the call.
    Write,
    /// Reaches a host this process did not already hold.
    Network,
    /// Starts something that runs on its own.
    Process,
    /// Touches the filesystem, whether reading or writing it.
    Filesystem,
}

/// How much a call is allowed to cost when it is wrong.
///
/// Ordered, and the order is used: a policy says *approval above `Medium`* and means it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// Wrong is cheap and visible.
    #[default]
    Low,
    /// Wrong costs work to undo.
    Medium,
    /// Wrong costs something that is not work.
    High,
    /// Wrong cannot be undone.
    Destructive,
}

/// Whether repeating the call is safe.
///
/// This is the field a retry reads. A workflow that retreats re-runs a whole scope
/// (`b10x-harness-flow`'s `Repeat`), so every tool in that scope runs again — and one that is
/// [`Idempotency::NonIdempotent`] does something twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Idempotency {
    /// Running it twice is running it once.
    #[default]
    Idempotent,
    /// Running it twice is doing it twice.
    NonIdempotent,
    /// Depends on the arguments, so the subject is what decides.
    Conditional,
}

/// What a tool needs to reach in order to work.
///
/// Distinct from [`Effect`]: an effect is what happens, an access is what must be *available* for it
/// to. A tool with no access declared needs nothing but the process it already runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessKind {
    Filesystem,
    Process,
    Network,
    /// A credential, of any shape.
    Secret,
}

/// The concrete thing one call touches.
///
/// Scheme-prefixed on purpose: `file:crates/x/src/y.rs` and `proc:cargo` are different kinds of
/// thing and a policy that had to guess which it was looking at would guess wrong on the first path
/// that looked like a program name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Subject(String);

impl Subject {
    /// A path, relative to the workspace root.
    pub fn file(path: impl AsRef<str>) -> Self {
        Self(format!("file:{}", path.as_ref()))
    }

    /// A program a call would start.
    pub fn process(program: impl AsRef<str>) -> Self {
        Self(format!("proc:{}", program.as_ref()))
    }

    /// A host a call would reach.
    pub fn host(host: impl AsRef<str>) -> Self {
        Self(format!("host:{}", host.as_ref()))
    }

    /// The subject as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The part before the colon: `file`, `proc`, `host`.
    pub fn scheme(&self) -> &str {
        self.0.split_once(':').map_or("", |(scheme, _)| scheme)
    }

    /// The part after the colon.
    pub fn value(&self) -> &str {
        self.0.split_once(':').map_or(self.0.as_str(), |(_, rest)| rest)
    }
}

impl std::fmt::Display for Subject {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// What a tool declares about what it does.
///
/// Every field has a safe default, so a tool that says nothing is described as a pure, cheap,
/// repeatable read — and a tool that is not must say so. The alternative default, *unknown*, would
/// have to be treated as *dangerous* to be safe, and then every existing tool would need editing
/// before anything worked at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    /// What class of thing it does.
    #[serde(default)]
    pub effects: Vec<Effect>,
    /// How much a wrong call costs.
    #[serde(default)]
    pub risk: Risk,
    /// Whether running it twice is running it once.
    #[serde(default)]
    pub idempotency: Idempotency,
    /// What it must be able to reach.
    #[serde(default)]
    pub access: Vec<AccessKind>,
}

impl Default for Envelope {
    fn default() -> Self {
        Self::read_only()
    }
}

impl Envelope {
    /// A pure, cheap, repeatable read — the shape every tool this harness shipped before today has.
    pub fn read_only() -> Self {
        Self {
            effects: vec![Effect::Read],
            risk: Risk::Low,
            idempotency: Idempotency::Idempotent,
            access: Vec::new(),
        }
    }

    /// `true` when anything about this tool outlives the call.
    ///
    /// The one question every gate asks first, in one place, so three callers cannot disagree about
    /// what counts.
    pub fn mutates(&self) -> bool {
        self.effects
            .iter()
            .any(|effect| matches!(effect, Effect::Write | Effect::Process | Effect::Network))
    }

    /// Whether a person has to say yes, under a ceiling on unattended risk.
    ///
    /// **Derived, never declared.** A tool that could assert its own `Approval::NotRequired` is a
    /// tool that can opt out of the envelope, which is the one thing a safety boundary must not
    /// offer. A caller that wants everything approved passes [`Risk::Low`] as the ceiling; one that
    /// wants nothing approved passes [`Risk::Destructive`] and has said so out loud.
    pub fn needs_approval(&self, unattended_ceiling: Risk) -> bool {
        self.risk > unattended_ceiling
            || (self.idempotency == Idempotency::NonIdempotent && self.mutates())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_that_says_nothing_is_described_as_the_safest_thing_it_could_be() {
        let quiet = Envelope::default();
        assert_eq!(quiet, Envelope::read_only());
        assert!(!quiet.mutates());
        assert!(!quiet.needs_approval(Risk::Low));
    }

    #[test]
    fn risk_is_ordered_because_a_policy_says_above_medium_and_means_it() {
        assert!(Risk::Destructive > Risk::High);
        assert!(Risk::High > Risk::Medium);
        assert!(Risk::Medium > Risk::Low);
    }

    #[test]
    fn approval_is_derived_from_the_envelope_and_never_asserted_by_the_tool() {
        let writes = Envelope {
            effects: vec![Effect::Write, Effect::Filesystem],
            risk: Risk::Medium,
            idempotency: Idempotency::Idempotent,
            access: vec![AccessKind::Filesystem],
        };
        assert!(!writes.needs_approval(Risk::Medium), "at the ceiling, not above it");
        assert!(writes.needs_approval(Risk::Low));

        // The second clause: doing it twice is doing it twice, so somebody has to want it once.
        let appends = Envelope {
            idempotency: Idempotency::NonIdempotent,
            ..writes.clone()
        };
        assert!(
            appends.needs_approval(Risk::Destructive),
            "a non-idempotent mutation is asked about however high the ceiling is"
        );

        // ...and only when it actually mutates. A non-idempotent *read* is a strange thing to
        // declare and is not a reason to interrupt anybody.
        let odd_read = Envelope {
            effects: vec![Effect::Read],
            idempotency: Idempotency::NonIdempotent,
            ..Envelope::read_only()
        };
        assert!(!odd_read.needs_approval(Risk::Low));
    }

    #[test]
    fn mutating_is_one_question_asked_in_one_place() {
        for effect in [Effect::Write, Effect::Process, Effect::Network] {
            let envelope = Envelope {
                effects: vec![effect],
                ..Envelope::read_only()
            };
            assert!(envelope.mutates(), "{effect:?}");
        }
        for effect in [Effect::Read, Effect::Filesystem] {
            let envelope = Envelope {
                effects: vec![effect],
                ..Envelope::read_only()
            };
            assert!(
                !envelope.mutates(),
                "{effect:?} says what is touched, not that anything changed"
            );
        }
    }

    #[test]
    fn a_subject_carries_its_scheme_so_nothing_has_to_guess_what_it_is_looking_at() {
        let file = Subject::file("crates/x/src/y.rs");
        assert_eq!(file.scheme(), "file");
        assert_eq!(file.value(), "crates/x/src/y.rs");
        assert_eq!(file.to_string(), "file:crates/x/src/y.rs");

        // The case the prefix exists for: a program name that reads like a path.
        let program = Subject::process("target/debug/protocol");
        assert_eq!(program.scheme(), "proc");
        assert_ne!(program, Subject::file("target/debug/protocol"));
    }

    #[test]
    fn the_vocabulary_round_trips_as_the_words_a_document_would_write() {
        let envelope = Envelope {
            effects: vec![Effect::Write, Effect::Filesystem],
            risk: Risk::Destructive,
            idempotency: Idempotency::NonIdempotent,
            access: vec![AccessKind::Filesystem],
        };
        let text = serde_json::to_string(&envelope).expect("serialises");
        assert!(text.contains("\"destructive\""), "{text}");
        assert!(text.contains("\"non_idempotent\""), "{text}");
        assert_eq!(
            serde_json::from_str::<Envelope>(&text).expect("reads back"),
            envelope
        );
    }
}
