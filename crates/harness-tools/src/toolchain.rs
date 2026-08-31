//! Catalogue integration for declarative toolchain providers.

use std::collections::BTreeMap;

use harness_toolchain::{ResolvedProvider, Role};
use harness_wire::{AccessKind, Effect, Envelope, Idempotency, Risk};
use serde_json::{Value, json};

use crate::Entry;

pub(crate) struct CommandPlan {
    pub groups: Vec<(String, harness_toolchain::CommandPlan)>,
}

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

pub(crate) fn programs(active: &[ResolvedProvider]) -> Vec<String> {
    let mut programs = active
        .iter()
        .flat_map(|provider| provider.programs.iter().cloned())
        .collect::<Vec<_>>();
    programs.sort();
    programs.dedup();
    programs
}

pub(crate) fn plan(
    name: &str,
    arguments: &Value,
    active: &[ResolvedProvider],
) -> Option<Result<CommandPlan, String>> {
    if let Some(role_name) = name.strip_prefix("toolchain_") {
        let role = [Role::Check, Role::Build, Role::Test, Role::FmtCheck]
            .into_iter()
            .find(|role| role.name() == role_name)?;
        let result = active
            .iter()
            .map(|provider| {
                let tool = provider
                    .tools
                    .iter()
                    .find(|tool| tool.role == Some(role))
                    .ok_or_else(|| {
                        format!(
                            "provider `{}` has no `{}` role implementation",
                            provider.name,
                            role.name()
                        )
                    })?;
                let provider_arguments = arguments.get(&provider.name).ok_or_else(|| {
                    format!("`{}` arguments are required by `{name}`", provider.name)
                })?;
                tool.plan(provider_arguments)
                    .map(|plan| (provider.name.clone(), plan))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|groups| CommandPlan { groups });
        return Some(result);
    }
    for provider in active {
        if let Some(plan) = provider.plan(name, arguments) {
            return Some(plan.map(|plan| CommandPlan {
                groups: vec![(provider.name.clone(), plan)],
            }));
        }
    }
    None
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
        let registry = harness_toolchain::Registry::builtins().expect("built-ins");
        let providers = registry
            .resolve(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .as_path(),
                Some(&["rust".to_owned()]),
            )
            .expect("installed providers");
        let plan = plan("toolchain_test", &json!({"rust":{}}), &providers)
            .expect("known")
            .expect("valid");
        assert_eq!(plan.groups.len(), 1);
    }
}
