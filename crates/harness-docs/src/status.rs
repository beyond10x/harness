//! The status data, `website/data/status.json` (`b10x-status/1`), and the status page,
//! `website/docs/status.md`, both from [`CAPABILITIES`].
//!
//! A shipped capability names the test that holds it as `path::function`, which generation checks
//! the file defines as a test that runs (`#[test]` or `#[tokio::test]`, not `#[ignore]`), so a claim
//! cannot outlive its evidence. The page and the data both name that test. A capability that is
//! not shipped names none. The page is plain Markdown rather than MDX because the unified
//! documentation site, which still collects these pages, accepts no executable MDX.

use std::{collections::BTreeSet, fmt::Write as _, fs, path::Path};

use serde_json::json;

use crate::{Context, Result, TREE, VERSION, bail, cell, front_matter};

/// Where the status data lands, relative to the repository root.
pub const DATA: &str = "website/data/status.json";
/// Where the status page lands, relative to the repository root.
pub const PAGE: &str = "website/docs/status.md";

/// The date the list below was last checked against `main`.
const AS_OF: &str = "2026-10-22";

/// One capability on the status page.
pub struct Capability {
    /// The group the status table lists it under.
    pub area: &'static str,
    /// What it is, in a few words.
    pub label: &'static str,
    /// One sentence a reader can check.
    pub detail: &'static str,
    /// `shipped`, `decided` or `planned`.
    pub status: &'static str,
    /// For a shipped capability, the test that holds it: `path::function`.
    pub evidence: Option<&'static str>,
    /// The documentation route that explains it, `/docs/...`, if one does.
    pub href: Option<&'static str>,
}

const fn shipped(
    area: &'static str,
    label: &'static str,
    detail: &'static str,
    evidence: &'static str,
    href: Option<&'static str>,
) -> Capability {
    Capability {
        area,
        label,
        detail,
        status: "shipped",
        evidence: Some(evidence),
        href,
    }
}

const fn planned(area: &'static str, label: &'static str, detail: &'static str) -> Capability {
    Capability {
        area,
        label,
        detail,
        status: "planned",
        evidence: None,
        href: None,
    }
}

const WIRES: &str = "Provider wires";
const LOOP: &str = "Agent loop";
const TOOLS: &str = "Tools and approvals";
const COMMAND: &str = "Command line";
const SESSIONS: &str = "Sessions and output";
const EXTENSIONS: &str = "Delegation, skills, memories and hooks";
const BRIDGE: &str = "Bridge mode";
const SPECIFICATION: &str = "Specification";

/// Every capability, grouped by area in the order the page shows them.
pub const CAPABILITIES: &[Capability] = &[
    shipped(
        WIRES,
        "OpenAI Responses",
        "The binary reads a real file through a real tool call over the Responses wire.",
        "crates/harness-cli/tests/end_to_end.rs::the_binary_reads_a_real_file_through_a_real_tool_call",
        Some("/docs/reference/wires"),
    ),
    shipped(
        WIRES,
        "Anthropic Messages",
        "The binary drives the Messages wire and calls a real tool through it.",
        "crates/harness-cli/tests/end_to_end.rs::the_binary_drives_the_second_wire_and_calls_a_real_tool_through_it",
        Some("/docs/reference/wires"),
    ),
    shipped(
        WIRES,
        "Provider presets",
        "A short model name resolves to the exact identifier this build pins.",
        "crates/harness-cli/src/provider.rs::a_short_name_resolves_to_the_exact_identifier_this_build_pins",
        Some("/docs/reference/cli#b10x-harness-providers-show"),
    ),
    shipped(
        WIRES,
        "Credentials the operator names",
        "Naming your own credential turns the preset's token renewal off, so no file is rewritten that the operator did not offer.",
        "crates/harness-cli/src/provider.rs::naming_your_own_credential_turns_the_renewal_off",
        Some("/docs/guides/command-line#credentials"),
    ),
    shipped(
        WIRES,
        "Secrets store only on request",
        "A run or server without the secrets flag never dials the session bus.",
        "crates/harness-cli/tests/secrets_security_cli.rs::a_run_or_server_without_the_flag_never_dials_the_session_bus",
        Some("/docs/guides/command-line#credentials"),
    ),
    shipped(
        LOOP,
        "Turn ceiling",
        "A turn ceiling stops the loop, and the stop names the ceiling.",
        "crates/harness-loop/src/tests.rs::a_turn_ceiling_stops_the_loop_and_names_itself",
        Some("/docs/concepts/agent-loop"),
    ),
    shipped(
        LOOP,
        "Spend ceiling",
        "A spend ceiling on a run that cannot price its model is refused before the first request.",
        "crates/harness-loop/src/tests.rs::a_spend_ceiling_on_a_run_that_cannot_price_itself_is_refused_before_the_first_request",
        Some("/docs/concepts/agent-loop"),
    ),
    shipped(
        LOOP,
        "Unpriced is not free",
        "A model the rate card does not price has no cost rather than a zero one.",
        "crates/harness-loop/src/price.rs::a_model_the_card_does_not_price_is_absent_rather_than_free",
        None,
    ),
    shipped(
        LOOP,
        "Unreported usage",
        "Usage a provider did not report stays unknown rather than becoming zero.",
        "crates/harness-loop/src/tests.rs::unreported_usage_stays_unknown_rather_than_zero",
        None,
    ),
    shipped(
        LOOP,
        "Compaction",
        "A declared context window compacts on the token count the provider reported.",
        "crates/harness-loop/src/tests.rs::a_declared_window_compacts_on_the_count_the_provider_reported",
        Some("/docs/concepts/agent-loop"),
    ),
    shipped(
        LOOP,
        "Cancellation",
        "Cancellation between tool calls stops the run before the next effect.",
        "crates/harness-loop/src/tests.rs::cancellation_between_tool_calls_stops_before_the_next_effect",
        Some("/docs/concepts/agent-loop"),
    ),
    shipped(
        LOOP,
        "Structured output",
        "A run asked for a schema that answers in prose stops without an answer.",
        "crates/harness-cli/tests/end_to_end.rs::a_run_asked_for_a_schema_that_answers_in_prose_stops_without_an_answer",
        Some("/docs/guides/structured-runs"),
    ),
    shipped(
        TOOLS,
        "Deny by default",
        "A run entry built from the process adapter is refused under the default approver.",
        "crates/harness-catalogue/tests/run_under_deny_all.rs::run_built_from_the_adapter_is_refused_under_deny_all",
        Some("/docs/concepts/tools-and-approvals"),
    ),
    shipped(
        TOOLS,
        "No silent approval",
        "A write is refused when the run has nobody it may ask.",
        "crates/harness-cli/tests/end_to_end.rs::a_write_is_refused_when_the_run_may_not_ask_anybody",
        Some("/docs/concepts/tools-and-approvals"),
    ),
    shipped(
        TOOLS,
        "Approval ceiling",
        "Raising the unattended ceiling lets a write through, on either wire.",
        "crates/harness-cli/tests/end_to_end.rs::raising_the_ceiling_lets_a_write_through_on_the_second_wire_too",
        Some("/docs/reference/cli#b10x-harness-run"),
    ),
    shipped(
        TOOLS,
        "Write scope",
        "A path the write scope denies is refused for both writers, and the refusal says the scope is what is wrong.",
        "crates/harness-catalogue/src/scope.rs::a_denied_path_refuses_both_and_says_it_is_the_scope_that_is_wrong",
        Some("/docs/guides/profiles"),
    ),
    shipped(
        TOOLS,
        "Withheld tools are reported",
        "A declared program with no confinement is reported as withheld rather than dropped.",
        "crates/harness-cli/tests/withheld.rs::a_declared_program_with_no_confinement_at_all_is_reported_rather_than_dropped",
        Some("/docs/guides/confinement"),
    ),
    shipped(
        TOOLS,
        "Verb surface",
        "The verbs surface publishes three tools over the same catalogue.",
        "crates/harness-cli/tests/end_to_end.rs::the_verbs_surface_publishes_three_tools_over_the_same_catalogue",
        Some("/docs/concepts/tools-and-approvals"),
    ),
    shipped(
        TOOLS,
        "Outbound MCP",
        "An MCP server named in a reviewed profile runs end to end over stdio.",
        "crates/harness-cli/tests/end_to_end.rs::outbound_mcp_runs_end_to_end_over_stdio",
        Some("/docs/guides/command-line#mcp-servers"),
    ),
    shipped(
        COMMAND,
        "Toolset description",
        "The tools command describes the toolset without an endpoint.",
        "crates/harness-cli/tests/end_to_end.rs::the_tools_subcommand_describes_the_toolset_without_an_endpoint",
        Some("/docs/reference/cli#b10x-harness-tools"),
    ),
    shipped(
        COMMAND,
        "Profiles",
        "Every profile that contributed to a run is in its record with its own digest.",
        "crates/harness-cli/src/profile.rs::every_profile_that_contributed_is_in_the_record_with_its_own_digest",
        Some("/docs/guides/profiles"),
    ),
    shipped(
        COMMAND,
        "Default write scope",
        "The default write scope denies the repository's own Git directory.",
        "crates/harness-cli/src/profile.rs::the_default_write_scope_denies_git",
        Some("/docs/guides/profiles"),
    ),
    shipped(
        COMMAND,
        "Declared context files",
        "A declared context file that is absent refuses the run before any session is filed.",
        "crates/harness-cli/tests/context.rs::a_declared_context_file_that_is_absent_refuses_the_run_before_any_session",
        Some("/docs/reference/cli#b10x-harness-context-show"),
    ),
    shipped(
        SESSIONS,
        "Resume",
        "A run files its conversation, and a later run resumes it.",
        "crates/harness-cli/tests/end_to_end.rs::a_run_files_its_conversation_and_a_later_one_resumes_it",
        Some("/docs/guides/sessions-and-events"),
    ),
    shipped(
        SESSIONS,
        "Sessions stay outside the workspace",
        "A session directory inside the workspace is refused before it is created.",
        "crates/harness-cli/src/transcript.rs::a_session_directory_inside_the_workspace_is_refused_before_creation",
        Some("/docs/guides/sessions-and-events"),
    ),
    shipped(
        SESSIONS,
        "JSON events",
        "JSON mode emits one event per line.",
        "crates/harness-cli/tests/end_to_end.rs::json_mode_emits_one_event_per_line",
        Some("/docs/guides/sessions-and-events"),
    ),
    shipped(
        EXTENSIONS,
        "Delegation",
        "A delegate runs a whole second loop, and the parent reads only its report.",
        "crates/harness-loop/src/tests.rs::a_delegate_runs_a_whole_second_loop_and_the_parent_reads_only_its_report",
        None,
    ),
    shipped(
        EXTENSIONS,
        "Concurrent delegates",
        "Two delegates of one turn run at the same time.",
        "crates/harness-loop/src/tests.rs::two_delegates_of_one_turn_run_at_the_same_time",
        None,
    ),
    shipped(
        EXTENSIONS,
        "Named agents",
        "A named agent cannot widen what the parent run was admitted.",
        "crates/harness-loop/src/agent.rs::an_agent_cannot_widen_what_the_parent_was_admitted",
        Some("/docs/guides/command-line#skills-agents-and-memories"),
    ),
    shipped(
        EXTENSIONS,
        "Skills",
        "Skill descriptions are in the instruction, and their bodies are not until the model asks.",
        "crates/harness-loop/src/skill.rs::the_descriptions_are_in_the_instruction_and_the_bodies_are_not",
        Some("/docs/guides/command-line#skills-agents-and-memories"),
    ),
    shipped(
        EXTENSIONS,
        "Read-only memories",
        "recall answers a memory's body, and the record names every memory whatever its status.",
        "crates/harness-loop/src/tests.rs::a_recall_answers_the_body_and_the_record_names_every_memory_whatever_its_status",
        Some("/docs/guides/command-line#skills-agents-and-memories"),
    ),
    shipped(
        EXTENSIONS,
        "Stop hooks",
        "A stop hook blocks one ending, and the run turns again before it finishes.",
        "crates/harness-cli/tests/end_to_end.rs::a_stop_hook_blocks_one_ending_and_the_run_turns_again_before_it_finishes",
        Some("/docs/reference/cli#b10x-harness-run"),
    ),
    shipped(
        EXTENSIONS,
        "Hook points",
        "A hook file naming a point that does not exist is refused by name.",
        "crates/harness-cli/src/hooks.rs::transition_is_no_longer_a_hook_point",
        None,
    ),
    shipped(
        BRIDGE,
        "Tool calls round trip",
        "A tool call round trips back to the client that registered the tool.",
        "crates/harness-cli/tests/bridge_mode.rs::a_tool_call_round_trips_back_to_the_client",
        Some("/docs/reference/cli#b10x-harness-app-server"),
    ),
    shipped(
        BRIDGE,
        "Pinned method inventory",
        "The client methods served and refused match the pinned profile manifest.",
        "crates/harness-app-server/tests/contract.rs::the_served_and_refused_client_methods_match_the_manifest",
        None,
    ),
    planned(
        BRIDGE,
        "Driven by a real external client",
        "A real external client drives bridge mode end to end, beyond the contract tests.",
    ),
    planned(
        SPECIFICATION,
        "The loop in the executable specification",
        "The agent loop's turn, tool and budget behaviour is modelled in the executable specification and run by its conformance suite.",
    ),
];

/// Whether `source` defines a test `name` that runs: a function `name` whose attributes directly
/// above it include `#[test]` or `#[tokio::test]` and no `#[ignore]`. A line comment is not a
/// definition, and neither is a function of that name that is not a test.
fn defines(source: &str, name: &str) -> bool {
    let lines: Vec<&str> = source.lines().map(str::trim).collect();
    let signature = format!("fn {name}(");
    lines.iter().enumerate().any(|(index, line)| {
        let item = ["pub ", "async "].iter().fold(*line, |rest, prefix| {
            rest.strip_prefix(prefix).unwrap_or(rest)
        });
        if !item.starts_with(&signature) {
            return false;
        }
        let attributes = lines[..index]
            .iter()
            .rev()
            .take_while(|above| above.starts_with("#["));
        let mut test = false;
        for attribute in attributes {
            if attribute.starts_with("#[ignore") {
                return false;
            }
            test |= *attribute == "#[test]" || attribute.starts_with("#[tokio::test");
        }
        test
    })
}

/// Refuses a list with a label twice, an unknown status, a route outside `/docs/`, a shipped item
/// without evidence or with evidence that does not exist, and evidence on an item not shipped.
fn checked(root: &Path, capabilities: &[Capability]) -> Result<()> {
    let mut labels = BTreeSet::new();
    for capability in capabilities {
        let label = capability.label;
        if !labels.insert(label) {
            bail!("{label}: listed twice");
        }
        if !matches!(capability.status, "shipped" | "decided" | "planned") {
            bail!(
                "{label}: status {} is not shipped, decided or planned",
                capability.status
            );
        }
        if let Some(href) = capability.href
            && !href.starts_with("/docs/")
        {
            bail!("{label}: href {href} is not a /docs/ route");
        }
        match (capability.status, capability.evidence) {
            ("shipped", Some(evidence)) => {
                let (file, name) = evidence
                    .split_once("::")
                    .with_context(|| format!("{label}: {evidence} is not `path::function`"))?;
                let source = fs::read_to_string(root.join(file))
                    .with_context(|| format!("{label}: reading {file}"))?;
                if !defines(&source, name) {
                    bail!("{label}: {file} has no test {name}");
                }
            }
            ("shipped", None) => bail!("{label}: shipped without a test"),
            (_, Some(_)) => bail!("{label}: only a shipped item names a test"),
            (_, None) => {}
        }
    }
    Ok(())
}

/// The `b10x-status/1` document for `capabilities`, checked against the repository at `root`.
///
/// # Errors
///
/// When the list fails its check: a label twice, an unknown status, a route outside
/// `/docs/`, or a shipped item without a test that runs.
pub fn data_for(root: &Path, capabilities: &[Capability]) -> Result<String> {
    checked(root, capabilities)?;
    let items: Vec<_> = capabilities
        .iter()
        .map(|capability| {
            let mut item = json!({
                "area": capability.area,
                "label": capability.label,
                "detail": capability.detail,
                "status": capability.status,
            });
            if let Some(href) = capability.href {
                item["href"] = json!(href);
            }
            if let Some(evidence) = capability.evidence {
                item["evidence"] = json!(evidence);
            }
            item
        })
        .collect();
    let document = json!({
        "format": "b10x-status/1",
        "asOf": AS_OF,
        "source": "harness-docs, from its capability list; every shipped item names the test that holds it",
        "items": items,
    });
    Ok(serde_json::to_string_pretty(&document)? + "\n")
}

/// The `b10x-status/1` document for the repository at `root`.
///
/// # Errors
///
/// When [`CAPABILITIES`] fails its check: a label twice, an unknown status, a route outside
/// `/docs/`, or a shipped item without a test that runs.
pub fn data(root: &Path) -> Result<String> {
    data_for(root, CAPABILITIES)
}

/// `/docs/reference/cli#x` as a link from `website/docs/status.md`: `./reference/cli.md#x`.
fn link(href: &str) -> String {
    let route = href.trim_start_matches("/docs/");
    let (page, anchor) = route.split_once('#').unwrap_or((route, ""));
    let anchor = if anchor.is_empty() {
        String::new()
    } else {
        format!("#{anchor}")
    };
    format!("./{page}.md{anchor}")
}

/// The test that holds a shipped item, `file::function`, linked to its file on `main`.
fn held_by(evidence: &str) -> String {
    let file = evidence.split_once("::").map_or(evidence, |(file, _)| file);
    format!("[`{evidence}`]({TREE}/{file})")
}

/// A status as the table shows it: a glyph and the word.
fn shown(status: &str) -> &'static str {
    match status {
        "shipped" => "● shipped",
        "decided" => "◐ decided",
        _ => "○ planned",
    }
}

fn page_for(root: &Path, capabilities: &[Capability]) -> Result<String> {
    checked(root, capabilities)?;
    let count = |status: &str| {
        capabilities
            .iter()
            .filter(|capability| capability.status == status)
            .count()
    };
    let mut out = front_matter(
        "Status",
        90,
        "What Harness does today and what is planned, capability by capability.",
    );
    let _ = write!(
        out,
        "# Status\n\n{} capabilities are shipped and {} are planned, as of {AS_OF}, in workspace \
         version `{VERSION}`. **Shipped** means implemented on `main` and held by a named test in \
         the repository; `harness-docs` generates this page and `website/data/status.json` from \
         one list and fails when a test it names is gone. **Planned** means recorded on the \
         repository's plan and not built. What a green test does not show, and what Harness does \
         not do, is on [Limitations](./limitations.md).\n",
        count("shipped"),
        count("planned") + count("decided"),
    );
    let mut areas: Vec<&str> = Vec::new();
    for capability in capabilities {
        if !areas.contains(&capability.area) {
            areas.push(capability.area);
        }
    }
    for area in areas {
        let _ = write!(
            out,
            "\n## {area}\n\n| Capability | Status | What it means | Held by |\n|---|---|---|---|\n"
        );
        for capability in capabilities.iter().filter(|c| c.area == area) {
            let label = cell(capability.label);
            let label = capability
                .href
                .map_or(label.clone(), |href| format!("[{label}]({})", link(href)));
            let _ = writeln!(
                out,
                "| {label} | {} | {} | {} |",
                shown(capability.status),
                cell(capability.detail),
                capability.evidence.map_or_else(String::new, held_by),
            );
        }
    }
    Ok(out)
}

/// The status page for the repository at `root`.
///
/// # Errors
///
/// When [`CAPABILITIES`] fails its check: a label twice, an unknown status, a route outside
/// `/docs/`, or a shipped item without a test that runs.
pub fn page(root: &Path) -> Result<String> {
    page_for(root, CAPABILITIES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_capability_rests_on_a_test_that_exists() {
        let root = crate::repository_root();
        let data = data(&root).unwrap();
        assert!(data.contains("\"format\": \"b10x-status/1\""));
        let page = page(&root).unwrap();
        assert!(page.contains(
            "| [Toolset description](./reference/cli.md#b10x-harness-tools) | ● shipped |"
        ));
        assert!(page.contains("| Driven by a real external client | ○ planned |"));
        assert!(page.contains(VERSION));
    }

    #[test]
    fn evidence_is_a_test_that_runs_not_any_function_of_that_name() {
        let name = "the_claim_holds";
        assert!(defines("#[test]\nfn the_claim_holds() {}\n", name));
        assert!(defines(
            "    #[tokio::test]\n    async fn the_claim_holds() {}\n",
            name
        ));
        assert!(defines(
            "#[test]\n#[cfg_attr(miri, ignore)]\npub fn the_claim_holds() {}\n",
            name
        ));
        assert!(!defines(
            "impl B {\n    pub fn the_claim_holds(&self) {}\n}\n",
            name
        ));
        assert!(!defines("// #[test]\n// fn the_claim_holds() {}\n", name));
        assert!(!defines(
            "#[test]\n#[ignore]\nfn the_claim_holds() {}\n",
            name
        ));
        assert!(!defines(
            "#[test]\n#[ignore = \"slow\"]\nfn the_claim_holds() {}\n",
            name
        ));
        assert!(!defines("#[test]\nfn the_claim_holds_too() {}\n", name));
    }

    #[test]
    fn every_shipped_item_names_its_test_on_the_page_and_in_the_data() {
        let root = crate::repository_root();
        let data: serde_json::Value = serde_json::from_str(&data(&root).unwrap()).unwrap();
        let items = data["items"].as_array().unwrap();
        let page = page(&root).unwrap();
        assert_eq!(items.len(), CAPABILITIES.len());
        for (capability, item) in CAPABILITIES.iter().zip(items) {
            if let Some(evidence) = capability.evidence {
                assert_eq!(item["evidence"], evidence, "{}", capability.label);
                assert!(page.contains(&format!("`{evidence}`")), "{evidence}");
            } else {
                assert!(item.get("evidence").is_none(), "{}", capability.label);
            }
        }
    }

    #[test]
    fn routes_become_relative_markdown_links() {
        assert_eq!(link("/docs/reference/wires"), "./reference/wires.md");
        assert_eq!(
            link("/docs/reference/cli#b10x-harness-run"),
            "./reference/cli.md#b10x-harness-run"
        );
    }
}
