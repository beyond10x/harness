//! What a walk reads out of a document before and while it runs: step kinds, write scopes, the
//! derived answer schema and the bounds the plan reconciled.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use harness_catalogue::{Scope, ScopeRule, WriteScope};
use harness_flow::{Flow, Group, Node, NodeId, Plan, Step};
use serde_json::{Map, Value};

/// Refuses a step payload this runner could not read, anywhere in the document.
///
/// The notation keeps `run` opaque, so nothing in `harness-flow` can say this. Here it is a
/// refusal before the first request rather than a step that quietly arrives with no prompt: a
/// payload that is a string names none of the keys § 2 reads, and a walk that ran it would send
/// the flow's input eight times and call the result a workflow.
///
/// An **absent** payload is fine — every key is optional, and a step with none is a step whose
/// prompt is the flow's own input.
///
/// A `scope` this build cannot read is refused **here**, by name, for the same reason and a
/// sharper one: it is the key that says where the step may write, and a walk that fell through to
/// the run's scope because it could not read the document's would run wider than the document
/// says while every reader of that document believes otherwise.
///
/// # Errors
///
/// Names the step by path and what is wrong with its payload.
pub fn check_payloads(flow: &Flow) -> Result<(), String> {
    payloads(&flow.root, &flow.root.id)
}

fn payloads(group: &Group, path: &str) -> Result<(), String> {
    for node in &group.nodes {
        let here = format!("{path}.{}", node.id());
        match node {
            Node::Group(inner) => payloads(inner, &here)?,
            Node::Step(step) if step.run.is_object() || step.run.is_null() => {
                declared_scope(step).map_err(|reason| format!("`{here}` {reason}"))?;
                action_of(step).map_err(|reason| format!("`{here}` {reason}"))?;
            }
            Node::Step(step) => {
                return Err(format!(
                    "`{here}` carries a `run` payload that is {}: a step's payload is an object of \
                     the keys this runner reads — `kind`, `state`, `summary`, `description`, \
                     `prompt`, `context`, `scope`, `command` — or nothing at all",
                    shape(&step.run)
                ));
            }
        }
    }
    Ok(())
}

/// One phrase saying what a value is, for a refusal that names the problem.
fn shape(value: &Value) -> &'static str {
    match value {
        Value::Object(_) => "an object",
        Value::Array(_) => "an array",
        Value::String(_) => "a string",
        Value::Number(_) => "a number",
        Value::Bool(_) => "a boolean",
        Value::Null => "nothing",
    }
}

/// How many attempts each section may take, by path.
///
/// Read from the [`Plan`] — which is where `repeat.max`, a caller's bound and the default of one
/// have already been reconciled — because a governor asked *whether* to allow a retreat must be
/// told how many are left, and working it out a second time from the document would be a second
/// answer to a question the plan already answered.
pub fn bounds(plan: &Plan) -> BTreeMap<String, u32> {
    let mut found = BTreeMap::new();
    collect_bounds(plan, &mut found);
    found
}

fn collect_bounds(plan: &Plan, into: &mut BTreeMap<String, u32>) {
    into.insert(plan.path.clone(), plan.attempts);
    for inner in plan.groups.values() {
        collect_bounds(inner, into);
    }
}

/// What each section promises its siblings, by path.
///
/// Read once, before anything runs, because it is what the derived schema offers a step: the names
/// the enclosing group declared and no others.
pub(crate) fn promises(group: &Group, path: &str) -> BTreeMap<String, Vec<NodeId>> {
    let mut found = BTreeMap::new();
    found.insert(path.to_owned(), group.gives.clone());
    for node in &group.nodes {
        if let Node::Group(inner) = node {
            found.extend(promises(inner, &format!("{path}.{}", inner.id)));
        }
    }
    found
}

/// The shape every step answers in, derived from the enclosing group's own `gives`.
///
/// **The model never sees a schema file.** `outcome` is the
/// [`StepOutcome`](harness_flow::StepOutcome) the walk acts on; `note` is what the step wants the
/// record to say; `gives` carries the names the group promised, and nothing else — a step cannot
/// hand over a name its own section never declared, because the document is where that contract
/// is written.
///
/// `gives` is present even for a section that promises nothing, with no properties. A key that
/// appeared only sometimes would read as a schema that forgot, and the step still has to answer
/// `outcome` either way.
pub fn answer_schema(gives: &[NodeId]) -> Value {
    let promised: Map<String, Value> = gives
        .iter()
        .map(|name| (name.clone(), serde_json::json!({})))
        .collect();
    serde_json::json!({
        "type": "object",
        "required": ["outcome"],
        "properties": {
            "outcome": {"enum": ["passed", "failed"]},
            "note": {"type": "string"},
            "gives": {"type": "object", "properties": promised},
        },
    })
}

/// One line per rule, in the order the scope was declared in.
///
/// One wording, in one place, because two of them say the same boundary: a run's standing
/// instruction states the **run's** scope before the first turn, and a step of a walk is told its
/// **node's** beside its prompt. A second table of words is a second boundary as far as a model
/// reading them is concerned.
pub fn scope_lines(scope: &Scope) -> String {
    let mut text = String::new();
    for rule in scope.rules() {
        let word = match rule.write {
            WriteScope::Allowed => "may be written or edited",
            WriteScope::PartialOnly => {
                "may be edited in part, never replaced whole — use `file_edit`"
            }
            WriteScope::Denied => "must not be changed at all",
        };
        let _ = writeln!(text, "- `{}` {word}", rule.paths);
    }
    text
}

/// The action named by a step's `kind`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepAction {
    /// A turn of the loop: absent `kind`, or `llm`.
    Llm,
    /// One gated `run` call of this argv, and no model asked anything.
    Command(Vec<String>),
    /// A terminal handoff to a person, with what they are asked.
    Operator { prompt: String },
}

/// The native meaning of a step payload.
///
/// Absent `kind` and explicit `llm` preserve the original model-turn behaviour. `command` is one
/// gated tool call. `operator` is a terminal handoff to a person. Anything else is refused before
/// the run is prepared, so a new producer vocabulary can never silently become a paid model turn.
///
/// # Errors
///
/// A step that says `kind: command` and carries no usable `command` — absent, not a list, empty,
/// or holding something that is not a string — is an error and not a turn: a document that meant
/// to run a verifier and would instead have asked a model about it is the misreport this exists
/// to prevent. So are a `kind` that is not a string, an unknown one, and an `operator` step with
/// no non-empty `prompt`.
pub fn action_of(step: &Step) -> Result<StepAction, String> {
    let payload = step.run.as_object();
    let kind = match payload.and_then(|map| map.get("kind")) {
        None => return Ok(StepAction::Llm),
        Some(Value::String(kind)) => kind.as_str(),
        Some(value) => {
            return Err(format!(
                "carries a `kind` that is {}: expected `llm`, `command` or `operator`",
                shape(value)
            ));
        }
    };
    match kind {
        "llm" => return Ok(StepAction::Llm),
        "operator" => {
            let Some(prompt) = payload
                .and_then(|map| map.get("prompt"))
                .and_then(Value::as_str)
                .filter(|prompt| !prompt.trim().is_empty())
            else {
                return Err(
                    "is an `operator` step and carries no non-empty `prompt` for the person"
                        .to_owned(),
                );
            };
            return Ok(StepAction::Operator {
                prompt: prompt.to_owned(),
            });
        }
        "command" => {}
        other => {
            return Err(format!(
                "declares unknown step kind `{other}`: expected `llm`, `command` or `operator`"
            ));
        }
    }
    let Some(command) = payload
        .and_then(|map| map.get("command"))
        .and_then(Value::as_array)
    else {
        return Err("is a `command` step and carries no `command` list to run".to_owned());
    };
    let argv: Option<Vec<String>> = command
        .iter()
        .map(|word| word.as_str().map(str::to_owned))
        .collect();
    match argv {
        Some(argv) if !argv.is_empty() => Ok(StepAction::Command(argv)),
        Some(_) => Err("is a `command` step whose `command` is empty".to_owned()),
        None => Err(
            "is a `command` step whose `command` holds something that is not a string".to_owned(),
        ),
    }
}

/// Where **this step** may write, read from its node, or an empty scope when it declares none.
///
/// The grammar is `--write-scope`'s, `<glob>=<word>`, because that is exactly what
/// `aep govern workflow flow --map` emits into the node — one reader for one grammar
/// ([`ScopeRule::parse`]), so a projection and a driven run cannot come to disagree about what a
/// rule means. The list is kept **in the order the map wrote it**: first match wins, so
/// re-ordering it changes what it says and nothing here may sort it.
///
/// # Errors
///
/// Names the declaration that could not be read, and what it is instead. A `scope` this build
/// cannot read must never fall through to the run's scope: that is a document stating a boundary
/// and a walk quietly running without it, which is the whole failure this key exists to close.
pub fn declared_scope(step: &Step) -> Result<Scope, String> {
    let Some(scope) = step.run.as_object().and_then(|map| map.get("scope")) else {
        return Ok(Scope::default());
    };
    if scope.is_null() {
        return Ok(Scope::default());
    }
    let Some(rules) = scope.as_array() else {
        return Err(format!(
            "carries a `scope` that is {}: a step's write scope is a list of \
             `<glob>=<allowed|partial-only|denied>`",
            shape(scope)
        ));
    };
    let declarations: Option<Vec<&str>> = rules.iter().map(Value::as_str).collect();
    let Some(declarations) = declarations else {
        return Err(
            "carries a `scope` holding something that is not a string: every rule is one \
             `<glob>=<allowed|partial-only|denied>`"
                .to_owned(),
        );
    };
    declarations
        .into_iter()
        .map(ScopeRule::parse)
        .collect::<Result<Vec<_>, _>>()
        .map(Scope::of)
        .map_err(|reason| format!("carries a `scope` this build cannot read: {reason}"))
}

/// A handed-over value as one line: a string as itself, anything else as the JSON it is.
///
/// A quoted `"SPEC-1"` in a prompt reads as a literal with quotes in it, which is not what the
/// section handed over.
pub(crate) fn plain(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_derived_schema_offers_exactly_what_the_section_promised() {
        // Design 0003 § 2, byte for byte. The model never sees a schema file, so this is the only
        // place the shape of a step's answer is written down.
        assert_eq!(
            answer_schema(&["a".to_owned(), "b".to_owned()]),
            serde_json::json!({
                "type": "object",
                "required": ["outcome"],
                "properties": {
                    "outcome": {"enum": ["passed", "failed"]},
                    "note": {"type": "string"},
                    "gives": {"type": "object", "properties": {"a": {}, "b": {}}},
                },
            })
        );
        // A section that promises nothing still answers `outcome`, and `gives` is present with no
        // properties rather than absent.
        assert_eq!(
            answer_schema(&[])["properties"]["gives"],
            serde_json::json!({"type": "object", "properties": {}})
        );
    }

    #[test]
    fn a_step_payload_that_is_not_an_object_is_refused_by_path() {
        let flow = Flow::from_yaml(
            "id: f\nroot:\n  id: root\n  nodes:\n    - id: one\n      run: \"just a string\"\n",
        )
        .expect("the document reads");
        let error = check_payloads(&flow).expect_err("refused");
        assert!(error.contains("`root.one`"), "{error}");
        assert!(error.contains("a string"), "{error}");
    }

    #[test]
    fn a_step_with_no_payload_at_all_is_accepted() {
        let flow = Flow::from_yaml("id: f\nroot:\n  id: root\n  nodes:\n    - id: one\n")
            .expect("the document reads");
        check_payloads(&flow).expect("every key is optional");
    }

    /// Each native kind is explicit, and malformed or unknown kinds never fall through to a turn.
    #[test]
    fn step_kinds_are_read_by_name_and_malformed_ones_are_errors_not_turns() {
        let step = |run: Value| Step {
            id: "check".to_owned(),
            needs: Vec::new(),
            run,
        };
        assert_eq!(
            action_of(&step(
                serde_json::json!({"state": "verify", "prompt": "Run it."})
            )),
            Ok(StepAction::Llm)
        );
        assert_eq!(
            action_of(&step(serde_json::json!({"state": "verify"}))),
            Ok(StepAction::Llm),
            "a step with no kind is a turn, as every step was"
        );
        assert_eq!(
            action_of(&step(serde_json::json!({
                "state": "verify", "kind": "command", "command": ["cargo", "test", "--workspace"]
            }))),
            Ok(StepAction::Command(vec![
                "cargo".to_owned(),
                "test".to_owned(),
                "--workspace".to_owned()
            ]))
        );
        assert_eq!(
            action_of(&step(serde_json::json!({
                "kind": "operator", "prompt": "Approve the specification."
            }))),
            Ok(StepAction::Operator {
                prompt: "Approve the specification.".to_owned()
            })
        );
        assert!(
            action_of(&step(serde_json::json!({"kind": "command"})))
                .is_err_and(|reason| reason.contains("no `command` list")),
        );
        assert!(
            action_of(&step(serde_json::json!({"kind": "command", "command": []})))
                .is_err_and(|reason| reason.contains("is empty")),
        );
        assert!(
            action_of(&step(
                serde_json::json!({"kind": "command", "command": ["sh", 1]})
            ))
            .is_err_and(|reason| reason.contains("not a string")),
        );
        assert!(
            action_of(&step(
                serde_json::json!({"kind": "operator", "prompt": "  "})
            ))
            .is_err_and(|reason| reason.contains("non-empty `prompt`")),
        );
        assert!(
            action_of(&step(serde_json::json!({"kind": "delegate"})))
                .is_err_and(|reason| reason.contains("unknown step kind `delegate`")),
        );
    }

    #[test]
    fn a_handed_over_string_reaches_a_prompt_without_its_quotes() {
        assert_eq!(plain(&serde_json::json!("SPEC-1")), "SPEC-1");
        assert_eq!(plain(&serde_json::json!({"id": 1})), "{\"id\":1}");
    }

    #[test]
    fn a_scope_this_build_cannot_read_is_refused_when_the_document_is_read() {
        // By name, at validate time, and never a fall-through to the run's scope: a document that
        // states a boundary and a walk that quietly ran without it is the failure the key exists
        // to close. `workflow plan` refuses it too, because it validates the same way.
        let refusal = |scope: &str| {
            let flow = Flow::from_yaml(&format!(
                "id: f\nroot:\n  id: root\n  nodes:\n    - id: one\n      run:\n        \
                 state: s\n        scope: {scope}\n"
            ))
            .expect("the document reads");
            check_payloads(&flow).expect_err("refused")
        };

        let shape = refusal("\"everything\"");
        assert!(shape.contains("`root.one`"), "{shape}");
        assert!(shape.contains("a string"), "{shape}");

        let word = refusal("[\"crates/**=readonly\"]");
        assert!(word.contains("`root.one`"), "{word}");
        assert!(word.contains("partial-only"), "and the three words: {word}");

        let listed = refusal("[7]");
        assert!(listed.contains("not a string"), "{listed}");

        let flow = Flow::from_yaml(
            "id: f\nroot:\n  id: root\n  nodes:\n    - id: one\n      run:\n        \
             state: s\n        scope: [\".engineering/**=denied\", \"**=allowed\"]\n",
        )
        .expect("the document reads");
        check_payloads(&flow).expect("the grammar `--write-scope` already takes");
    }
}
