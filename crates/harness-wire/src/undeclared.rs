//! A field a tool's published schema does not declare, found before the tool runs.
//!
//! # Why the schema is not left to the model's provider
//!
//! A schema that says `additionalProperties: false` is a promise to the caller: *a field I do not
//! name is refused*. A provider may hold the model to it, and nothing else that calls a tool does —
//! an MCP client, a replayed record, a provider that does not enforce schemas. Measured on harness
//! 0.15.1 through a served surface: `file_write` carried an undeclared `expected_sha256` holding a
//! stale digest, the field was ignored, and the file was replaced. A caller that sent a guard got
//! neither the guard nor a refusal.
//!
//! So whoever performs a call asks this first, against the exact schema it published, and refuses
//! by name. Only `additionalProperties: false` is read — a schema that does not say it accepts
//! extra fields exactly as before — and nothing else in the schema is checked here: types,
//! `required` and `enum` stay the tool's own business, as they were.
//!
//! # What it walks
//!
//! The top level and, through `properties`, `prefixItems` and `items` as JSON Schema 2020-12
//! reads them, every nested object the arguments carry whose own schema is closed. A level that
//! carries a keyword this walk does not model — `$ref`, `allOf`, `anyOf`, `oneOf`, `not`,
//! `if`/`then`/`else`, `patternProperties`, `dependentSchemas`, the `unevaluated*` pair and their
//! older-draft relatives — or that names another dialect in `$schema` is not judged, and neither is
//! anything beneath it. This check only adds refusals, so it must never refuse a call the schema
//! admits, and leaving a level unjudged is the direction that cannot. No schema this harness
//! publishes uses any of them; an operator's `answer` schema may.

use serde_json::Value;

/// A field one call carried that the object it sits in does not declare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndeclaredField {
    /// Where the field sits, outermost first: `["expected_sha256"]` at the top,
    /// `["rust", "verbose"]` inside a nested object, an array index as its decimal digits.
    pub path: Vec<String>,
    /// Every name the object it sits in declares, in the schema's own order.
    pub accepted: Vec<String>,
}

impl UndeclaredField {
    /// The field's path as one dotted name: `rust.verbose`, `items.0.name`.
    #[must_use]
    pub fn dotted(&self) -> String {
        self.path.join(".")
    }

    /// The sentence the caller reads: which field, which tool, what it accepts, and that nothing
    /// was done.
    #[must_use]
    pub fn refusal(&self, tool: &str) -> String {
        let accepted = if self.accepted.is_empty() {
            "no arguments at all".to_owned()
        } else {
            self.accepted
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let field = self.dotted();
        let within = match self.path.split_last() {
            Some((_, outer)) if !outer.is_empty() => format!("`{}` accepts", outer.join(".")),
            _ => "it accepts".to_owned(),
        };
        format!(
            "`{field}` is not an argument of `{tool}`, whose schema declares no field it does not \
             name: {within} {accepted}. Nothing was done; call it again without `{field}`."
        )
    }
}

/// The first field `arguments` carries that a closed object of `schema` does not declare, or
/// [`None`] when there is none.
///
/// Arguments that are not an object, or a schema that is not one, have nothing to judge here and
/// answer [`None`]: what the tool makes of them is unchanged.
#[must_use]
pub fn undeclared_field(schema: &Value, arguments: &Value) -> Option<UndeclaredField> {
    let mut path = Vec::new();
    find(schema, arguments, &mut path)
}

/// Keywords that change which fields or elements a schema level admits in a way this walk does not
/// model. A level carrying any of them is not judged, and neither is anything beneath it: this
/// check only ever adds refusals, so not judging is the direction that cannot refuse a call the
/// schema admits.
const UNMODELLED: [&str; 16] = [
    "$ref",
    "$dynamicRef",
    "$recursiveRef",
    "allOf",
    "anyOf",
    "oneOf",
    "not",
    "if",
    "then",
    "else",
    "patternProperties",
    "dependentSchemas",
    "dependencies",
    "unevaluatedProperties",
    "unevaluatedItems",
    "additionalItems",
];

/// The dialect whose `prefixItems` and `items` this walk reads.
const DRAFT_2020_12: &str = "https://json-schema.org/draft/2020-12/schema";

/// Whether this walk can read the level at all.
///
/// A level that names another dialect is not read either: before 2020-12, `prefixItems` meant
/// nothing and `items` applied to every element, which is not what [`find`] assumes.
fn modelled(schema: &serde_json::Map<String, Value>) -> bool {
    !UNMODELLED
        .iter()
        .any(|keyword| schema.contains_key(*keyword))
        && schema
            .get("$schema")
            .is_none_or(|dialect| dialect.as_str() == Some(DRAFT_2020_12))
}

fn find(schema: &Value, value: &Value, path: &mut Vec<String>) -> Option<UndeclaredField> {
    let schema = schema.as_object()?;
    if !modelled(schema) {
        return None;
    }
    match value {
        Value::Object(fields) => {
            let properties = schema.get("properties").and_then(Value::as_object);
            let closed = schema.get("additionalProperties") == Some(&Value::Bool(false));
            for (name, inner) in fields {
                match properties.and_then(|properties| properties.get(name)) {
                    Some(declared) => {
                        path.push(name.clone());
                        let found = find(declared, inner, path);
                        path.pop();
                        if found.is_some() {
                            return found;
                        }
                    }
                    None if closed => {
                        path.push(name.clone());
                        return Some(UndeclaredField {
                            path: path.clone(),
                            accepted: properties
                                .map(|properties| properties.keys().cloned().collect())
                                .unwrap_or_default(),
                        });
                    }
                    None => {}
                }
            }
            None
        }
        // JSON Schema 2020-12: element `i` below `prefixItems`' length is judged by
        // `prefixItems[i]` alone, and only the elements after it by `items`.
        Value::Array(items) => {
            let prefix = schema
                .get("prefixItems")
                .and_then(Value::as_array)
                .map_or(&[][..], Vec::as_slice);
            let rest = schema.get("items");
            items.iter().enumerate().find_map(|(index, item)| {
                let each = prefix.get(index).or(rest)?;
                path.push(index.to_string());
                let found = find(each, item, path);
                path.pop();
                found
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn writer() -> Value {
        json!({
            "type": "object",
            "properties": {"path": {"type": "string"}, "text": {"type": "string"}},
            "required": ["path", "text"],
            "additionalProperties": false,
        })
    }

    #[test]
    fn a_field_a_closed_schema_does_not_declare_is_found_with_what_it_accepts() {
        let found = undeclared_field(
            &writer(),
            &json!({"path": "a", "text": "b", "expected_sha256": "00"}),
        )
        .expect("found");
        assert_eq!(found.path, ["expected_sha256"]);
        assert_eq!(found.accepted, ["path", "text"]);
        let words = found.refusal("file_write");
        for named in ["`expected_sha256`", "`file_write`", "`path`", "`text`"] {
            assert!(words.contains(named), "{words}");
        }
    }

    #[test]
    fn a_schema_that_does_not_close_itself_accepts_an_extra_field_as_before() {
        let open = json!({"type": "object", "properties": {"path": {"type": "string"}}});
        assert_eq!(
            undeclared_field(&open, &json!({"path": "a", "extra": 1})),
            None
        );
        let mut explicit = open.clone();
        explicit["additionalProperties"] = json!(true);
        assert_eq!(undeclared_field(&explicit, &json!({"extra": 1})), None);
    }

    #[test]
    fn declared_fields_and_arguments_that_are_not_an_object_are_not_its_business() {
        assert_eq!(undeclared_field(&writer(), &json!({"path": "a"})), None);
        assert_eq!(undeclared_field(&writer(), &Value::Null), None);
        assert_eq!(undeclared_field(&writer(), &json!("text")), None);
    }

    #[test]
    fn a_closed_nested_object_is_judged_by_its_own_schema_and_named_by_its_path() {
        let role = json!({
            "type": "object",
            "properties": {"rust": writer(), "loose": {"type": "object"}},
            "additionalProperties": false,
        });
        let found = undeclared_field(&role, &json!({"rust": {"path": "a", "verbose": true}}))
            .expect("found");
        assert_eq!(found.path, ["rust", "verbose"]);
        assert_eq!(found.accepted, ["path", "text"]);
        assert!(found.refusal("toolchain_test").contains("`rust.verbose`"));
        assert!(found.refusal("toolchain_test").contains("`rust` accepts"));
        assert_eq!(
            undeclared_field(&role, &json!({"loose": {"anything": 1}})),
            None,
            "a nested object that does not close itself is open"
        );
    }

    #[test]
    fn an_item_of_a_list_is_judged_by_the_items_schema() {
        let list = json!({
            "type": "object",
            "properties": {"edits": {"type": "array", "items": writer()}},
            "additionalProperties": false,
        });
        let found = undeclared_field(
            &list,
            &json!({"edits": [{"path": "a", "text": "b"}, {"path": "c", "old": "d"}]}),
        )
        .expect("found");
        assert_eq!(found.path, ["edits", "1", "old"]);
    }

    #[test]
    fn an_element_covered_by_prefix_items_is_judged_by_its_own_prefix_schema_alone() {
        // JSON Schema 2020-12: `items` applies only to the elements after `prefixItems`.
        let rows = json!({
            "type": "object",
            "properties": {"rows": {
                "type": "array",
                "prefixItems": [{
                    "type": "object",
                    "properties": {"title": {"type": "string"}},
                    "additionalProperties": false,
                }],
                "items": {
                    "type": "object",
                    "properties": {"cell": {"type": "string"}},
                    "additionalProperties": false,
                },
            }},
            "additionalProperties": false,
        });
        assert_eq!(
            undeclared_field(&rows, &json!({"rows": [{"title": "t"}, {"cell": "c"}]})),
            None
        );
        let first = undeclared_field(&rows, &json!({"rows": [{"cell": "c"}]})).expect("found");
        assert_eq!(first.path, ["rows", "0", "cell"]);
        assert_eq!(first.accepted, ["title"]);
        let later = undeclared_field(&rows, &json!({"rows": [{"title": "t"}, {"title": "u"}]}))
            .expect("found");
        assert_eq!(later.path, ["rows", "1", "title"]);
        assert_eq!(later.accepted, ["cell"]);
    }

    /// A schema level carrying `keyword` would refuse `b` were it closed and modelled; neither it
    /// nor anything beneath it is judged.
    fn assert_skipped(keyword: &str, value: &Value) {
        let mut level = json!({
            "type": "object",
            "properties": {"a": {}, "inner": writer()},
            "additionalProperties": false,
        });
        level[keyword] = value.clone();
        assert_eq!(
            undeclared_field(&level, &json!({"a": 1, "b": 2})),
            None,
            "`{keyword}`: the level itself was judged"
        );
        assert_eq!(
            undeclared_field(&level, &json!({"inner": {"path": "p", "extra": 1}})),
            None,
            "`{keyword}`: a child of the level was judged"
        );
        let outer = json!({
            "type": "object",
            "properties": {"nested": level},
            "additionalProperties": false,
        });
        assert_eq!(
            undeclared_field(&outer, &json!({"nested": {"b": 2}})),
            None,
            "`{keyword}`: the level was judged when nested"
        );
        assert!(
            undeclared_field(&outer, &json!({"c": 3})).is_some(),
            "`{keyword}`: the closed level above it is still judged"
        );
    }

    macro_rules! skipped {
        ($($test:ident: $keyword:literal => $value:tt;)*) => {$(
            #[test]
            fn $test() {
                assert_skipped($keyword, &json!($value));
            }
        )*};
    }

    skipped! {
        a_level_with_ref_is_not_judged: "$ref" => "#/$defs/other";
        a_level_with_dynamic_ref_is_not_judged: "$dynamicRef" => "#meta";
        a_level_with_recursive_ref_is_not_judged: "$recursiveRef" => "#";
        a_level_with_all_of_is_not_judged: "allOf" => [{"properties": {"b": {}}}];
        a_level_with_any_of_is_not_judged: "anyOf" => [{"properties": {"b": {}}}];
        a_level_with_one_of_is_not_judged: "oneOf" => [{"properties": {"b": {}}}];
        a_level_with_not_is_not_judged: "not" => {"required": ["z"]};
        a_level_with_if_is_not_judged: "if" => {"required": ["a"]};
        a_level_with_then_is_not_judged: "then" => {"properties": {"b": {}}};
        a_level_with_else_is_not_judged: "else" => {"properties": {"b": {}}};
        a_level_with_pattern_properties_is_not_judged: "patternProperties" => {"^b$": {}};
        a_level_with_dependent_schemas_is_not_judged: "dependentSchemas" => {"a": {"properties": {"b": {}}}};
        a_level_with_dependencies_is_not_judged: "dependencies" => {"a": {"properties": {"b": {}}}};
        a_level_with_unevaluated_properties_is_not_judged: "unevaluatedProperties" => false;
        a_level_with_unevaluated_items_is_not_judged: "unevaluatedItems" => false;
        a_level_with_additional_items_is_not_judged: "additionalItems" => false;
        a_level_naming_an_older_draft_is_not_judged: "$schema" => "http://json-schema.org/draft-07/schema#";
    }

    #[test]
    fn a_level_naming_draft_2020_12_is_judged() {
        let mut level = writer();
        level["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
        assert!(undeclared_field(&level, &json!({"path": "p", "extra": 1})).is_some());
    }

    #[test]
    fn a_list_whose_items_is_a_tuple_of_an_older_draft_is_not_judged() {
        let tuple = json!({
            "type": "object",
            "properties": {"pair": {"type": "array", "items": [writer(), writer()]}},
            "additionalProperties": false,
        });
        assert_eq!(
            undeclared_field(&tuple, &json!({"pair": [{"path": "p", "extra": 1}]})),
            None
        );
    }

    #[test]
    fn an_object_that_declares_nothing_says_it_takes_no_arguments() {
        let empty = json!({"type": "object", "properties": {}, "additionalProperties": false});
        let found = undeclared_field(&empty, &json!({"extra": 1})).expect("found");
        assert!(found.refusal("rust_check").contains("no arguments at all"));
    }
}
