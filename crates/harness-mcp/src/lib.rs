#![forbid(unsafe_code)]
//! A reviewed MCP snapshot projected through Harness's existing tool boundary.

use std::path::Path;
use std::time::Duration;

use b10x_mcp_command::connect_named;
use b10x_mcp_config::{LocalPaths, Registry};
use b10x_mcp_types::{ConnectionId, Limits, ToolCall as McpCall};
use harness_wire::{
    Approval, Envelope, Subject, ToolCall, ToolName, ToolOutcome, ToolPort, ToolSpec,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// Body-free provenance carried in a run record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Evidence {
    /// Client-owned connection name.
    pub connection: String,
    /// Digest of the whole local registry used to resolve it.
    pub registry_sha256: String,
    /// Digest of the reviewed profile granting publication.
    pub profile_sha256: String,
    /// Digest of the exact tools/list snapshot frozen for the run.
    pub snapshot_sha256: String,
    /// Negotiated MCP revision.
    pub protocol_version: String,
}

/// One reviewed connection and an explicit subset of its discovered tools.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Profile {
    /// Name in the shared MCP registry.
    pub connection: String,
    /// Registry digest reviewed with this profile.
    pub registry_sha256: String,
    /// tools/list snapshot digest reviewed with this profile.
    pub snapshot_sha256: String,
    /// Explicit tool grants. A discovered tool absent here is not published.
    pub tools: Vec<ToolPolicy>,
}

/// Harness-owned authority for one remote tool.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ToolPolicy {
    /// Original MCP tool name.
    pub remote: String,
    /// Provider-safe name published to the model.
    pub publish: String,
    /// Optional locally reviewed prose replacing the server description.
    pub description: Option<String>,
    /// Local effect/risk/idempotency/access claim. Server annotations are never read into it.
    pub envelope: Envelope,
    /// Concrete static subjects the policy says every call touches.
    pub subjects: Vec<PolicySubject>,
}

/// One locally authored subject.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PolicySubject {
    /// A workspace-relative file or subtree label.
    File { value: String },
    /// A process label.
    Process { value: String },
    /// A network host label.
    Host { value: String },
}

impl PolicySubject {
    fn subject(&self) -> Subject {
        match self {
            Self::File { value } => Subject::file(value),
            Self::Process { value } => Subject::process(value),
            Self::Host { value } => Subject::host(value),
        }
    }
}

/// A prepared, frozen connection. Calls still pass through the loop's ordinary approval and hooks.
pub struct McpTools {
    runtime: tokio::runtime::Runtime,
    connection: b10x_mcp_client::Connection,
    specs: Vec<ToolSpec>,
    policies: Vec<ToolPolicy>,
    evidence: Evidence,
}

impl McpTools {
    /// Read the strict registry and policy, connect once, and verify both reviewed digests.
    ///
    /// # Errors
    ///
    /// Refuses malformed policy, changed registry or discovery bytes, unpublishable names,
    /// duplicate grants, and any connection or protocol failure before a model request is sent.
    pub fn prepare(profile_path: &Path, registry_path: Option<&Path>) -> Result<Self, String> {
        if !profile_path.is_absolute() {
            return Err("MCP profile path must be absolute".to_owned());
        }
        let document = std::fs::read_to_string(profile_path).map_err(|error| {
            format!("reading MCP profile `{}`: {error}", profile_path.display())
        })?;
        let profile: Profile = toml::from_str(&document).map_err(|error| {
            format!("invalid MCP profile `{}`: {error}", profile_path.display())
        })?;
        validate_profile(&profile)?;
        let paths = LocalPaths::discover().map_err(|error| error.to_string())?;
        let registry_path = registry_path.unwrap_or(&paths.registry);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("creating MCP runtime: {error}"))?;
        let registry = runtime
            .block_on(Registry::load(registry_path))
            .map_err(|error| error.to_string())?;
        let registry_sha256 = registry.sha256().map_err(|error| error.to_string())?;
        if registry_sha256 != profile.registry_sha256 {
            return Err(format!(
                "MCP registry changed: profile pins {}, current registry is {registry_sha256}",
                profile.registry_sha256
            ));
        }
        let id =
            ConnectionId::new(profile.connection.clone()).map_err(|error| error.to_string())?;
        let connection = runtime
            .block_on(connect_named(&registry, &paths, id, Limits::default()))
            .map_err(|error| error.to_string())?;
        if connection.snapshot().sha256 != profile.snapshot_sha256 {
            return Err(format!(
                "MCP tool snapshot changed for `{}`: profile pins {}, server returned {}",
                profile.connection,
                profile.snapshot_sha256,
                connection.snapshot().sha256
            ));
        }
        let specs = project_specs(connection.snapshot(), &profile)?;
        let evidence = Evidence {
            connection: profile.connection,
            registry_sha256,
            profile_sha256: hex(Sha256::digest(document.as_bytes())),
            snapshot_sha256: connection.snapshot().sha256.clone(),
            protocol_version: connection.snapshot().protocol_version.clone(),
        };
        Ok(Self {
            runtime,
            connection,
            specs,
            policies: profile.tools,
            evidence,
        })
    }

    /// Body-free evidence for this prepared connection.
    pub fn evidence(&self) -> &Evidence {
        &self.evidence
    }

    fn policy(&self, published: &ToolName) -> Option<&ToolPolicy> {
        self.policies
            .iter()
            .find(|policy| policy.publish == published.as_str())
    }
}

impl ToolPort for McpTools {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    fn subjects(&self, call: &ToolCall) -> Vec<Subject> {
        self.policy(&call.name).map_or_else(Vec::new, |policy| {
            policy.subjects.iter().map(PolicySubject::subject).collect()
        })
    }

    fn operation(&self, call: &ToolCall) -> Option<String> {
        self.policy(&call.name)
            .map(|policy| format!("mcp:{}:{}", self.evidence.connection, policy.remote))
    }

    fn call(&mut self, call: &ToolCall) -> ToolOutcome {
        self.call_within(call, None)
    }

    fn call_within(&mut self, call: &ToolCall, remaining: Option<Duration>) -> ToolOutcome {
        let Some(policy) = self.policy(&call.name) else {
            return ToolOutcome::failed(format!(
                "MCP tool `{}` was not published by this reviewed profile",
                call.name
            ));
        };
        let remote = policy.remote.clone();
        let request = McpCall {
            name: remote,
            arguments: call.arguments.clone(),
        };
        match self
            .runtime
            .block_on(self.connection.call(&request, remaining))
        {
            Ok(result) => ToolOutcome {
                output: result.raw,
                failed: result.is_error,
                refusal: None,
            },
            Err(error) => ToolOutcome::failed(format!("MCP call failed: {error}")),
        }
    }
}

fn project_specs(
    snapshot: &b10x_mcp_types::ToolSnapshot,
    profile: &Profile,
) -> Result<Vec<ToolSpec>, String> {
    let mut seen_remote = std::collections::BTreeSet::new();
    let mut seen_published = std::collections::BTreeSet::new();
    profile
        .tools
        .iter()
        .map(|policy| {
            if !seen_remote.insert(&policy.remote) {
                return Err(format!("MCP profile grants `{}` twice", policy.remote));
            }
            if !seen_published.insert(&policy.publish) {
                return Err(format!(
                    "MCP profile publishes `{}` twice",
                    policy.publish
                ));
            }
            let descriptor = snapshot.tool(&policy.remote).ok_or_else(|| {
                format!(
                    "MCP profile grants `{}` but the reviewed snapshot does not list it",
                    policy.remote
                )
            })?;
            let name = ToolName::new(policy.publish.clone())
                .map_err(|error| format!("MCP publish name `{}`: {error}", policy.publish))?;
            if !name
                .as_str()
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            {
                return Err(format!(
                    "MCP publish name `{name}` must match [A-Za-z0-9_-]+ for every shipped wire"
                ));
            }
            let description = policy.description.clone().ok_or_else(|| {
                format!(
                    "MCP tool `{}` needs locally reviewed `description`; server prose is not authority",
                    policy.remote
                )
            })?;
            Ok(ToolSpec {
                name,
                description,
                input_schema: descriptor.input_schema.clone(),
                approval: Approval::NotRequired,
                envelope: policy.envelope.clone(),
            })
        })
        .collect()
}

fn validate_profile(profile: &Profile) -> Result<(), String> {
    ConnectionId::new(profile.connection.clone()).map_err(|error| error.to_string())?;
    for (name, value) in [
        ("registry-sha256", profile.registry_sha256.as_str()),
        ("snapshot-sha256", profile.snapshot_sha256.as_str()),
    ] {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!("MCP profile `{name}` must be a SHA-256 hex digest"));
        }
    }
    if profile.tools.is_empty() {
        return Err("MCP profile grants no tools".to_owned());
    }
    Ok(())
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .fold(String::new(), |mut output, byte| {
            use std::fmt::Write as _;
            let _ = write!(output, "{byte:02x}");
            output
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_annotations_do_not_set_the_harness_envelope() {
        let snapshot = b10x_mcp_testkit::synthetic_snapshot("synthetic").unwrap();
        let profile = Profile {
            connection: "synthetic".to_owned(),
            registry_sha256: "a".repeat(64),
            snapshot_sha256: snapshot.sha256.clone(),
            tools: vec![ToolPolicy {
                remote: "close_issue".to_owned(),
                publish: "mcp_synthetic_close_issue".to_owned(),
                description: Some("Reviewed close operation".to_owned()),
                envelope: Envelope::read_only(),
                subjects: Vec::new(),
            }],
        };
        let specs = project_specs(&snapshot, &profile).unwrap();
        assert_eq!(specs[0].envelope, Envelope::read_only());
        assert_eq!(snapshot.tools[1].raw["annotations"]["readOnlyHint"], true);
    }

    #[test]
    fn every_grant_needs_reviewed_prose() {
        let snapshot = b10x_mcp_testkit::synthetic_snapshot("synthetic").unwrap();
        let profile = Profile {
            connection: "synthetic".to_owned(),
            registry_sha256: "a".repeat(64),
            snapshot_sha256: snapshot.sha256.clone(),
            tools: vec![ToolPolicy {
                remote: "read_issue".to_owned(),
                publish: "read_issue".to_owned(),
                description: None,
                envelope: Envelope::read_only(),
                subjects: Vec::new(),
            }],
        };
        assert!(project_specs(&snapshot, &profile).is_err());
    }
}
