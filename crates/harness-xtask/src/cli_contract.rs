//! Independent validation and generation of immutable command-line contracts.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

const FIRST_FULL: &str = "2026-08-30.2";
const ARGUMENT_KEYS: &[&str] = &[
    "conflicts_with",
    "default",
    "long",
    "required",
    "requires",
    "short",
    "takes_value",
    "value_name",
];
const POSITIONAL_KEYS: &[&str] = &["multiple", "name", "required"];

pub fn check(root: &Path) -> Result<(), String> {
    let contracts = root.join("contracts/cli");
    let mut failures = Vec::new();
    let mut versions = 0_u64;
    for product in directories(&contracts)? {
        for version in directories(&product)? {
            versions += 1;
            check_version(root, &version, &mut failures);
        }
    }
    if versions == 0 {
        failures.push(format!(
            "no command-line contracts under `{}`",
            contracts.display()
        ));
    }
    if failures.is_empty() {
        println!("command line: {versions} pinned version(s) verified");
        Ok(())
    } else {
        Err(format!(
            "{} command-line contract failure(s):\n{}",
            failures.len(),
            failures.join("\n")
        ))
    }
}

pub fn self_test() -> Result<(), String> {
    let clean = json!({
        "product":"b10x-harness",
        "subcommands":["run"],
        "arguments":{"run":[{
            "conflicts_with":[], "default":Value::Null, "long":"--profile",
            "required":false, "requires":[], "short":"-p", "takes_value":true,
            "value_name":"PROFILE"
        }]},
        "positionals":{"run":[{"multiple":false,"name":"PROMPT","required":false}]}
    });
    let mut cases = Vec::new();
    cases.push(("clean", clean.clone(), false));
    let mut missing_short = clean.clone();
    missing_short["arguments"]["run"][0]
        .as_object_mut()
        .expect("row")
        .remove("short");
    cases.push(("missing short", missing_short, true));
    let mut bad_short = clean.clone();
    bad_short["arguments"]["run"][0]["short"] = json!("--p");
    cases.push(("bad short", bad_short, true));
    let mut bare_placeholder = clean.clone();
    bare_placeholder["arguments"]["run"][0]["takes_value"] = json!(false);
    cases.push(("bare placeholder", bare_placeholder, true));
    let mut absent_conflict = clean.clone();
    absent_conflict["arguments"]["run"][0]["conflicts_with"] = json!(["--absent"]);
    cases.push(("absent conflict", absent_conflict, true));
    let mut no_positionals = clean.clone();
    no_positionals
        .as_object_mut()
        .expect("document")
        .remove("positionals");
    cases.push(("missing positionals", no_positionals, true));

    let mut failed = Vec::new();
    for (name, value, should_fail) in cases {
        let mut failures = Vec::new();
        validate_argv("self-test", &value, FIRST_FULL, &mut failures);
        if failures.is_empty() == should_fail {
            failed.push(format!("{name}: {failures:?}"));
        }
    }
    if failed.is_empty() {
        println!("command line: self-test green, 6 case(s)");
        Ok(())
    } else {
        Err(format!(
            "command-line self-test failed:\n{}",
            failed.join("\n")
        ))
    }
}

pub fn pin(root: &Path, version: &str) -> Result<(), String> {
    if !valid_version(version) {
        return Err(format!("`{version}` is not a date-based contract version"));
    }
    let product = root.join("contracts/cli/b10x-harness");
    let target = product.join(version);
    if target.exists() {
        return Err(format!(
            "`{}` already exists and is immutable",
            target.display()
        ));
    }
    std::fs::create_dir(&target)
        .map_err(|error| format!("creating `{}`: {error}", target.display()))?;
    let argv = harness_cli::contract::argv();
    std::fs::write(target.join("argv.json"), &argv)
        .map_err(|error| format!("writing argv contract: {error}"))?;
    let digest = sha256(argv.as_bytes());
    let document: Value = serde_json::from_str(&argv).map_err(|error| error.to_string())?;
    let manifest = json!({
        "product":"b10x-harness",
        "version":version,
        "interface":"argv",
        "generated_from":"clap::CommandFactory::command()",
        "subcommands":document["subcommands"],
        "files":[{"path":"argv.json","bytes":argv.len(),"sha256":digest}],
    });
    let mut manifest_text =
        serde_json::to_string_pretty(&manifest).map_err(|error| error.to_string())?;
    manifest_text.push('\n');
    std::fs::write(target.join("manifest.json"), manifest_text)
        .map_err(|error| format!("writing manifest: {error}"))?;

    let previous = harness_cli::contract::ARGV_CONTRACT_VERSION;
    let previous_readme = std::fs::read_to_string(product.join(previous).join("README.md"))
        .map_err(|error| format!("reading previous README: {error}"))?;
    let readme = previous_readme.replace(previous, version).replace(
        "## What changed since `2026-08-30.1`",
        &format!("## What changed since `{previous}`"),
    );
    std::fs::write(target.join("README.md"), readme)
        .map_err(|error| format!("writing README: {error}"))?;
    println!("generated `{}`", target.display());
    Ok(())
}

fn check_version(root: &Path, directory: &Path, failures: &mut Vec<String>) {
    let manifest_path = directory.join("manifest.json");
    let Ok(body) = std::fs::read(&manifest_path) else {
        failures.push(format!("{}: no manifest", directory.display()));
        return;
    };
    let Ok(manifest) = serde_json::from_slice::<Value>(&body) else {
        failures.push(format!("{}: manifest is not JSON", directory.display()));
        return;
    };
    for key in [
        "files",
        "generated_from",
        "interface",
        "product",
        "subcommands",
        "version",
    ] {
        if manifest.get(key).is_none() {
            failures.push(format!("{}: missing `{key}`", manifest_path.display()));
        }
    }
    let product = directory
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let version = directory
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if manifest["product"] != product || !product.starts_with("b10x") {
        failures.push(format!(
            "{}: product does not match",
            manifest_path.display()
        ));
    }
    if manifest["version"] != version {
        failures.push(format!(
            "{}: version does not match",
            manifest_path.display()
        ));
    }
    check_files(directory, &manifest, failures);
    let argv_path = directory.join("argv.json");
    match std::fs::read(&argv_path)
        .ok()
        .and_then(|body| serde_json::from_slice::<Value>(&body).ok())
    {
        Some(argv) => {
            validate_argv(&argv_path.display().to_string(), &argv, version, failures);
            if argv["product"] != manifest["product"]
                || argv["subcommands"] != manifest["subcommands"]
            {
                failures.push(format!("{}: differs from manifest", argv_path.display()));
            }
        }
        None => failures.push(format!("{}: absent or not JSON", argv_path.display())),
    }
    check_immutable(root, directory, failures);
}

#[allow(
    clippy::too_many_lines,
    reason = "the independent validator keeps every pinned argv field in one exhaustive pass"
)]
fn validate_argv(label: &str, argv: &Value, version: &str, failures: &mut Vec<String>) {
    let Some(arguments) = argv["arguments"].as_object() else {
        failures.push(format!("{label}: `arguments` is not an object"));
        return;
    };
    let declared = argv["subcommands"].as_array().cloned().unwrap_or_default();
    let declared_names: BTreeSet<&str> = declared.iter().filter_map(Value::as_str).collect();
    if !is_sorted(declared.iter().filter_map(Value::as_str)) {
        failures.push(format!("{label}: subcommands are not sorted"));
    }
    for name in &declared_names {
        if !arguments.contains_key(*name) {
            failures.push(format!("{label}: `{name}` has no arguments"));
        }
    }
    let full = cut_order(version) >= cut_order(FIRST_FULL);
    for (command, rows) in arguments {
        let Some(rows) = rows.as_array() else {
            failures.push(format!("{label}: `{command}` arguments are not an array"));
            continue;
        };
        let longs: BTreeSet<&str> = rows.iter().filter_map(|row| row["long"].as_str()).collect();
        if !is_sorted(rows.iter().filter_map(|row| row["long"].as_str())) {
            failures.push(format!("{label}: `{command}` arguments are not sorted"));
        }
        if longs.len() != rows.len() {
            failures.push(format!("{label}: `{command}` repeats or omits a long flag"));
        }
        for row in rows {
            let Some(object) = row.as_object() else {
                failures.push(format!("{label}: `{command}` has a non-object row"));
                continue;
            };
            for key in ARGUMENT_KEYS {
                if (*key != "short" || full) && !object.contains_key(*key) {
                    failures.push(format!("{label}: `{command}` row missing `{key}`"));
                }
            }
            let long = row["long"].as_str().unwrap_or_default();
            if !long.starts_with("--") {
                failures.push(format!("{label}: `{command}` `{long}` is not a long flag"));
            }
            if row["default"] != Value::Null && row["takes_value"] != Value::Bool(true) {
                failures.push(format!(
                    "{label}: `{command}` `{long}` has an unused default"
                ));
            }
            if full {
                if let Some(short) = row["short"].as_str() {
                    if short.len() != 2 || !short.starts_with('-') {
                        failures.push(format!("{label}: `{command}` `{long}` has invalid short"));
                    }
                } else if !row["short"].is_null() {
                    failures.push(format!("{label}: `{command}` `{long}` has invalid short"));
                }
                if row["takes_value"] == Value::Bool(false) && !row["value_name"].is_null() {
                    failures.push(format!(
                        "{label}: `{command}` `{long}` bare flag has placeholder"
                    ));
                }
            }
            for key in ["conflicts_with", "requires"] {
                let Some(list) = row[key].as_array() else {
                    failures.push(format!(
                        "{label}: `{command}` `{long}` `{key}` is not a list"
                    ));
                    continue;
                };
                let names: Vec<&str> = list.iter().filter_map(Value::as_str).collect();
                if names.len() != list.len() || !is_sorted(names.iter().copied()) {
                    failures.push(format!("{label}: `{command}` `{long}` `{key}` is invalid"));
                }
                for name in names {
                    if name == long || !longs.contains(name) {
                        failures.push(format!(
                            "{label}: `{command}` `{long}` `{key}` names non-flag `{name}`"
                        ));
                    }
                }
            }
        }
    }
    if full {
        let Some(positionals) = argv["positionals"].as_object() else {
            failures.push(format!("{label}: `positionals` is not an object"));
            return;
        };
        if positionals.keys().collect::<BTreeSet<_>>() != arguments.keys().collect::<BTreeSet<_>>()
        {
            failures.push(format!("{label}: positional command set differs"));
        }
        for (command, rows) in positionals {
            let Some(rows) = rows.as_array() else {
                failures.push(format!("{label}: `{command}` positionals are not an array"));
                continue;
            };
            for row in rows {
                let Some(object) = row.as_object() else {
                    failures.push(format!("{label}: `{command}` positional is not an object"));
                    continue;
                };
                for key in POSITIONAL_KEYS {
                    if !object.contains_key(*key) {
                        failures.push(format!("{label}: `{command}` positional missing `{key}`"));
                    }
                }
                if row["name"].as_str().is_none_or(str::is_empty)
                    || !row["required"].is_boolean()
                    || !row["multiple"].is_boolean()
                {
                    failures.push(format!(
                        "{label}: `{command}` positional has invalid values"
                    ));
                }
            }
        }
    }
}

fn check_files(directory: &Path, manifest: &Value, failures: &mut Vec<String>) {
    let Some(entries) = manifest["files"].as_array() else {
        failures.push(format!("{}: files is not an array", directory.display()));
        return;
    };
    let present: BTreeSet<String> = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter(|path| path.file_name().is_some_and(|name| name != "manifest.json"))
        .filter_map(|path| path.file_name()?.to_str().map(ToOwned::to_owned))
        .collect();
    let recorded: BTreeSet<String> = entries
        .iter()
        .filter_map(|entry| entry["path"].as_str().map(ToOwned::to_owned))
        .collect();
    if present != recorded {
        failures.push(format!(
            "{}: recorded files differ from disk",
            directory.display()
        ));
    }
    for entry in entries {
        let Some(relative) = entry["path"].as_str() else {
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

fn check_immutable(root: &Path, directory: &Path, failures: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        if !path.is_file() {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let spec = format!("origin/main:{}", relative.to_string_lossy());
        if !Command::new("git")
            .current_dir(root)
            .args(["cat-file", "-e", &spec])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
        {
            continue;
        }
        let Ok(base) = Command::new("git")
            .current_dir(root)
            .args(["show", &spec])
            .output()
        else {
            failures.push(format!("cannot read `{spec}`"));
            continue;
        };
        if std::fs::read(&path).ok().as_deref() != Some(base.stdout.as_slice()) {
            failures.push(format!("{}: released contract changed", relative.display()));
        }
    }
}

fn cut_order(version: &str) -> (&str, u64) {
    version
        .rsplit_once('.')
        .map_or((version, 0), |(day, suffix)| {
            suffix
                .parse::<u64>()
                .map_or((version, 0), |number| (day, number))
        })
}

fn valid_version(version: &str) -> bool {
    let (day, _) = cut_order(version);
    let bytes = day.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

fn is_sorted<'a>(values: impl Iterator<Item = &'a str>) -> bool {
    let values: Vec<&str> = values.collect();
    values.windows(2).all(|pair| pair[0] <= pair[1])
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
