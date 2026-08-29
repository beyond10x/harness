//! Structured output: a schema the model answers by calling a tool.
//!
//! # Why a tool and not a wire feature
//!
//! Both wires have a provider-native shape for this — `text.format` on one, a header-gated beta on
//! the other — and each would cost a new pinned contract version, and the second would ship on a
//! feature nothing here has seen live. Publishing the schema as a tool the model calls to finish
//! is wire-neutral, needs no contract change, is testable against both emulators today, and is
//! what a delegate's structured answer will be built on. The provider-native path is milestone M2
//! of `docs/design/0002-sub-agents-structured-output-hooks.md` § 1, behind the same value.

use harness_wire::{Approval, Envelope, Idempotency, Risk, ToolName, ToolSpec};
use serde_json::Value;

/// The tool name a schema is published under when the caller names none.
pub const DEFAULT_ANSWER_NAME: &str = "answer";

/// How many times a run whose model ended in prose is told to call the answer tool instead.
///
/// One. The nudge is a turn like any other and is charged to every ceiling; a second one would be
/// paying twice for the same instruction. After it the run stops
/// [`crate::LoopStop::Unstructured`] — never `Completed`, because a consumer that piped stdout to
/// a JSON reader and got prose with exit 0 would be the silent failure AGENTS.md invariant 8
/// forbids.
pub const MAX_ANSWER_NUDGES: u32 = 1;

/// What the model is told about the answer tool when the caller says nothing.
pub const DEFAULT_ANSWER_DESCRIPTION: &str = "Finish the task by calling this tool exactly once \
    with the result in the shape its schema gives. Call it alone, as the last thing: any other \
    call in the same turn is refused, and nothing after it is read.";

/// The shape a run's answer must take.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputSchema {
    /// The tool the model calls to answer.
    pub name: ToolName,
    /// What the model is told the tool is for.
    pub description: String,
    /// A JSON Schema whose top level is an object.
    pub schema: Value,
}

/// Why a schema was refused before the run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OutputSchemaError {
    /// A tool's `input_schema` must be an object schema on both wires; a refusal before the run
    /// beats a 400 after it.
    #[error(
        "the output schema must be a JSON Schema for an object — a JSON object with \
         `\"type\": \"object\"` at the top level — and this one is {0}"
    )]
    NotAnObject(String),
}

impl OutputSchema {
    /// A schema published under [`DEFAULT_ANSWER_NAME`] with [`DEFAULT_ANSWER_DESCRIPTION`].
    ///
    /// # Errors
    ///
    /// Refuses a schema that is not an object schema at the top level.
    ///
    /// # Panics
    ///
    /// Only if the constant default name stops being a legal tool name.
    pub fn new(schema: Value) -> Result<Self, OutputSchemaError> {
        Self::named(
            ToolName::new(DEFAULT_ANSWER_NAME)
                .expect("the default answer name is a legal tool name"),
            DEFAULT_ANSWER_DESCRIPTION,
            schema,
        )
    }

    /// A schema published under a name and description the caller chose.
    ///
    /// # Errors
    ///
    /// Refuses a schema that is not an object schema at the top level.
    pub fn named(
        name: ToolName,
        description: impl Into<String>,
        schema: Value,
    ) -> Result<Self, OutputSchemaError> {
        let is_object_schema = schema
            .as_object()
            .and_then(|object| object.get("type"))
            .and_then(Value::as_str)
            .is_some_and(|kind| kind == "object");
        if !is_object_schema {
            return Err(OutputSchemaError::NotAnObject(describe(&schema)));
        }
        Ok(Self {
            name,
            description: description.into(),
            schema,
        })
    }

    /// The tool the model sees.
    ///
    /// Answering touches nothing on any machine, so the envelope is the safest thing a tool can
    /// declare, and the gate never asks about it.
    pub fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name.clone(),
            description: self.description.clone(),
            input_schema: self.schema.clone(),
            approval: Approval::NotRequired,
            envelope: Envelope {
                effects: Vec::new(),
                risk: Risk::Low,
                idempotency: Idempotency::Idempotent,
                access: Vec::new(),
            },
        }
    }
}

/// One line saying what a value is, for a refusal that names the problem.
fn describe(value: &Value) -> String {
    match value {
        Value::Object(object) => match object.get("type") {
            Some(kind) => format!("an object whose `type` is {kind}"),
            None => "an object with no `type`".to_owned(),
        },
        Value::Array(_) => "an array".to_owned(),
        Value::String(_) => "a string".to_owned(),
        Value::Number(_) => "a number".to_owned(),
        Value::Bool(_) => "a boolean".to_owned(),
        Value::Null => "null".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_object_schema_is_accepted_under_the_default_name() {
        let schema = OutputSchema::new(json!({
            "type": "object",
            "properties": {"verdict": {"type": "string"}},
            "required": ["verdict"],
        }))
        .expect("an object schema");
        assert_eq!(schema.name.as_str(), DEFAULT_ANSWER_NAME);
        let spec = schema.spec();
        assert_eq!(spec.input_schema["type"], json!("object"));
        assert!(!spec.envelope.mutates());
        assert!(!spec.envelope.needs_approval(Risk::Low));
    }

    #[test]
    fn a_schema_that_is_not_an_object_schema_is_refused_by_what_it_is() {
        for (schema, expected) in [
            (json!({"type": "string"}), "`type` is \"string\""),
            (json!({"properties": {}}), "no `type`"),
            (json!(["a"]), "an array"),
            (json!(null), "null"),
        ] {
            let error = OutputSchema::new(schema).expect_err("refused");
            assert!(error.to_string().contains(expected), "{error}");
        }
    }
}
