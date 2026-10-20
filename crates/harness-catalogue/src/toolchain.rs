//! Catalogue integration for declarative toolchain providers: what the catalogue publishes.
//!
//! What moved to `tool_toolchain::executor` (tools `0.4.0`) is planning and execution: harness's
//! own `plan` and `CommandPlan`, and the loop that ran a plan's argv, judged its written paths and
//! shaped its answer. What stayed is `Catalogue::invoke_toolchain`, harness's adapter and the
//! entry point of a toolchain call — it builds the executor over the active provider's
//! `run_within` ([`crate::operations::ProviderRunner`]) and this run's write scope, and turns its
//! answer and refusals into harness's — and publication, here, because an entry is harness's wire
//! envelope, risk and approval decision, and those are not the tools crate's to make.

use std::collections::BTreeMap;

use harness_wire::{AccessKind, Effect, Envelope, Idempotency, Risk};
use serde_json::{Value, json};
use tool_toolchain::{ResolvedProvider, Role};

use crate::Entry;

pub(crate) fn entries(active: &[ResolvedProvider]) -> Vec<Entry> {
    let mut entries = active
        .iter()
        .flat_map(|provider| {
            provider.tools.iter().map(|tool| Entry {
                operation: "shell",
                name: tool.name.clone(),
                summary: tool.description.clone(),
                input_schema: tool.input_schema.clone(),
                envelope: process(tool.workspace_writes),
            })
        })
        .collect::<Vec<_>>();
    for role in [Role::Check, Role::Build, Role::Test, Role::FmtCheck] {
        if active.is_empty()
            || !active
                .iter()
                .all(|provider| provider.tools.iter().any(|tool| tool.role == Some(role)))
        {
            continue;
        }
        let implementations = active
            .iter()
            .map(|provider| {
                let tool = provider
                    .tools
                    .iter()
                    .find(|tool| tool.role == Some(role))
                    .expect("completeness was checked");
                (provider.name.clone(), tool)
            })
            .collect::<Vec<_>>();
        let properties: BTreeMap<String, Value> = implementations
            .iter()
            .map(|(name, tool)| (name.clone(), tool.input_schema.clone()))
            .collect();
        let required = implementations
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        entries.push(Entry {
            operation: "shell",
            name: format!("toolchain_{}", role.name()),
            summary: format!("Run the {} role for every active toolchain.", role.name()),
            input_schema: json!({
                "type":"object",
                "properties":properties,
                "required":required,
                "additionalProperties":false,
            }),
            envelope: process(
                implementations
                    .iter()
                    .any(|(_, tool)| tool.workspace_writes),
            ),
        });
    }
    entries
}

/// Every catalogue entry contributed by selected providers, including complete generic roles.
#[must_use]
pub fn entry_names(active: &[ResolvedProvider]) -> Vec<String> {
    entries(active)
        .into_iter()
        .map(|entry| entry.name)
        .collect()
}

fn process(writes: bool) -> Envelope {
    let mut effects = vec![Effect::Process, Effect::Filesystem];
    if writes {
        effects.push(Effect::Write);
    }
    Envelope {
        effects,
        risk: Risk::High,
        idempotency: Idempotency::Conditional,
        access: vec![AccessKind::Process, AccessKind::Filesystem],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_roles_require_namespaced_provider_arguments() {
        let registry = tool_toolchain::Registry::builtins().expect("built-ins");
        let providers = registry
            .resolve(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .as_path(),
                Some(&["rust".to_owned()]),
            )
            .expect("installed providers");
        // The plan the catalogue's toolchain entries run is the executor's.
        let plan =
            tool_toolchain::executor::plan("toolchain_test", &json!({"rust":{}}), &providers)
                .expect("known")
                .expect("valid");
        assert_eq!(plan.groups.len(), 1);
        assert_eq!(plan.groups[0].provider, "rust");
    }
}
