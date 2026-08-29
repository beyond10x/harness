//! Skills: `skill`, the operator's instructions loaded when the model wants them.
//!
//! # Why the body is not simply prepended
//!
//! A run given a skill up front pays for it on **every** turn: this loop is stateless and replays
//! the whole conversation, so a 6 KB `SKILL.md` is 6 KB of input on turn one and on turn forty,
//! whether or not the run ever needed it. `--context` exists for the files a run genuinely needs
//! throughout, and its doc says so; a library of skills is the other case.
//!
//! So the **descriptions** go in the standing instruction — one line each, identical every turn,
//! in the half of the request a prompt cache holds — and the **body** arrives as a tool result when
//! the model asks for it. That is what the vendor whose format this reads does, and it is why the
//! two arms of a comparison can be read against each other at all: a native run that was handed
//! every body eagerly and a vendor run that loaded one on demand are not the same experiment.
//!
//! # This crate parses nothing
//!
//! [`Skills`] is a loaded value. Walking directories and reading `SKILL.md` frontmatter is
//! `harness-cli`'s, exactly as `hooks.rs` there loads what `hook.rs` here defines. A loop that went
//! and looked at a filesystem would be a loop whose behaviour depends on something no caller
//! declared.
//!
//! # The format is a vendor's, and that is stated rather than hidden
//!
//! `SKILL.md` with `name`/`description` frontmatter is Anthropic's on-disk shape. Reading a
//! vendor's **file format** is not the same act as becoming a client of a vendor **protocol** —
//! the distinction `README.md` draws where it refuses an MCP client. Nothing here speaks to a
//! server, opens a socket, or gives a third party a say in what this run may do.

use std::fmt::Write as _;

use harness_wire::{Approval, Effect, Envelope, Idempotency, Risk, ToolName, ToolSpec};
use serde_json::json;

/// The tool name skills are published under when the caller names none.
pub const DEFAULT_SKILL_NAME: &str = "skill";

/// What the model is told the tool is for.
pub const SKILL_DESCRIPTION: &str = "Load one of this run's skills: a set of instructions the \
    operator wrote for a kind of work. The skills available to you, and what each is for, are \
    listed in your instructions — call this with a skill's `name` when the work in front of you \
    is what that skill describes, and follow what it says. Loading a skill costs one call and \
    tells you things you cannot infer from the workspace.";

/// One skill: what it is called, what it is for, and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// The name the model calls it by, from the document's own frontmatter.
    pub name: String,
    /// One line, in the standing instruction, that the model plans with.
    ///
    /// This is the whole of what a run that never loads the skill is told about it, so a
    /// description that does not say *when to use this* leaves the skill unreachable in practice.
    /// The loader does not police that — it is the skill author's sentence, passed through.
    pub description: String,
    /// The document, frontmatter stripped.
    pub body: String,
}

/// The skills a run may load, and the tool it loads them with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skills {
    /// The tool the model calls.
    pub name: ToolName,
    skills: Vec<Skill>,
}

impl Skills {
    /// The skills a caller loaded, under the default tool name.
    ///
    /// An empty set is a legitimate value and publishes nothing — see [`Skills::is_empty`]. A
    /// caller that named a directory holding no skill has a question to answer, and it is asked
    /// where the directory was read, not here.
    ///
    /// # Panics
    ///
    /// Never in practice: [`DEFAULT_SKILL_NAME`] is a constant this build checks at its only use,
    /// so the name it is built from cannot be illegal without this crate failing its own tests.
    #[must_use]
    pub fn new(skills: Vec<Skill>) -> Self {
        Self {
            name: ToolName::new(DEFAULT_SKILL_NAME)
                .expect("the default skill name is a legal tool name"),
            skills,
        }
    }

    /// The same, under a name the caller chose.
    #[must_use]
    pub fn with_name(mut self, name: ToolName) -> Self {
        self.name = name;
        self
    }

    /// `true` when there is nothing to publish.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Every skill's name, in the order they were loaded.
    ///
    /// What `session.started` reports. Always a list and never absent, even empty: a reader
    /// outside this process cannot otherwise tell *this run had no skills* from *this build does
    /// not say*, which is the distinction `Started::withheld` was fixed to keep.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.skills.iter().map(|skill| skill.name.clone()).collect()
    }

    /// The line-per-skill block the standing instruction carries.
    ///
    /// Empty string when there are none, so the caller can concatenate without a branch.
    #[must_use]
    pub fn brief(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }
        let mut brief = String::from(
            "\nSkills available to you, loaded with the `skill` tool by name. Load one when the \
             work in front of you is what it describes:\n",
        );
        for skill in &self.skills {
            let _ = writeln!(brief, "- `{}` — {}", skill.name, skill.description);
        }
        brief
    }

    /// What one skill says, or `None` for a name this run does not have.
    #[must_use]
    pub fn body(&self, name: &str) -> Option<&str> {
        self.skills
            .iter()
            .find(|skill| skill.name == name)
            .map(|skill| skill.body.as_str())
    }

    /// The tool the model sees.
    ///
    /// **`Risk::Low` with a read effect, and the effect is not decoration.** The delegate tool
    /// beside this one declares no effects because everything it does happens as a call of its
    /// own, gated separately. A skill load is not like that: it reads a file, once, here, and a
    /// reader of the envelope is entitled to see that this call touches a filesystem — even though
    /// what it reads is a document the operator handed the run before it started.
    ///
    /// Idempotent, because loading a skill twice yields the same document and changes nothing.
    #[must_use]
    pub fn spec(&self) -> ToolSpec {
        let names: Vec<&str> = self
            .skills
            .iter()
            .map(|skill| skill.name.as_str())
            .collect();
        ToolSpec {
            name: self.name.clone(),
            description: SKILL_DESCRIPTION.to_owned(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        // Enumerated rather than described, so a model cannot spend a call
                        // guessing a name: the provider refuses the wrong one before it is sent.
                        "enum": names,
                        "description": "Which skill to load."
                    }
                },
                "required": ["name"],
                "additionalProperties": false
            }),
            approval: Approval::NotRequired,
            envelope: Envelope {
                effects: vec![Effect::Read, Effect::Filesystem],
                risk: Risk::Low,
                idempotency: Idempotency::Idempotent,
                access: Vec::new(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skills() -> Skills {
        Skills::new(vec![
            Skill {
                name: "planning".to_owned(),
                description: "Plan work in a governed store.".to_owned(),
                body: "# Planning\n\nUse the CLI.\n".to_owned(),
            },
            Skill {
                name: "schema-contracts".to_owned(),
                description: "Pin a schema.".to_owned(),
                body: "# Schemas\n".to_owned(),
            },
        ])
    }

    #[test]
    fn the_descriptions_are_in_the_instruction_and_the_bodies_are_not() {
        // The whole reason this is a tool rather than a `--context` file. A stateless loop replays
        // its conversation, so a body in the instruction is paid for on every turn of every run,
        // including the runs that never needed that skill.
        let brief = skills().brief();
        assert!(brief.contains("Plan work in a governed store."), "{brief}");
        assert!(
            !brief.contains("Use the CLI."),
            "the body is what the tool call is for; putting it here would bill it every turn: \
             {brief}"
        );
    }

    #[test]
    fn a_run_with_no_skills_publishes_nothing_and_says_nothing() {
        let none = Skills::new(Vec::new());
        assert!(none.is_empty());
        assert_eq!(none.brief(), "", "no heading for an empty list");
        assert!(
            none.names().is_empty(),
            "and an empty list rather than an absence: a reader cannot otherwise tell `no skills` \
             from `this build does not say`"
        );
    }

    #[test]
    fn the_schema_enumerates_the_names_so_a_wrong_one_is_refused_before_it_is_sent() {
        // A model that has to guess spends a call finding out. The provider can refuse an `enum`
        // violation without this loop being asked at all.
        let spec = skills().spec();
        let names = &spec.input_schema["properties"]["name"]["enum"];
        assert_eq!(names, &json!(["planning", "schema-contracts"]));
    }

    #[test]
    fn a_name_this_run_does_not_have_returns_nothing_rather_than_the_wrong_body() {
        let skills = skills();
        assert_eq!(
            skills.body("planning"),
            Some("# Planning\n\nUse the CLI.\n")
        );
        assert_eq!(skills.body("Planning"), None, "names are exact");
        assert_eq!(skills.body("absent"), None);
    }
}
