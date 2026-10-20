//! A call carrying a field its entry's published schema does not declare is refused, naming it.
//!
//! Measured by metaharness on harness 0.15.1 through its served surface: `file_write` declares
//! `additionalProperties: false`, and a call carrying an undeclared `expected_sha256` holding a
//! stale digest was accepted — the field was ignored and the file replaced. A caller that sent a
//! digest guard got neither the guard nor a refusal. `file_edit` was the same.
//!
//! These cases hold the class rather than the instance: every entry either surface publishes —
//! the flat one, the three verbs and what `tool_invoke` reaches, toolchain tools and roles
//! included — is sent one undeclared field wherever its published schema says
//! `additionalProperties: false`, at the top and inside every closed nested object, alone and in a
//! batch, and each must refuse naming that field and what the entry accepts.

#![cfg(unix)]

use std::path::Path;

use harness_catalogue::{Catalogue, Flat, INVOKE_VERB, LocalOperations, ResolvedProvider, Verbs};
use harness_wire::{CallId, ToolCall, ToolName, ToolOutcome, ToolPort, ToolSpec};
use serde_json::{Map, Value, json};

/// The field no published schema declares.
const PROBE: &str = "undeclared_probe";

/// A digest of nothing this suite writes, which is what a stale guard looks like.
const STALE: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// What the guarded file holds before any call, and must still hold after a refused one.
const BEFORE: &str = "before\n";

/// A Cargo project the built-in Rust provider resolves in, with one file to guard.
fn crate_tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary workspace");
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .expect("a manifest");
    std::fs::create_dir(dir.path().join("src")).expect("src");
    std::fs::write(dir.path().join("src/lib.rs"), "pub fn formatted() {}\n").expect("a source");
    std::fs::write(dir.path().join("notes.txt"), BEFORE).expect("the guarded file");
    dir
}

fn rust(root: &Path) -> Vec<ResolvedProvider> {
    tool_toolchain::Registry::builtins()
        .expect("the built-ins")
        .resolve(root, Some(&["rust".to_owned()]))
        .expect("the built-in Rust provider resolves where this suite builds")
}

/// Everything a catalogue can publish: the readers, the writers, `run` and the Rust toolchain's
/// tools and roles.
///
/// `true` is declared beside the toolchain's own programs because those are admitted only through
/// their typed tools, and a catalogue whose only programs are internal publishes no `run`.
fn catalogue(root: &Path) -> Catalogue {
    let providers = rust(root);
    let mut programs: Vec<String> = providers
        .iter()
        .flat_map(|provider| provider.programs.iter().cloned())
        .collect();
    programs.push("true".to_owned());
    let operations = LocalOperations::unconfined(root, programs).expect("a workspace");
    Catalogue::of_with_toolchains(operations, providers)
}

fn call(name: &str, arguments: Value) -> ToolCall {
    ToolCall {
        call_id: CallId::new("c-1").expect("valid"),
        name: ToolName::new(name).expect("valid"),
        arguments,
    }
}

fn text(outcome: &ToolOutcome) -> String {
    outcome
        .output
        .as_str()
        .map_or_else(|| outcome.output.to_string(), ToOwned::to_owned)
}

fn guarded(root: &Path) -> String {
    std::fs::read_to_string(root.join("notes.txt")).expect("the guarded file is there")
}

/// Whether a schema refuses what it does not declare.
fn closed(schema: &Value) -> bool {
    schema.get("additionalProperties") == Some(&Value::Bool(false))
}

/// The property names a schema declares, in its own order.
fn declared(schema: &Value) -> Vec<String> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default()
}

/// One undeclared field per closed object in `schema`: at the top, and inside every declared
/// property whose own schema is a closed object, however deep.
///
/// Each probe is the dotted path of the field it adds, the names the object it lands in declares,
/// and the arguments that carry it.
fn probes(schema: &Value) -> Vec<(String, Vec<String>, Value)> {
    let mut found = Vec::new();
    if closed(schema) {
        found.push((PROBE.to_owned(), declared(schema), json!({PROBE: true})));
    }
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (name, inner) in properties {
            for (path, accepted, arguments) in probes(inner) {
                let mut outer = Map::new();
                outer.insert(name.clone(), arguments);
                found.push((format!("{name}.{path}"), accepted, Value::Object(outer)));
            }
        }
    }
    found
}

/// The refusal names the field, as the caller spelled its path, and every name it could have
/// used instead; nothing about it reads as a success.
fn assert_refused_naming(outcome: &ToolOutcome, route: &str, path: &str, accepted: &[String]) {
    let words = text(outcome);
    assert!(outcome.failed, "{route}: not refused: {words}");
    assert!(
        words.contains(&format!("`{path}`")),
        "{route}: the refusal does not name `{path}`: {words}"
    );
    for name in accepted {
        assert!(
            words.contains(&format!("`{name}`")),
            "{route}: the refusal does not name `{name}`, which the entry accepts: {words}"
        );
    }
}

/// The port answers the same refusal **before** the call is performed, through
/// `ToolPort::undeclared`, which is what the loop asks ahead of the approver and every hook — on
/// the port itself and on a fork of it, which is what a delegate running beside another gets.
fn assert_answered_ahead(
    port: &dyn ToolPort,
    call: &ToolCall,
    route: &str,
    path: &str,
    accepted: &[String],
) {
    let ahead = port
        .undeclared(call)
        .unwrap_or_else(|| panic!("{route}: `ToolPort::undeclared` answered nothing"));
    assert_refused_naming(
        &ToolOutcome::failed(ahead),
        &format!("{route} (ahead)"),
        path,
        accepted,
    );
    let fork = port.fork().expect("both surfaces fork");
    let ahead = fork
        .undeclared(call)
        .unwrap_or_else(|| panic!("{route}: a fork's `ToolPort::undeclared` answered nothing"));
    assert_refused_naming(
        &ToolOutcome::failed(ahead),
        &format!("{route} (fork, ahead)"),
        path,
        accepted,
    );
}

/// A published spec, walked: every probe, ahead, alone and as a batch of one.
fn walk(port: &mut dyn ToolPort, spec: &ToolSpec, through: &str) -> usize {
    let probes = probes(&spec.input_schema);
    for (path, accepted, arguments) in &probes {
        let route = format!("{through} {}", spec.name.as_str());
        assert_answered_ahead(
            port,
            &call(spec.name.as_str(), arguments.clone()),
            &route,
            path,
            accepted,
        );
        let alone = port.call(&call(spec.name.as_str(), arguments.clone()));
        assert_refused_naming(&alone, &route, path, accepted);
        let batch = port.call_batch(&[call(spec.name.as_str(), arguments.clone())], None);
        assert_eq!(batch.len(), 1, "{route}: one answer per call");
        assert_refused_naming(&batch[0], &format!("{route} (batch)"), path, accepted);
    }
    probes.len()
}

#[test]
fn file_write_and_file_edit_refuse_a_stale_expected_sha256_by_name_and_change_nothing() {
    let dir = crate_tree();
    let root = dir.path();
    let calls = [
        (
            "file_write",
            json!({"path": "notes.txt", "text": "after\n", "expected_sha256": STALE}),
        ),
        (
            "file_edit",
            json!({"path": "notes.txt", "old": "before", "new": "after", "expected_sha256": STALE}),
        ),
    ];
    for (name, arguments) in &calls {
        let accepted = declared(
            &catalogue(root)
                .get(name)
                .expect("a writing provider publishes both writers")
                .input_schema,
        );
        let direct = catalogue(root)
            .invoke(name, arguments)
            .expect_err("refused before anything is written");
        assert!(
            direct.message().contains("`expected_sha256`"),
            "{name}: {direct}"
        );
        assert_eq!(guarded(root), BEFORE, "{name} through `Catalogue::invoke`");

        let mut flat = Flat::new(catalogue(root));
        let outcome = flat.call(&call(name, arguments.clone()));
        assert_refused_naming(
            &outcome,
            &format!("flat {name}"),
            "expected_sha256",
            &accepted,
        );
        assert_eq!(guarded(root), BEFORE, "{name} through the flat surface");
        let outcome = flat.call_batch(&[call(name, arguments.clone())], None);
        assert_refused_naming(
            &outcome[0],
            &format!("flat batch {name}"),
            "expected_sha256",
            &accepted,
        );
        assert_eq!(guarded(root), BEFORE, "{name} through a flat batch");

        let mut verbs = Verbs::new(catalogue(root));
        let invoke = call(INVOKE_VERB, json!({"name": name, "arguments": arguments}));
        let outcome = verbs.call(&invoke);
        assert_refused_naming(
            &outcome,
            &format!("tool_invoke {name}"),
            "expected_sha256",
            &accepted,
        );
        assert_eq!(guarded(root), BEFORE, "{name} through `tool_invoke`");
        let outcome = verbs.call_batch(std::slice::from_ref(&invoke), None);
        assert_refused_naming(
            &outcome[0],
            &format!("tool_invoke batch {name}"),
            "expected_sha256",
            &accepted,
        );
        assert_eq!(
            guarded(root),
            BEFORE,
            "{name} through a `tool_invoke` batch"
        );
        let outcome = verbs.call(&call(name, arguments.clone()));
        assert_refused_naming(
            &outcome,
            &format!("bare {name}"),
            "expected_sha256",
            &accepted,
        );
        assert_eq!(
            guarded(root),
            BEFORE,
            "{name} called by its bare name on the verbs"
        );
    }

    // And the same calls without the field still write: the refusal is about the field.
    let mut flat = Flat::new(catalogue(root));
    let valid = json!({"path": "notes.txt", "text": "after\n"});
    assert_eq!(flat.undeclared(&call("file_write", valid.clone())), None);
    let verbs = Verbs::new(catalogue(root));
    let invoke = json!({"name": "file_write", "arguments": valid});
    assert_eq!(verbs.undeclared(&call(INVOKE_VERB, invoke)), None);
    assert_eq!(verbs.undeclared(&call("file_write", valid.clone())), None);
    let written = flat.call(&call("file_write", valid));
    assert!(!written.failed, "{}", text(&written));
    assert_eq!(guarded(root), "after\n");
}

#[test]
fn every_closed_schema_either_surface_publishes_refuses_an_undeclared_field_by_name() {
    let dir = crate_tree();
    let root = dir.path();

    let mut flat = Flat::new(catalogue(root));
    let specs: Vec<ToolSpec> = flat.specs().to_vec();
    let mut covered = Vec::new();
    let mut open = Vec::new();
    for spec in &specs {
        let probed = walk(&mut flat, spec, "flat");
        if closed(&spec.input_schema) {
            covered.push(format!("flat {} ({probed} probes)", spec.name.as_str()));
        } else {
            open.push(format!("flat {}", spec.name.as_str()));
        }
    }

    let mut verbs = Verbs::new(catalogue(root));
    for spec in verbs.specs().to_vec() {
        let probed = walk(&mut verbs, &spec, "verb");
        if closed(&spec.input_schema) {
            covered.push(format!("verb {} ({probed} probes)", spec.name.as_str()));
        } else {
            open.push(format!("verb {}", spec.name.as_str()));
        }
    }
    for spec in verbs.reachable_specs() {
        let name = spec.name.as_str().to_owned();
        let probes = probes(&spec.input_schema);
        for (path, accepted, arguments) in &probes {
            let invoke = call(INVOKE_VERB, json!({"name": name, "arguments": arguments}));
            let bare = call(&name, arguments.clone());
            assert_answered_ahead(
                &verbs,
                &invoke,
                &format!("tool_invoke {name}"),
                path,
                accepted,
            );
            assert_answered_ahead(&verbs, &bare, &format!("bare {name}"), path, accepted);
            let outcome = verbs.call(&invoke);
            assert_refused_naming(&outcome, &format!("tool_invoke {name}"), path, accepted);
            let outcome = verbs.call_batch(std::slice::from_ref(&invoke), None);
            assert_refused_naming(
                &outcome[0],
                &format!("tool_invoke batch {name}"),
                path,
                accepted,
            );
            let outcome = verbs.call(&call(&name, arguments.clone()));
            assert_refused_naming(&outcome, &format!("bare {name}"), path, accepted);
        }
        if closed(&spec.input_schema) {
            covered.push(format!("tool_invoke {name} ({} probes)", probes.len()));
        } else {
            open.push(format!("tool_invoke {name}"));
        }
    }
    assert_eq!(guarded(root), BEFORE, "no probe wrote anything");

    // The walk is only worth its green if it walked what it says it did.
    for wanted in [
        "flat file_write",
        "flat file_edit",
        "flat run",
        "flat rust_test",
        "flat toolchain_test",
        "verb tool_invoke",
        "tool_invoke file_write",
        "tool_invoke toolchain_test",
    ] {
        assert!(
            covered
                .iter()
                .any(|entry| entry.starts_with(&format!("{wanted} "))),
            "`{wanted}` was not walked: {covered:?}"
        );
    }
    eprintln!("closed, walked: {covered:#?}");
    eprintln!("not closed, accepting extra fields as before: {open:#?}");
}

#[test]
fn a_tool_invoke_carrying_an_undeclared_field_beside_a_valid_call_is_refused_alone_and_batched() {
    // The verb's own schema is closed too. In a batch the invocation is routed straight to its
    // entry, so a check made only on the single-call path would let this through there.
    let dir = crate_tree();
    let mut verbs = Verbs::new(catalogue(dir.path()));
    let invoke = call(
        INVOKE_VERB,
        json!({"name": "file_read", "arguments": {"path": "notes.txt"}, PROBE: true}),
    );
    let accepted = ["name".to_owned(), "arguments".to_owned()];
    assert_answered_ahead(&verbs, &invoke, "tool_invoke", PROBE, &accepted);
    let alone = verbs.call(&invoke);
    assert_refused_naming(&alone, "tool_invoke", PROBE, &accepted);
    let batch = verbs.call_batch(&[invoke.clone(), invoke], None);
    assert_eq!(batch.len(), 2);
    for outcome in &batch {
        assert_refused_naming(outcome, "tool_invoke batch", PROBE, &accepted);
    }
}
