//! Binds the server's own inventory to the pinned profile.
//!
//! The Python checker proves the trace agrees with the manifest. This proves the *code* agrees with
//! the manifest. Without both, a constant can drift from the contract that documents it and nothing
//! notices until a bridge stops driving this server.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use b10x_harness_app_server::{
    CLIENT_METHODS, DYNAMIC_TOOL_ITEM, PRODUCT, PROFILE, REFUSED_CLIENT_METHODS, SERVER_METHODS,
    TERMINAL_STATUSES,
};
use serde_json::Value;

const VERSION: &str = "2026-08-21";

fn manifest() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("contracts")
        .join("app-server-profile")
        .join(PROFILE)
        .join(VERSION)
        .join("manifest.json");
    serde_json::from_str(
        &fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("reading `{}`: {error}", path.display())),
    )
    .expect("the manifest is JSON")
}

fn strings(value: &Value, key: &str) -> BTreeSet<String> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("`{key}` is an array"))
        .iter()
        .map(|entry| entry.as_str().expect("a string").to_owned())
        .collect()
}

fn from(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|entry| (*entry).to_owned()).collect()
}

#[test]
fn the_served_and_refused_client_methods_match_the_manifest() {
    let manifest = manifest();
    assert_eq!(from(CLIENT_METHODS), strings(&manifest, "client_methods"));
    assert_eq!(
        from(REFUSED_CLIENT_METHODS),
        strings(&manifest, "refused_client_methods")
    );
}

#[test]
fn the_emitted_methods_match_the_manifest() {
    assert_eq!(from(SERVER_METHODS), strings(&manifest(), "server_methods"));
}

#[test]
fn the_terminal_statuses_and_tool_item_match_the_manifest() {
    let manifest = manifest();
    assert_eq!(
        from(TERMINAL_STATUSES),
        strings(&manifest, "terminal_statuses")
    );
    assert_eq!(manifest["dynamic_tool_item"], DYNAMIC_TOOL_ITEM);
}

#[test]
fn the_manifest_names_this_implementation_rather_than_the_vendor() {
    let manifest = manifest();
    assert_eq!(manifest["product"], PRODUCT);
    assert_eq!(manifest["profile"], PROFILE);
    assert_ne!(
        manifest["product"], "codex-cli",
        "a server that impersonates the vendor makes an incident unreadable"
    );
}

#[test]
fn every_server_frame_in_the_pinned_trace_is_one_this_server_may_emit() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("contracts")
        .join("app-server-profile")
        .join(PROFILE)
        .join(VERSION)
        .join("fixtures")
        .join("walking-trace.jsonl");
    let trace = fs::read_to_string(&path).expect("the trace is readable");
    let mut seen = BTreeSet::new();
    for line in trace.lines() {
        let entry: Value = serde_json::from_str(line).expect("each line is JSON");
        if entry["direction"] != "server" {
            continue;
        }
        if let Some(method) = entry["frame"]["method"].as_str() {
            assert!(
                SERVER_METHODS.contains(&method),
                "the trace carries `{method}`, which this server does not emit"
            );
            seen.insert(method.to_owned());
        }
    }
    assert_eq!(
        seen,
        from(SERVER_METHODS),
        "the trace must exercise every method this server can emit"
    );
}
