//! Reading skills off a disk, in the layout the vendor whose format this is writes them.
//!
//! `hooks.rs` beside this file loads what `harness-loop`'s `hook.rs` defines; this loads what its
//! `skill.rs` defines, for the same reason. A loop that walked a directory would be a loop whose
//! instructions depend on something no caller declared.
//!
//! # The format is Anthropic's, and reading it is not becoming a client of theirs
//!
//! `<dir>/<name>/SKILL.md`, YAML frontmatter with `name` and `description`, body after. That is
//! their on-disk shape and this file exists to read it, so a plugin written for their harness runs
//! here unchanged and a comparison between the two is a comparison of harnesses rather than of who
//! had to rewrite their instructions. Reading that file format still grants no remote authority:
//! unlike outbound MCP, it needs no separately reviewed profile and opens no connection. Nothing
//! here opens a socket or gives anyone a say in what a run may do.
//!
//! # The parser is small on purpose, and refuses rather than guessing
//!
//! Three keys are needed and no dependency is taken for them — the workspace rule that kept
//! `hyper` out of `harness-substrate` for four routes. What that costs is stated rather than
//! hidden: this reads `key: value` at the top level of a frontmatter block and understands nothing
//! else — no nested maps, no block scalars, no anchors, no multi-line values. A document using any
//! of them is **refused by name**, never half-read. A skill silently missing half its description
//! is worse than a run that would not start.

use std::fs;
use std::path::{Path, PathBuf};

use harness_loop::Skill;

/// The delimiter a frontmatter block opens and closes with.
const FENCE: &str = "---";

/// Where a plugin states its own name.
const MANIFEST: &str = ".claude-plugin/plugin.json";

/// What a plugin calls itself, for qualifying the skills inside it.
///
/// **A skill in a plugin is `<plugin>:<skill>`, not `<skill>`.** That is the vendor's own
/// namespacing and it is not cosmetic: two plugins may both ship a `planning`, and a run that
/// loaded whichever was read first would follow instructions nobody chose. It is also what a
/// comparison's expectations name — `aep-planning:planning` — so an unqualified name
/// reads as *that skill was not offered* on an arm that offered it.
///
/// `None` where there is no manifest or it states no name: a directory handed to `--skills-dir` is
/// not a plugin and its skills keep the names their own documents give them.
fn plugin_name(directory: &Path) -> Option<String> {
    let text = fs::read_to_string(directory.join(MANIFEST)).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
    manifest
        .get("name")
        .and_then(serde_json::Value::as_str)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
}

/// Every skill under a plugin directory, named as the plugin namespaces them.
///
/// # Errors
///
/// As [`skills_in`]. A plugin with no `skills/` is not an error — a plugin may ship only agents.
pub fn skills_in_plugin(directory: &Path) -> Result<Vec<Skill>, String> {
    let skills = directory.join("skills");
    if !skills.is_dir() {
        return Ok(Vec::new());
    }
    let qualifier = plugin_name(directory);
    let mut loaded = skills_in(&skills)?;
    if let Some(plugin) = qualifier {
        for skill in &mut loaded {
            skill.name = format!("{plugin}:{}", skill.name);
        }
    }
    Ok(loaded)
}

/// Every skill under one directory, in name order.
///
/// Name order rather than readdir order, because readdir order is a filesystem's and two machines
/// would otherwise put a run's skills in the instruction in different orders — which changes the
/// bytes of every request and defeats a prompt cache for no reason anybody chose.
///
/// # Errors
///
/// Names the directory that cannot be read, or the first document that cannot be. A directory that
/// exists and holds no skill is **not** an error here: `--skills-dir` pointing somewhere empty is
/// answered by the caller, which knows whether the run needed one.
pub fn skills_in(directory: &Path) -> Result<Vec<Skill>, String> {
    let entries = fs::read_dir(directory).map_err(|error| {
        format!(
            "reading the skills directory `{}`: {error}",
            directory.display()
        )
    })?;
    let mut documents: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "reading the skills directory `{}`: {error}",
                directory.display()
            )
        })?;
        let document = entry.path().join("SKILL.md");
        if document.is_file() {
            documents.push(document);
        }
    }
    documents.sort();
    documents.iter().map(|path| skill_at(path)).collect()
}

/// One skill, from its own document.
///
/// # Errors
///
/// Names the file and what is wrong with it: no frontmatter, an unterminated block, a key this
/// build does not read, or a missing `name` or `description`.
pub fn skill_at(document: &Path) -> Result<Skill, String> {
    let bytes = fs::metadata(document)
        .map_err(|error| format!("reading metadata for `{}`: {error}", document.display()))?
        .len();
    if bytes > harness_wire::MAX_TOOL_RESULT_BYTES as u64 {
        return Err(format!(
            "the skill document `{}` is {bytes} bytes, over the {} byte result bound; it was refused before being read",
            document.display(),
            harness_wire::MAX_TOOL_RESULT_BYTES
        ));
    }
    let text = fs::read_to_string(document)
        .map_err(|error| format!("reading `{}`: {error}", document.display()))?;
    let named = |message: String| format!("the skill document `{}`: {message}", document.display());

    let rest = text
        .strip_prefix(FENCE)
        .and_then(|rest| rest.strip_prefix('\n'))
        .ok_or_else(|| named(format!("does not open with a `{FENCE}` frontmatter fence")))?;
    let end = rest
        .find(&format!("\n{FENCE}"))
        .ok_or_else(|| named(format!("opens a `{FENCE}` fence and never closes it")))?;
    let (frontmatter, body) = rest.split_at(end);
    let body = body
        .strip_prefix(&format!("\n{FENCE}"))
        .unwrap_or(body)
        .trim_start_matches('\n');

    let mut name = None;
    let mut description = None;
    for line in frontmatter.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| named(format!("`{line}` is not `key: value`")))?;
        // **Refused, not ignored.** A `tools:` or an `allowed-directories:` this build skipped
        // would be a constraint the skill's author wrote and the run did not apply, and nothing
        // would say so. The caller can add a key here when it means something.
        match key.trim() {
            "name" => name = Some(value.trim().to_owned()),
            "description" => description = Some(value.trim().to_owned()),
            other => {
                return Err(named(format!(
                    "declares `{other}`, which this build does not read. It reads `name` and \
                     `description`. A key skipped here is a rule its author wrote and this run \
                     would not have applied."
                )));
            }
        }
    }

    let name = name.ok_or_else(|| named("declares no `name`".to_owned()))?;
    let description = description.ok_or_else(|| {
        named(
            "declares no `description`. The description is the whole of what a run that never \
             loads this skill is told about it, so a skill without one cannot be chosen."
                .to_owned(),
        )
    })?;
    if name.is_empty() {
        return Err(named("declares an empty `name`".to_owned()));
    }
    Ok(Skill {
        name,
        description,
        body: body.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let skill = dir.join(name);
        fs::create_dir_all(&skill).expect("a skill directory");
        let document = skill.join("SKILL.md");
        fs::write(&document, text).expect("a document");
        document
    }

    #[test]
    fn a_skill_is_its_frontmatter_and_everything_after_the_fence() {
        let root = tempfile::tempdir().expect("a root");
        let document = write(
            root.path(),
            "planning",
            "---\nname: planning\ndescription: Plan work in a governed store.\n---\n\n# Planning\n\nUse the CLI.\n",
        );
        let skill = skill_at(&document).expect("reads");
        assert_eq!(skill.name, "planning");
        assert_eq!(skill.description, "Plan work in a governed store.");
        assert_eq!(
            skill.body, "# Planning\n\nUse the CLI.\n",
            "the fence and the frontmatter are not part of what the model is handed"
        );
    }

    #[test]
    fn a_key_this_build_does_not_read_refuses_the_run_rather_than_being_skipped() {
        // The failure this prevents: a skill author writes `allowed-tools:`, this build ignores
        // it, and the run applies a constraint nobody applied. A refusal names the key; a skip
        // names nothing and looks like it worked.
        let root = tempfile::tempdir().expect("a root");
        let document = write(
            root.path(),
            "planning",
            "---\nname: planning\ndescription: d\nallowed-tools: [Read]\n---\nbody\n",
        );
        let error = skill_at(&document).expect_err("refused");
        assert!(error.contains("allowed-tools"), "{error}");
        assert!(error.contains("does not read"), "{error}");
    }

    #[test]
    fn a_document_with_no_description_is_refused_because_it_could_never_be_chosen() {
        let root = tempfile::tempdir().expect("a root");
        let document = write(root.path(), "planning", "---\nname: planning\n---\nbody\n");
        let error = skill_at(&document).expect_err("refused");
        assert!(error.contains("description"), "{error}");
    }

    #[test]
    fn an_unterminated_fence_is_named_rather_than_read_as_a_whole_document() {
        let root = tempfile::tempdir().expect("a root");
        let document = write(
            root.path(),
            "planning",
            "---\nname: planning\ndescription: d\n",
        );
        let error = skill_at(&document).expect_err("refused");
        assert!(error.contains("never closes"), "{error}");
    }

    #[test]
    fn an_oversized_skill_is_refused_before_its_body_is_loaded() {
        let root = tempfile::tempdir().expect("a root");
        let document = write(
            root.path(),
            "large",
            &format!(
                "---\nname: large\ndescription: d\n---\n{}",
                "x".repeat(harness_wire::MAX_TOOL_RESULT_BYTES)
            ),
        );
        let error = skill_at(&document).expect_err("oversized refuses");
        assert!(error.contains("result bound"), "{error}");
        assert!(error.contains("before being read"), "{error}");
    }

    #[test]
    fn the_skill_byte_bound_accepts_both_edges_and_whole_multibyte_text() {
        let root = tempfile::tempdir().expect("a root");
        let prefix = "---\nname: bounded\ndescription: d\n---\n";
        for total in [
            harness_wire::MAX_TOOL_RESULT_BYTES - 1,
            harness_wire::MAX_TOOL_RESULT_BYTES,
        ] {
            let text = format!("{prefix}{}", "x".repeat(total - prefix.len()));
            assert_eq!(text.len(), total);
            let document = write(root.path(), &format!("ascii-{total}"), &text);
            let skill = skill_at(&document).expect("the bound is inclusive");
            assert_eq!(skill.body.len(), total - prefix.len());
        }

        let body_bytes = harness_wire::MAX_TOOL_RESULT_BYTES - prefix.len();
        let text = format!("{prefix}{}é", "x".repeat(body_bytes - 'é'.len_utf8()));
        assert_eq!(text.len(), harness_wire::MAX_TOOL_RESULT_BYTES);
        let document = write(root.path(), "multibyte", &text);
        let skill = skill_at(&document).expect("a whole multibyte character at the bound");
        assert!(skill.body.ends_with('é'));
        assert!(!skill.body.contains('\u{fffd}'));
    }

    #[test]
    fn a_directory_is_read_in_name_order_and_not_in_the_filesystems() {
        // Two machines whose readdir differs would otherwise put the skills in the instruction in
        // different orders, changing the bytes of every request and defeating a prompt cache.
        let root = tempfile::tempdir().expect("a root");
        for name in ["zulu", "alpha", "mike"] {
            write(
                root.path(),
                name,
                &format!("---\nname: {name}\ndescription: d\n---\nbody\n"),
            );
        }
        let names: Vec<String> = skills_in(root.path())
            .expect("reads")
            .into_iter()
            .map(|skill| skill.name)
            .collect();
        assert_eq!(names, vec!["alpha", "mike", "zulu"]);
    }

    #[test]
    fn a_directory_holding_no_skill_reads_as_none_rather_than_failing() {
        let root = tempfile::tempdir().expect("a root");
        fs::create_dir_all(root.path().join("not-a-skill")).expect("a directory");
        assert!(skills_in(root.path()).expect("reads").is_empty());
    }

    #[test]
    #[ignore = "requires the sibling agentplugins checkout; exercised by upstream-agentplugins.yml"]
    fn the_real_plugin_this_repository_ships_against_reads() {
        // The one that catches a parser written to a format nobody uses. The dedicated upstream
        // workflow checks out the independently released marketplace beside this repository.
        // Relative to this crate, never an absolute path from whoever wrote the test: an
        // absolute one is a personal directory published in a public repository, and it makes the
        // test pass on exactly one machine.
        let planning = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../agentplugins/plugins/aep-planning/skills/planning/SKILL.md");
        let planning = planning.as_path();
        assert!(planning.is_file(), "missing {}", planning.display());
        let skill = skill_at(planning).expect("the shipped skill reads");
        assert_eq!(skill.name, "planning");
        assert!(
            skill.description.contains("planning"),
            "{}",
            skill.description
        );
        assert!(
            skill.body.contains("# Planning"),
            "the body is the document"
        );
    }
}

#[cfg(test)]
mod plugin_tests {
    use super::*;

    #[test]
    fn a_plugins_skills_carry_its_name_and_a_loose_directorys_do_not() {
        // `<plugin>:<skill>` is the vendor's namespacing and not decoration: two plugins may both
        // ship a `planning`, and an unqualified name also reads as *not offered* to an expectation
        // naming `aep-planning:planning`, which is how a run that offered the skill got
        // scored as one that did not.
        let root = tempfile::tempdir().expect("a root");
        let plugin = root.path().join("a-plugin");
        fs::create_dir_all(plugin.join(".claude-plugin")).expect("a manifest directory");
        fs::write(
            plugin.join(MANIFEST),
            r#"{"name": "aep-planning", "version": "0.1.0"}"#,
        )
        .expect("a manifest");
        let skill = plugin.join("skills").join("planning");
        fs::create_dir_all(&skill).expect("a skill directory");
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: planning\ndescription: d\n---\nbody\n",
        )
        .expect("a document");

        let qualified = skills_in_plugin(&plugin).expect("reads");
        assert_eq!(qualified.len(), 1);
        assert_eq!(qualified[0].name, "aep-planning:planning");

        // The same tree read as a plain skills directory keeps the document's own name: a
        // directory handed to `--skills-dir` is not a plugin and has no namespace to take.
        let loose = skills_in(&plugin.join("skills")).expect("reads");
        assert_eq!(loose[0].name, "planning");
    }

    #[test]
    fn a_plugin_with_no_manifest_keeps_the_documents_own_names() {
        let root = tempfile::tempdir().expect("a root");
        let plugin = root.path().join("a-plugin");
        let skill = plugin.join("skills").join("planning");
        fs::create_dir_all(&skill).expect("a skill directory");
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: planning\ndescription: d\n---\nbody\n",
        )
        .expect("a document");
        assert_eq!(
            skills_in_plugin(&plugin).expect("reads")[0].name,
            "planning"
        );
    }

    #[test]
    fn a_plugin_that_ships_only_agents_contributes_nothing_rather_than_failing() {
        let root = tempfile::tempdir().expect("a root");
        let plugin = root.path().join("a-plugin");
        fs::create_dir_all(plugin.join("agents")).expect("an agents directory");
        assert!(skills_in_plugin(&plugin).expect("reads").is_empty());
    }
}
