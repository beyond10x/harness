//! Reading subagents off a disk, in the layout the vendor whose format this is writes them.
//!
//! `skills.rs` beside this file reads `<dir>/<name>/SKILL.md`; this reads `<dir>/<name>.md`, which
//! is the same vendor's shape for the other thing an operator writes down — a delegate with its own
//! standing instruction and its own, smaller, set of tools. It exists for the same reason: a plugin
//! written for their harness runs here unchanged, so a comparison between the two harnesses is a
//! comparison of harnesses rather than of who had to rewrite their instructions.
//!
//! # The vendor's tool names stop here
//!
//! An agent document grants `Read`, `Grep`, `Bash`. This harness has `file_read`, `search`, `run`.
//! The translation happens in this file and nowhere deeper, because `harness-loop` must be able to
//! run a delegate nobody wrote a `.md` for — a caller building an [`Agent`] in Rust names harness
//! tools, and a loop that also understood `Read` would be a loop with a vendor's vocabulary baked
//! into it. This is the vendor-format layer; the vocabulary belongs to it.
//!
//! A vendor name with no entry in that table **refuses the document by name**. It is not dropped:
//! a tool its author granted that this build quietly ignored is a permission the run would not
//! have, with nothing saying so — the agent would simply fail at the work, and the transcript would
//! show a model that did not try rather than a harness that did not offer.
//!
//! # The parser is small on purpose, and refuses rather than guessing
//!
//! As in `skills.rs`: this reads `key: value` at the top level of a frontmatter block and
//! understands nothing else — no nested maps, no block scalars, no anchors, no multi-line values.
//! `tools:` is read in its bracketed inline form and in no other, which matters more here than
//! elsewhere. A `tools:` opening a YAML block list would read to a line-based parser as a `tools:`
//! with an empty value, an empty grant reads as *no restriction*, and the delegate would quietly
//! receive the parent's whole catalogue. That is the one misreading in this file that hands out
//! power, so it is refused by name.

use std::fs;
use std::path::{Path, PathBuf};

use harness_loop::Agent;

/// The delimiter a frontmatter block opens and closes with.
const FENCE: &str = "---";

/// Where a plugin states its own name.
const MANIFEST: &str = ".claude-plugin/plugin.json";

/// What the document's file extension must be for it to be an agent.
const DOCUMENT: &str = "md";

/// The vendor's tool names, and what this harness calls the same capability.
///
/// The left column is not a suggestion an author may extend: a name absent here is refused, so
/// this table is the whole of what an agent document may grant. Adding a row is how a new tool
/// becomes grantable, and the row is the only place the two vocabularies meet.
const VENDOR_TOOLS: [(&str, &str); 7] = [
    ("Read", "file_read"),
    ("Grep", "search"),
    ("Glob", "find"),
    ("Bash", "run"),
    ("Write", "file_write"),
    ("Edit", "file_edit"),
    ("LS", "dir_list"),
];

/// What a plugin calls itself, for qualifying the agents inside it.
///
/// **An agent in a plugin is `<plugin>:<agent>`, not `<agent>`.** That is the vendor's own
/// namespacing and it is not cosmetic: two plugins may both ship a `reviewer`, and a run that
/// delegated to whichever was read first would hand the work to instructions nobody chose.
///
/// `None` where there is no manifest or it states no name: a directory handed to an `--agents-dir`
/// is not a plugin and its agents keep the names their own documents give them.
///
/// This is a copy of `skills.rs`'s function of the same name, written rather than shared because
/// that one is private and this file may not edit it. **Unify the two when either next changes** —
/// two readers of one manifest key that drift apart is a plugin whose skills and agents disagree
/// about what the plugin is called.
fn plugin_name(directory: &Path) -> Option<String> {
    let text = fs::read_to_string(directory.join(MANIFEST)).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
    manifest
        .get("name")
        .and_then(serde_json::Value::as_str)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
}

/// Every agent under a plugin directory's `agents/`, named as the plugin namespaces them.
///
/// # Errors
///
/// As [`agents_in`]. A plugin with no `agents/` is not an error — a plugin may ship only skills.
pub fn agents_in_plugin(directory: &Path) -> Result<Vec<Agent>, String> {
    let agents = directory.join("agents");
    if !agents.is_dir() {
        return Ok(Vec::new());
    }
    let qualifier = plugin_name(directory);
    let mut loaded = agents_in(&agents)?;
    if let Some(plugin) = qualifier {
        for agent in &mut loaded {
            agent.name = format!("{plugin}:{}", agent.name);
        }
    }
    Ok(loaded)
}

/// Every agent under one directory, in name order.
///
/// Name order rather than readdir order, because readdir order is a filesystem's and two machines
/// would otherwise put a run's delegates in the instruction in different orders — which changes the
/// bytes of every request and defeats a prompt cache for no reason anybody chose.
///
/// # Errors
///
/// Names the directory that cannot be read, or the first document that cannot be. A directory that
/// exists and holds no agent is **not** an error here: an empty directory is answered by the
/// caller, which knows whether the run needed a delegate.
pub fn agents_in(directory: &Path) -> Result<Vec<Agent>, String> {
    let entries = fs::read_dir(directory).map_err(|error| {
        format!(
            "reading the agents directory `{}`: {error}",
            directory.display()
        )
    })?;
    let mut documents: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "reading the agents directory `{}`: {error}",
                directory.display()
            )
        })?;
        let document = entry.path();
        if document.is_file() && document.extension().is_some_and(|kind| kind == DOCUMENT) {
            documents.push(document);
        }
    }
    documents.sort();
    documents.iter().map(|path| agent_at(path)).collect()
}

/// One agent, from its own document.
///
/// # Errors
///
/// Names the file and what is wrong with it: no frontmatter, an unterminated block, a key this
/// build does not read, a tool it cannot map, or a missing `name` or `description`.
pub fn agent_at(document: &Path) -> Result<Agent, String> {
    let text = fs::read_to_string(document)
        .map_err(|error| format!("reading `{}`: {error}", document.display()))?;
    let named = |message: String| format!("the agent document `{}`: {message}", document.display());

    let rest = text
        .strip_prefix(FENCE)
        .and_then(|rest| rest.strip_prefix('\n'))
        .ok_or_else(|| named(format!("does not open with a `{FENCE}` frontmatter fence")))?;
    let end = rest
        .find(&format!("\n{FENCE}"))
        .ok_or_else(|| named(format!("opens a `{FENCE}` fence and never closes it")))?;
    let (frontmatter, instructions) = rest.split_at(end);
    let instructions = instructions
        .strip_prefix(&format!("\n{FENCE}"))
        .unwrap_or(instructions)
        .trim_start_matches('\n');

    let mut name = None;
    let mut description = None;
    let mut tools = Vec::new();
    for line in frontmatter.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| named(format!("`{line}` is not `key: value`")))?;
        // **Refused, not ignored.** A `model:` or a `disable-model-invocation:` this build skipped
        // would be a rule the agent's author wrote and the run did not apply, and nothing would say
        // so. The caller can add a key here when it means something.
        match key.trim() {
            "name" => name = Some(value.trim().to_owned()),
            "description" => description = Some(value.trim().to_owned()),
            "tools" => tools = granted_tools(value).map_err(named)?,
            other => {
                return Err(named(format!(
                    "declares `{other}`, which this build does not read. It reads `name`, \
                     `description` and `tools`. A key skipped here is a rule its author wrote and \
                     this run would not have applied."
                )));
            }
        }
    }

    let name = name.ok_or_else(|| named("declares no `name`".to_owned()))?;
    let description = description.ok_or_else(|| {
        named(
            "declares no `description`. The description is the whole of what a run deciding \
             whether to delegate is told about this agent, so an agent without one cannot be \
             chosen."
                .to_owned(),
        )
    })?;
    if name.is_empty() {
        return Err(named("declares an empty `name`".to_owned()));
    }
    Ok(Agent {
        name,
        description,
        tools,
        instructions: instructions.to_owned(),
    })
}

/// The harness tools a `tools:` value grants, or what is wrong with the value.
///
/// The empty list is refused rather than returned. An [`Agent`] with no tools means *the parent's
/// whole catalogue*, so `tools: []` — written by an author who meant the opposite — would hand a
/// delegate everything. The two readings are as far apart as a permission gets, and neither is
/// worth guessing at.
fn granted_tools(value: &str) -> Result<Vec<String>, String> {
    let value = value.trim();
    let listed = value
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .ok_or_else(|| {
            format!(
                "declares `tools: {value}`, which is not the `[Read, Grep]` form this build reads. \
                 A `tools:` that opens a block list reads here as an empty grant, and an empty \
                 grant is no restriction at all — so it is refused rather than guessed at."
            )
        })?;
    if listed.trim().is_empty() {
        return Err(
            "grants an empty `tools: []`. An agent with no tools of its own is offered the whole \
             catalogue its parent has, which is the opposite of what an empty list looks like it \
             asks for. Name the tools, or leave `tools` out."
                .to_owned(),
        );
    }
    listed
        .split(',')
        .map(|vendor| {
            let vendor = vendor.trim();
            harness_tool(vendor).map(ToOwned::to_owned).ok_or_else(|| {
                let known: Vec<&str> = VENDOR_TOOLS.iter().map(|(name, _)| *name).collect();
                format!(
                    "grants `{vendor}`, which this build maps to no tool of its own. It maps {}. \
                     A granted tool dropped here would be a permission the agent's author wrote \
                     and this run would not have, with nothing saying so.",
                    known.join(", ")
                )
            })
        })
        .collect()
}

/// This harness's name for a vendor tool, where there is one.
fn harness_tool(vendor: &str) -> Option<&'static str> {
    VENDOR_TOOLS
        .iter()
        .find(|(name, _)| *name == vendor)
        .map(|(_, harness)| *harness)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let document = dir.join(format!("{name}.{DOCUMENT}"));
        fs::write(&document, text).expect("a document");
        document
    }

    #[test]
    fn an_agent_is_its_frontmatter_and_everything_after_the_fence() {
        let root = tempfile::tempdir().expect("a root");
        let document = write(
            root.path(),
            "decomposer",
            "---\nname: decomposer\ndescription: Split one epic into stories.\n---\n\n# Decomposer\n\nYou are given one epic.\n",
        );
        let agent = agent_at(&document).expect("reads");
        assert_eq!(agent.name, "decomposer");
        assert_eq!(agent.description, "Split one epic into stories.");
        assert_eq!(
            agent.instructions, "# Decomposer\n\nYou are given one epic.\n",
            "the fence and the frontmatter are not part of what the delegate is handed"
        );
    }

    #[test]
    fn a_description_holding_a_colon_keeps_all_of_itself() {
        // The failure this prevents: every shipped description names an id like
        // `epic:passkey-login`, and a parser splitting on the last colon — or refusing a second
        // one — would truncate the one sentence a run deciding whether to delegate ever reads.
        let root = tempfile::tempdir().expect("a root");
        let document = write(
            root.path(),
            "decomposer",
            "---\nname: decomposer\ndescription: Invoke with an id, for example `epic:passkey-login`.\n---\nbody\n",
        );
        let agent = agent_at(&document).expect("reads");
        assert_eq!(
            agent.description,
            "Invoke with an id, for example `epic:passkey-login`."
        );
    }

    #[test]
    fn the_vendors_tool_names_are_translated_to_this_harnesses_own() {
        // The failure this prevents: `Read` reaching the loop. The loop has no `Read`, so a
        // delegate granted one would be a delegate offered nothing, and the transcript would show
        // a model that did not try rather than a harness that did not offer.
        let root = tempfile::tempdir().expect("a root");
        let document = write(
            root.path(),
            "worker",
            "---\nname: worker\ndescription: d\ntools: [Read, Grep, Glob, Bash, Write, Edit, LS]\n---\nbody\n",
        );
        let agent = agent_at(&document).expect("reads");
        assert_eq!(
            agent.tools,
            vec![
                "file_read",
                "search",
                "find",
                "run",
                "file_write",
                "file_edit",
                "dir_list"
            ]
        );
    }

    #[test]
    fn an_absent_tools_key_grants_the_whole_catalogue_rather_than_nothing() {
        let root = tempfile::tempdir().expect("a root");
        let document = write(
            root.path(),
            "worker",
            "---\nname: worker\ndescription: d\n---\nbody\n",
        );
        assert!(
            agent_at(&document).expect("reads").tools.is_empty(),
            "an empty grant is the loop's word for no restriction, which is what a document \
             saying nothing about tools asks for"
        );
    }

    #[test]
    fn a_vendor_tool_this_build_cannot_map_refuses_the_document_rather_than_being_dropped() {
        // The failure this prevents: an author grants `WebFetch`, this build maps six of the seven
        // names and silently drops that one, and the run is missing a permission its author wrote
        // with nothing anywhere saying so.
        let root = tempfile::tempdir().expect("a root");
        let document = write(
            root.path(),
            "worker",
            "---\nname: worker\ndescription: d\ntools: [Read, WebFetch]\n---\nbody\n",
        );
        let error = agent_at(&document).expect_err("refused");
        assert!(error.contains("WebFetch"), "{error}");
        assert!(error.contains("no tool of its own"), "{error}");
        assert!(
            error.contains("worker"),
            "the document is named, not just the tool: {error}"
        );
    }

    #[test]
    fn an_empty_tools_list_is_refused_because_it_would_read_as_no_restriction() {
        // The one misreading in this file that hands out power. `tools: []` looks like *nothing*
        // and would arrive at the loop as *everything the parent has*.
        let root = tempfile::tempdir().expect("a root");
        let document = write(
            root.path(),
            "worker",
            "---\nname: worker\ndescription: d\ntools: []\n---\nbody\n",
        );
        let error = agent_at(&document).expect_err("refused");
        assert!(error.contains("empty"), "{error}");
    }

    #[test]
    fn a_tools_value_that_is_not_a_bracketed_list_is_refused_rather_than_half_read() {
        // A YAML block list is the dangerous case: to a line-based parser `tools:` alone is an
        // empty value, an empty grant is no restriction, and `- Read` on the next line is not
        // `key: value` at all. Refusing at the `tools:` line names the real problem.
        let root = tempfile::tempdir().expect("a root");
        let document = write(
            root.path(),
            "worker",
            "---\nname: worker\ndescription: d\ntools:\n---\nbody\n",
        );
        let error = agent_at(&document).expect_err("refused");
        assert!(error.contains("[Read, Grep]"), "{error}");
    }

    #[test]
    fn a_key_this_build_does_not_read_refuses_the_run_rather_than_being_skipped() {
        // The failure this prevents: an agent author writes `model: haiku`, this build ignores it,
        // and the run spends an epic's decomposition on a model nobody chose. A refusal names the
        // key; a skip names nothing and looks like it worked.
        let root = tempfile::tempdir().expect("a root");
        let document = write(
            root.path(),
            "worker",
            "---\nname: worker\ndescription: d\nmodel: haiku\n---\nbody\n",
        );
        let error = agent_at(&document).expect_err("refused");
        assert!(error.contains("model"), "{error}");
        assert!(error.contains("does not read"), "{error}");
    }

    #[test]
    fn a_document_with_no_description_is_refused_because_it_could_never_be_chosen() {
        let root = tempfile::tempdir().expect("a root");
        let document = write(root.path(), "worker", "---\nname: worker\n---\nbody\n");
        let error = agent_at(&document).expect_err("refused");
        assert!(error.contains("description"), "{error}");
    }

    #[test]
    fn a_document_with_no_name_is_refused_because_nothing_could_delegate_to_it() {
        let root = tempfile::tempdir().expect("a root");
        let document = write(root.path(), "worker", "---\ndescription: d\n---\nbody\n");
        let error = agent_at(&document).expect_err("refused");
        assert!(error.contains("`name`"), "{error}");
    }

    #[test]
    fn an_unterminated_fence_is_named_rather_than_read_as_a_whole_document() {
        let root = tempfile::tempdir().expect("a root");
        let document = write(root.path(), "worker", "---\nname: worker\ndescription: d\n");
        let error = agent_at(&document).expect_err("refused");
        assert!(error.contains("never closes"), "{error}");
    }

    #[test]
    fn a_directory_is_read_in_name_order_and_not_in_the_filesystems() {
        // Two machines whose readdir differs would otherwise put the delegates in the instruction
        // in different orders, changing the bytes of every request and defeating a prompt cache.
        let root = tempfile::tempdir().expect("a root");
        for name in ["zulu", "alpha", "mike"] {
            write(
                root.path(),
                name,
                &format!("---\nname: {name}\ndescription: d\n---\nbody\n"),
            );
        }
        let names: Vec<String> = agents_in(root.path())
            .expect("reads")
            .into_iter()
            .map(|agent| agent.name)
            .collect();
        assert_eq!(names, vec!["alpha", "mike", "zulu"]);
    }

    #[test]
    fn a_directory_holding_no_agent_reads_as_none_rather_than_failing() {
        let root = tempfile::tempdir().expect("a root");
        fs::write(root.path().join("README.txt"), "not an agent").expect("a file");
        fs::create_dir_all(root.path().join("a-directory")).expect("a directory");
        assert!(agents_in(root.path()).expect("reads").is_empty());
    }

    #[test]
    fn the_real_agents_this_repository_ships_against_read() {
        // The one that would have caught a parser written to a format nobody uses. Skipped where
        // the sibling checkout is not present, because this repository does not own those files.
        let shipped =
            Path::new("/home/timo/beyond10x/engineering-protocols/integrations/claude-code/agents");
        if !shipped.is_dir() {
            return;
        }
        let agents = agents_in(shipped).expect("the shipped agents read");
        let names: Vec<&str> = agents.iter().map(|agent| agent.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["decomposer", "plan-reviewer", "reverse-engineer"]
        );

        let decomposer = &agents[0];
        assert_eq!(
            decomposer.tools,
            vec!["file_read", "search", "find", "run"],
            "`tools: [Read, Grep, Glob, Bash]` is what the shipped document grants"
        );
        assert!(
            decomposer.description.contains("epic"),
            "{}",
            decomposer.description
        );
        assert!(
            decomposer.instructions.contains("# Decomposer"),
            "the body is the standing instruction"
        );
        for agent in &agents {
            // Each shipped body opens with its own heading, so anything left of that heading is
            // frontmatter that leaked. Searching the whole body for `FENCE` is the wrong check and
            // was tried: `plan-reviewer` writes a markdown table, and a table separator is
            // `|---|---|`.
            assert!(
                agent.instructions.starts_with("# "),
                "`{}` is handed something before its own heading: {:?}",
                agent.name,
                &agent.instructions[..agent.instructions.len().min(40)]
            );
            assert!(
                !agent.instructions.contains(&agent.description),
                "`{}` kept its frontmatter in what the delegate is handed",
                agent.name
            );
        }
    }
}

#[cfg(test)]
mod plugin_tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) {
        fs::create_dir_all(dir).expect("an agents directory");
        fs::write(dir.join(format!("{name}.{DOCUMENT}")), text).expect("a document");
    }

    #[test]
    fn a_plugins_agents_carry_its_name_and_a_loose_directorys_do_not() {
        // `<plugin>:<agent>` is the vendor's namespacing and not decoration: two plugins may both
        // ship a `reviewer`, and a run that delegated to whichever was read first would hand the
        // work to instructions nobody chose.
        let root = tempfile::tempdir().expect("a root");
        let plugin = root.path().join("a-plugin");
        fs::create_dir_all(plugin.join(".claude-plugin")).expect("a manifest directory");
        fs::write(
            plugin.join(MANIFEST),
            r#"{"name": "engineering-protocols", "version": "0.1.0"}"#,
        )
        .expect("a manifest");
        write(
            &plugin.join("agents"),
            "decomposer",
            "---\nname: decomposer\ndescription: d\n---\nbody\n",
        );

        let qualified = agents_in_plugin(&plugin).expect("reads");
        assert_eq!(qualified.len(), 1);
        assert_eq!(qualified[0].name, "engineering-protocols:decomposer");

        // The same tree read as a plain agents directory keeps the document's own name: a
        // directory handed to a `--agents-dir` is not a plugin and has no namespace to take.
        let loose = agents_in(&plugin.join("agents")).expect("reads");
        assert_eq!(loose[0].name, "decomposer");
    }

    #[test]
    fn a_plugin_with_no_manifest_keeps_the_documents_own_names() {
        let root = tempfile::tempdir().expect("a root");
        let plugin = root.path().join("a-plugin");
        write(
            &plugin.join("agents"),
            "decomposer",
            "---\nname: decomposer\ndescription: d\n---\nbody\n",
        );
        assert_eq!(
            agents_in_plugin(&plugin).expect("reads")[0].name,
            "decomposer"
        );
    }

    #[test]
    fn a_plugin_that_ships_only_skills_contributes_nothing_rather_than_failing() {
        let root = tempfile::tempdir().expect("a root");
        let plugin = root.path().join("a-plugin");
        fs::create_dir_all(plugin.join("skills")).expect("a skills directory");
        assert!(agents_in_plugin(&plugin).expect("reads").is_empty());
    }
}
