//! Independent validation of immutable provider-wire contracts.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use sha2::{Digest as _, Sha256};

const REQUIRED: &[&str] = &[
    "conformance",
    "endpoint",
    "files",
    "output_items",
    "request_fields",
    "stateful",
    "stream_events",
    "streaming",
    "transport",
    "version",
    "wire",
];

pub fn check(root: &Path) -> Result<(), String> {
    let contracts = root.join("contracts/provider-wires");
    let mut failures = Vec::new();
    let mut versions = 0_u64;
    for wire in directories(&contracts)? {
        for version in directories(&wire)? {
            versions += 1;
            check_version(root, &version, &mut failures);
        }
    }
    if versions == 0 {
        failures.push(format!(
            "no provider-wire versions under `{}`",
            contracts.display()
        ));
    }
    if failures.is_empty() {
        println!("provider-wire contracts: {versions} pinned version(s) verified");
        Ok(())
    } else {
        Err(format!(
            "{} provider-wire contract failure(s):\n{}",
            failures.len(),
            failures.join("\n")
        ))
    }
}

fn check_version(root: &Path, directory: &Path, failures: &mut Vec<String>) {
    let manifest_path = directory.join("manifest.json");
    let Ok(body) = std::fs::read(&manifest_path) else {
        failures.push(format!("{}: no manifest.json", directory.display()));
        return;
    };
    let Ok(manifest) = serde_json::from_slice::<Value>(&body) else {
        failures.push(format!("{}: not JSON", manifest_path.display()));
        return;
    };
    for key in REQUIRED {
        if manifest.get(key).is_none() {
            failures.push(format!("{}: missing `{key}`", manifest_path.display()));
        }
    }
    let wire = directory
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let version = directory
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if manifest["wire"] != wire {
        failures.push(format!(
            "{}: wire does not match directory",
            manifest_path.display()
        ));
    }
    if manifest["version"] != version {
        failures.push(format!(
            "{}: version does not match directory",
            manifest_path.display()
        ));
    }
    if !matches!(
        manifest["conformance"].as_str(),
        Some("provider_emulated" | "vendor_live")
    ) {
        failures.push(format!(
            "{}: invalid conformance class",
            manifest_path.display()
        ));
    }
    check_files(directory, &manifest, failures);
    check_stream(directory, &manifest, version >= "2026-08-31", failures);
    if version >= "2026-08-31" {
        for key in ["request_encoding", "request_headers", "terminal_sentinel"] {
            if manifest.get(key).is_none() {
                failures.push(format!(
                    "{}: new cut missing `{key}`",
                    manifest_path.display()
                ));
            }
        }
        check_inventory(directory, &manifest, failures);
    }
    check_immutable(root, directory, failures);
}

fn check_files(directory: &Path, manifest: &Value, failures: &mut Vec<String>) {
    let fixtures = directory.join("fixtures");
    let fixture_files = match files(&fixtures) {
        Ok(files) => files,
        Err(error) => {
            failures.push(error);
            return;
        }
    };
    let present: BTreeSet<String> = fixture_files
        .into_iter()
        .filter_map(|path| {
            path.strip_prefix(directory)
                .ok()
                .map(|path| path.to_string_lossy().into_owned())
        })
        .collect();
    let Some(entries) = manifest["files"].as_array() else {
        failures.push(format!("{}: `files` is not an array", directory.display()));
        return;
    };
    let recorded: BTreeSet<String> = entries
        .iter()
        .filter_map(|entry| entry["path"].as_str().map(ToOwned::to_owned))
        .collect();
    for missing in recorded.difference(&present) {
        failures.push(format!(
            "{}: recorded `{missing}` is missing",
            directory.display()
        ));
    }
    for unrecorded in present.difference(&recorded) {
        failures.push(format!(
            "{}: `{unrecorded}` is not recorded",
            directory.display()
        ));
    }
    for entry in entries {
        let Some(relative) = entry["path"].as_str() else {
            failures.push(format!("{}: a file has no path", directory.display()));
            continue;
        };
        let path = directory.join(relative);
        let Ok(body) = std::fs::read(&path) else {
            continue;
        };
        if entry["bytes"].as_u64() != u64::try_from(body.len()).ok() {
            failures.push(format!("{}: byte count differs", path.display()));
        }
        let digest = sha256(&body);
        if entry["sha256"].as_str() != Some(&digest) {
            failures.push(format!("{}: sha256 differs", path.display()));
        }
    }
}

fn check_stream(directory: &Path, manifest: &Value, subset: bool, failures: &mut Vec<String>) {
    let path = directory.join("fixtures/turn-stream.sse");
    let Ok(body) = std::fs::read_to_string(&path) else {
        failures.push(format!("{}: no turn-stream.sse", directory.display()));
        return;
    };
    let mut seen = BTreeSet::new();
    for (index, line) in body.lines().enumerate() {
        let Some(payload) = line.strip_prefix("data: ") else {
            continue;
        };
        if payload == "[DONE]" {
            continue;
        }
        let Ok(event) = serde_json::from_str::<Value>(payload) else {
            failures.push(format!(
                "{}:{}: event is not JSON",
                path.display(),
                index + 1
            ));
            continue;
        };
        if let Some(kind) = event["type"].as_str() {
            seen.insert(kind.to_owned());
        } else {
            failures.push(format!(
                "{}:{}: event has no type",
                path.display(),
                index + 1
            ));
        }
    }
    let declared = strings(&manifest["stream_events"]);
    for unknown in seen.difference(&declared) {
        failures.push(format!(
            "{}: event `{unknown}` is undeclared",
            path.display()
        ));
    }
    if !subset {
        for absent in declared.difference(&seen) {
            failures.push(format!(
                "{}: event `{absent}` is not exercised",
                path.display()
            ));
        }
    }
}

fn check_inventory(directory: &Path, manifest: &Value, failures: &mut Vec<String>) {
    let path = directory.join("fixtures/accepted-events.json");
    let Ok(body) = std::fs::read(&path) else {
        failures.push(format!("{}: no accepted-events.json", directory.display()));
        return;
    };
    let Ok(inventory) = serde_json::from_slice::<Value>(&body) else {
        failures.push(format!("{}: inventory is not JSON", path.display()));
        return;
    };
    if inventory["stream_events"] != manifest["stream_events"] {
        failures.push(format!(
            "{}: stream inventory differs from manifest",
            path.display()
        ));
    }
    if manifest.get("content_block_deltas").is_some()
        && inventory["content_block_deltas"] != manifest["content_block_deltas"]
    {
        failures.push(format!(
            "{}: delta inventory differs from manifest",
            path.display()
        ));
    }
    for key in ["stream_events", "content_block_deltas"] {
        if let Some(values) = manifest.get(key).and_then(Value::as_array) {
            let strings: Vec<&str> = values.iter().filter_map(Value::as_str).collect();
            let mut sorted = strings.clone();
            sorted.sort_unstable();
            if strings != sorted {
                failures.push(format!("{}: `{key}` is not sorted", directory.display()));
            }
        }
    }
}

fn check_immutable(root: &Path, directory: &Path, failures: &mut Vec<String>) {
    let Ok(paths) = files(directory) else {
        return;
    };
    for path in paths {
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let spec = format!("origin/main:{}", relative.to_string_lossy());
        let exists = Command::new("git")
            .current_dir(root)
            .args(["cat-file", "-e", &spec])
            .stderr(Stdio::null())
            .status();
        if !matches!(exists, Ok(status) if status.success()) {
            continue;
        }
        let Ok(output) = Command::new("git")
            .current_dir(root)
            .args(["show", &spec])
            .output()
        else {
            failures.push(format!("cannot read immutable `{spec}`"));
            continue;
        };
        if std::fs::read(&path).ok().as_deref() != Some(output.stdout.as_slice()) {
            failures.push(format!(
                "{}: released contract differs from origin/main",
                relative.display()
            ));
        }
    }
}

fn strings(value: &Value) -> BTreeSet<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn sha256(body: &[u8]) -> String {
    let mut encoded = String::with_capacity(64);
    for byte in Sha256::digest(body) {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn directories(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    for entry in
        std::fs::read_dir(root).map_err(|error| format!("reading `{}`: {error}", root.display()))?
    {
        let path = entry
            .map_err(|error| format!("reading directory entry: {error}"))?
            .path();
        if path.is_dir() {
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}

fn files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    for entry in
        std::fs::read_dir(root).map_err(|error| format!("reading `{}`: {error}", root.display()))?
    {
        let path = entry
            .map_err(|error| format!("reading directory entry: {error}"))?
            .path();
        if path.is_dir() {
            found.extend(files(&path)?);
        } else {
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn planted_contract() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().expect("a root");
        let directory = root
            .path()
            .join("contracts/provider-wires/example-wire/2026-08-31");
        let fixtures = directory.join("fixtures");
        std::fs::create_dir_all(&fixtures).expect("fixture directory");
        let stream = b"data: {\"type\":\"event.a\"}\n\n";
        let inventory = b"{\"stream_events\":[\"event.a\"]}\n";
        std::fs::write(fixtures.join("turn-stream.sse"), stream).expect("stream");
        std::fs::write(fixtures.join("accepted-events.json"), inventory).expect("inventory");
        let manifest = json!({
            "version": "2026-08-31",
            "wire": "example-wire",
            "conformance": "provider_emulated",
            "endpoint": "/turn",
            "transport": "sse",
            "streaming": true,
            "stateful": false,
            "request_fields": [],
            "request_encoding": "compact JSON followed by LF",
            "request_headers": {},
            "terminal_sentinel": null,
            "output_items": [],
            "stream_events": ["event.a"],
            "files": [
                {
                    "path": "fixtures/accepted-events.json",
                    "bytes": inventory.len(),
                    "sha256": sha256(inventory),
                },
                {
                    "path": "fixtures/turn-stream.sse",
                    "bytes": stream.len(),
                    "sha256": sha256(stream),
                }
            ],
        });
        std::fs::write(
            directory.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).expect("manifest"),
        )
        .expect("manifest file");
        (root, directory)
    }

    #[test]
    fn a_planted_digest_change_is_rejected() {
        let (root, directory) = planted_contract();
        std::fs::write(
            directory.join("fixtures/turn-stream.sse"),
            b"data: {\"type\":\"event.b\"}\n\n",
        )
        .expect("tampered stream");
        let mut failures = Vec::new();
        check_version(root.path(), &directory, &mut failures);
        assert!(
            failures
                .iter()
                .any(|failure| failure.contains("sha256 differs")),
            "the planted digest defect passed: {failures:?}"
        );
    }

    #[test]
    fn a_planted_inventory_mismatch_is_rejected() {
        let (root, directory) = planted_contract();
        let path = directory.join("fixtures/accepted-events.json");
        let changed = b"{\"stream_events\":[\"event.b\"]}\n";
        std::fs::write(&path, changed).expect("tampered inventory");
        let mut failures = Vec::new();
        check_version(root.path(), &directory, &mut failures);
        assert!(
            failures
                .iter()
                .any(|failure| failure.contains("stream inventory differs")),
            "the planted inventory defect passed: {failures:?}"
        );
    }
}
