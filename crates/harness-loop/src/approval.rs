use harness_wire::{ToolCall, ToolSpec};
use serde::{Deserialize, Serialize};

/// What a person decided about one call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ApprovalDecision {
    Approved,
    Denied { reason: String },
}

impl ApprovalDecision {
    pub fn denied(reason: impl Into<String>) -> Self {
        Self::Denied {
            reason: reason.into(),
        }
    }

    pub fn is_approved(&self) -> bool {
        matches!(self, Self::Approved)
    }
}

/// Who decides, when a published tool says a person must.
///
/// Because the loop is ours, this is an ordinary blocking call rather than a protocol round trip.
/// The loop stops until it returns, so a decision cannot arrive after the effect.
pub trait ApprovalPort {
    fn decide(&mut self, call: &ToolCall, spec: &ToolSpec) -> ApprovalDecision;
}

/// Denies every request.
///
/// The default, because a harness that approves by default turns a review gate into decoration.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DenyAll;

impl ApprovalPort for DenyAll {
    fn decide(&mut self, _: &ToolCall, spec: &ToolSpec) -> ApprovalDecision {
        // Says that a retry cannot help, because the alternative was measured: a model told only
        // "not approved" tries the same call again until the turn budget is gone.
        ApprovalDecision::denied(format!(
            "`{}` needs a person's decision and no approver is attached to this run, so retrying \
             cannot approve it either; do what can be done without it and say what could not",
            spec.name
        ))
    }
}

/// Approves every request. For tests and for an explicitly unattended run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ApproveAll;

impl ApprovalPort for ApproveAll {
    fn decide(&mut self, _: &ToolCall, _: &ToolSpec) -> ApprovalDecision {
        ApprovalDecision::Approved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_wire::Envelope;
    use harness_wire::{Approval, CallId, ToolName};
    use serde_json::json;

    fn call_and_spec() -> (ToolCall, ToolSpec) {
        let name = ToolName::new("fs.write").expect("valid");
        (
            ToolCall {
                call_id: CallId::new("call-1").expect("valid"),
                name: name.clone(),
                arguments: json!({}),
            },
            ToolSpec {
                name,
                description: "writes".to_owned(),
                input_schema: json!({"type": "object"}),
                approval: Approval::Required,
                envelope: Envelope::default(),
            },
        )
    }

    #[test]
    fn the_default_approver_denies_and_says_why() {
        let (call, spec) = call_and_spec();
        let decision = DenyAll.decide(&call, &spec);
        assert!(!decision.is_approved());
        let ApprovalDecision::Denied { reason } = decision else {
            panic!("DenyAll denies");
        };
        assert!(reason.contains("fs.write"), "{reason}");
    }

    #[test]
    fn approve_all_approves() {
        let (call, spec) = call_and_spec();
        assert!(ApproveAll.decide(&call, &spec).is_approved());
    }
}
