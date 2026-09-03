//! Named agents: a delegate the operator described in advance, on a **narrower** gate.
//!
//! `delegate` beside this hands a fresh context the parent's whole catalogue and a task written on
//! the spot. A named agent is the other half: a standing description the operator wrote before the
//! run — what this agent is for, what it may touch, and the instruction it works under — that the
//! model picks by name. The value of naming one is that the model chooses between *reviewer* and
//! *researcher* on a line it can read every turn, instead of the run's author re-explaining the
//! same sub-task in every task string.
//!
//! # An agent narrows; it can never widen
//!
//! [`crate::Delegation`]'s doc states the property this file must not break: *"Delegation widens
//! nothing: the child can do exactly what the parent's catalogue admits, entry for entry."*
//! [`Agent::admitted`] therefore returns an **intersection** with the parent's list, never a union,
//! and no other function here hands out a tool name at all.
//!
//! That is not defensive tidiness — it is the difference between a document and a grant. An agent
//! file is markdown, and markdown arrives from places nobody audits: a plugin directory, a
//! checked-out dependency, a repository a colleague opened. If `tools: [run]` in frontmatter could
//! *add* `run`, then writing a file into a workspace would be enough to execute commands on the
//! operator's machine — on a run whose catalogue deliberately did not publish `run`, and past the
//! approval gate, because the gate is only ever asked about entries that exist. So a declared name
//! the parent does not have buys exactly nothing, and the test that proves it is the hostile one.
//!
//! # An agent that declares no tools is unrestricted, not disarmed
//!
//! The on-disk format's `tools:` key is optional, and a file without one has declared **no
//! restriction** — it has not declared *no tools*. Reading the absent key as an empty allowance
//! would silently disarm every agent file written without one, and the failure is quiet: the agent
//! runs, does nothing, and reports that it could not find anything. That is invariant 7, *preserve
//! absence as absence*, applied to a field rather than to usage. Empty [`Agent::tools`] therefore
//! means the parent's whole catalogue, and the caller that read the file is the one that must not
//! invent an empty list where the document was silent.
//!
//! # What the agent asked for and did not get is written down
//!
//! A name the parent lacks is refused, but it is refused **out loud**, as a [`Withheld`] entry
//! carrying the tool and a sentence saying why. Dropping it silently would leave a reader unable
//! to tell *this agent never wanted `run`* from *this agent asked for `run` and this machine would
//! not admit it* — the same distinction `Started::withheld` exists to keep, and the more useful
//! one of the two, because it is the one that explains why an agent underperformed its
//! description.
//!
//! # This crate parses nothing, and knows no vendor's tool names
//!
//! [`Agents`] is a loaded value, exactly as [`crate::Skills`] is. Walking `agents/*.md`, reading
//! YAML frontmatter, and mapping the vendor's verbs — `Read`, `Grep`, `Glob`, `Bash` — onto this
//! harness's catalogue entries are all `harness-cli`'s work. By the time an [`Agent`] exists here
//! its `tools` are **already harness-native names**, comparable to the parent's list by string
//! equality; if they were not, an intersection would silently admit nothing and this file's whole
//! guarantee would degrade into an unexplained empty catalogue.
//!
//! Design: `docs/design/0002-sub-agents-structured-output-hooks.md` § 2.

use std::fmt::Write as _;

use crate::Withheld;
use serde::{Deserialize, Serialize};

/// One agent: what it is called, what it is for, what it may touch, and what it is told.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    /// The name the model picks it by, from the document's own frontmatter.
    pub name: String,
    /// One line, in the standing instruction, that the model chooses on.
    ///
    /// This is the whole of what the model knows about the agent when it decides whether to use
    /// it, so a description that does not say *when to send work here* leaves the agent
    /// unreachable in practice. Passed through as the author wrote it; nothing here polices it.
    pub description: String,
    /// Harness tool names this agent may use. EMPTY MEANS the parent's whole catalogue,
    /// because an agent file with no `tools:` key declares no restriction.
    pub tools: Vec<String>,
    /// The agent's body, used as its standing instruction.
    pub instructions: String,
}

/// The agents a run may hand work to.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agents {
    agents: Vec<Agent>,
}

impl Agents {
    /// The agents a caller loaded, in the order they were loaded.
    ///
    /// An empty set is a legitimate value and publishes nothing — see [`Agents::is_empty`]. A
    /// caller that pointed at a directory holding no agent has a question to answer, and it is
    /// asked where the directory was read, not here.
    #[must_use]
    pub fn new(agents: Vec<Agent>) -> Self {
        Self { agents }
    }

    /// `true` when there is nothing to publish.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Every agent's name, in the order they were loaded.
    ///
    /// What `session.started` reports. Always a list and never absent, even empty: a reader
    /// outside this process cannot otherwise tell *this run had no agents* from *this build does
    /// not say*, which is the distinction `Started::withheld` was fixed to keep.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.agents.iter().map(|agent| agent.name.clone()).collect()
    }

    /// One agent by name, or `None` for a name this run does not have.
    ///
    /// Exact match. A near miss resolving to the wrong agent would run somebody else's
    /// instructions under somebody else's tool allowance, which is a worse outcome than a refusal
    /// the model can read and correct.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Agent> {
        self.agents.iter().find(|agent| agent.name == name)
    }

    /// The line-per-agent block the standing instruction carries.
    ///
    /// Empty string when there are none, so the caller can concatenate without a branch.
    ///
    /// Descriptions only, never instructions: an agent's body is what its own run is given, and
    /// putting it here would bill every agent's full prompt on every turn of a stateless loop —
    /// the same arithmetic that keeps a skill's body out of [`crate::Skills::brief`].
    #[must_use]
    pub fn brief(&self) -> String {
        if self.agents.is_empty() {
            return String::new();
        }
        let mut brief = String::from(
            "\nAgents you can hand a self-contained task to, by name. Each is a fresh context \
             with its own instructions and its own share of your tools — send work to the one \
             whose description matches it:\n",
        );
        for agent in &self.agents {
            let _ = writeln!(brief, "- `{}` — {}", agent.name, agent.description);
        }
        brief
    }
}

impl Agent {
    /// Which of the parent's tools this agent may use, and what it asked for and did not get.
    ///
    /// Returns (admitted, withheld).
    ///
    /// The admitted list is `parent ∩ self.tools` **in the parent's order**, so two runs of the
    /// same agent against the same catalogue publish byte-identical lists and a reader diffing two
    /// session records sees a difference only where there was one. An empty [`Agent::tools`] is
    /// the whole of `parent`, unchanged and in place.
    ///
    /// Every declared name `parent` does not carry comes back as a [`Withheld`] rather than
    /// disappearing, because *asked and refused* and *never asked* are different facts about a run
    /// and only the first one explains an agent that did less than its description promised.
    #[must_use]
    pub fn admitted(&self, parent: &[String]) -> (Vec<String>, Vec<Withheld>) {
        if self.tools.is_empty() {
            return (parent.to_vec(), Vec::new());
        }
        let admitted: Vec<String> = parent
            .iter()
            .filter(|tool| self.tools.iter().any(|declared| declared == *tool))
            .cloned()
            .collect();
        let withheld: Vec<Withheld> = self
            .tools
            .iter()
            .filter(|declared| !parent.iter().any(|tool| tool == *declared))
            .map(|declared| Withheld {
                tool: declared.clone(),
                reason: format!(
                    "the `{}` agent declares `{declared}`, and this run's catalogue does not admit \
                     it; an agent narrows the tools it was given and never adds one",
                    self.name
                ),
            })
            .collect();
        (admitted, withheld)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parent() -> Vec<String> {
        vec![
            "read_file".to_owned(),
            "search".to_owned(),
            "list_files".to_owned(),
        ]
    }

    fn agent(name: &str, tools: &[&str]) -> Agent {
        Agent {
            name: name.to_owned(),
            description: format!("What {name} is for."),
            tools: tools.iter().map(|tool| (*tool).to_owned()).collect(),
            instructions: format!("# {name}\n\nDo the thing.\n"),
        }
    }

    fn agents() -> Agents {
        Agents::new(vec![
            agent("reviewer", &["read_file", "search"]),
            agent("researcher", &[]),
        ])
    }

    #[test]
    fn an_agent_cannot_widen_what_the_parent_was_admitted() {
        // The property `delegate.rs` states and this file must not break: a child does exactly
        // what the parent's catalogue admits, entry for entry. A union here would let a markdown
        // file in a workspace grant a tool the run deliberately did not publish.
        let asks_for_more = agent("greedy", &["read_file", "run", "file_write"]);
        let (admitted, _) = asks_for_more.admitted(&parent());
        assert_eq!(
            admitted,
            vec!["read_file".to_owned()],
            "the intersection, never the union"
        );
    }

    #[test]
    fn a_tool_the_agent_asked_for_and_did_not_get_is_recorded_by_name() {
        // Prevents the silent drop. Without this a reader cannot tell `this agent never wanted
        // run` from `this agent asked for run and the catalogue refused it`, and only the second
        // explains an agent that did less than its description promised.
        let asks_for_more = agent("greedy", &["read_file", "run", "file_write"]);
        let (_, withheld) = asks_for_more.admitted(&parent());
        let names: Vec<&str> = withheld.iter().map(|entry| entry.tool.as_str()).collect();
        assert_eq!(names, vec!["run", "file_write"]);
        let first = withheld
            .first()
            .expect("`run` was refused, so there is an entry");
        assert!(
            first.reason.contains("greedy") && first.reason.contains("`run`"),
            "the record has to say who asked and for what: {}",
            first.reason
        );
        assert!(
            first.reason.contains("does not admit"),
            "and why it was refused: {}",
            first.reason
        );
    }

    #[test]
    fn an_agent_that_declares_no_tools_gets_the_parents_whole_catalogue() {
        // A file with no `tools:` key declared no restriction — it did not declare no tools.
        // Reading the absent key as an empty allowance would disarm every agent written without
        // one, and quietly: the agent runs, finds nothing, and reports that it found nothing.
        let unrestricted = agent("researcher", &[]);
        let (admitted, withheld) = unrestricted.admitted(&parent());
        assert_eq!(admitted, parent(), "the whole catalogue, unchanged");
        assert!(
            withheld.is_empty(),
            "an agent that asked for nothing in particular was refused nothing: {withheld:?}"
        );
    }

    #[test]
    fn the_admitted_list_is_in_the_parents_order_so_two_runs_publish_the_same_list() {
        // The declaration order is the agent author's and varies between files that mean the same
        // thing. Ordering by the parent instead makes the published list a function of the
        // catalogue alone, so a reader diffing two session records sees a difference only where
        // there was one.
        let one = agent("a", &["list_files", "read_file"]);
        let other = agent("b", &["read_file", "list_files"]);
        let expected = vec!["read_file".to_owned(), "list_files".to_owned()];
        assert_eq!(one.admitted(&parent()).0, expected);
        assert_eq!(other.admitted(&parent()).0, expected);
    }

    #[test]
    fn declaring_a_tool_the_parent_lacks_does_not_grant_it() {
        // The hostile case. An agent file arrives from a plugin directory nobody audits; if
        // naming `run` were enough to get it, writing a file into a workspace would execute
        // commands on the operator's machine, on a run that deliberately published no `run` and
        // past a gate that is only ever asked about entries which exist.
        let hostile = agent("hostile", &["run"]);
        let catalogue = vec!["read_file".to_owned(), "search".to_owned()];
        let (admitted, withheld) = hostile.admitted(&catalogue);
        assert!(
            admitted.is_empty(),
            "declaring a tool is not acquiring one: {admitted:?}"
        );
        assert_eq!(withheld.len(), 1);
        assert_eq!(
            withheld
                .first()
                .expect("one name was refused, so there is one entry")
                .tool,
            "run"
        );
    }

    #[test]
    fn a_run_with_no_agents_publishes_nothing_and_says_nothing() {
        let none = Agents::new(Vec::new());
        assert!(none.is_empty());
        assert_eq!(none.brief(), "", "no heading for an empty list");
        assert!(
            none.names().is_empty(),
            "and an empty list rather than an absence: a reader cannot otherwise tell `no agents` \
             from `this build does not say`"
        );
    }

    #[test]
    fn the_descriptions_are_in_the_instruction_and_the_instructions_are_not() {
        // A stateless loop replays its conversation, so an agent's body in the standing
        // instruction is paid for on every turn of every run, including the runs that never send
        // that agent anything. The body is what the agent's own run is given.
        let brief = agents().brief();
        assert!(brief.contains("What reviewer is for."), "{brief}");
        assert!(brief.contains("What researcher is for."), "{brief}");
        assert!(
            !brief.contains("Do the thing."),
            "the instruction belongs to the agent's own run; here it is billed every turn: {brief}"
        );
    }

    #[test]
    fn a_name_this_run_does_not_have_returns_nothing_rather_than_the_wrong_agent() {
        // A near miss would run somebody else's instructions under somebody else's tool
        // allowance, which is worse than a refusal the model can read and correct.
        let agents = agents();
        assert_eq!(agents.names(), vec!["reviewer", "researcher"]);
        assert_eq!(
            agents.get("reviewer").map(|agent| agent.tools.as_slice()),
            Some(["read_file".to_owned(), "search".to_owned()].as_slice())
        );
        assert_eq!(agents.get("Reviewer"), None, "names are exact");
        assert_eq!(agents.get("absent"), None);
    }
}
