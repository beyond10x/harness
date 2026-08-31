//! Drift checks for the curated public website.

use std::path::{Path, PathBuf};

use serde_json::Value;

const FORBIDDEN_PUBLIC_REFERENCES: &[&str] = &[
    "docs/design",
    ".engineering",
    "STATUS.md",
    "ROADMAP.md",
    "beyond10x/atlas",
    "../atlas",
];

/// Check the website against this build's generated contracts.
pub fn check(root: &Path) -> Result<(), String> {
    check_with(
        root,
        env!("CARGO_PKG_VERSION"),
        harness_cli::contract::ARGV_CONTRACT_VERSION,
        &harness_cli::contract::argv(),
    )?;
    println!("public website contract: clean");
    Ok(())
}

fn check_with(
    root: &Path,
    release_version: &str,
    cli_contract_version: &str,
    argv: &str,
) -> Result<(), String> {
    let docs = root.join("website/docs");
    let cli_path = docs.join("reference/cli.md");
    let cli = read(&cli_path)?;
    let index = read(&docs.join("index.md"))?;
    let status = read(&docs.join("status.md"))?;
    let contract: Value = serde_json::from_str(argv)
        .map_err(|error| format!("decoding generated argv contract: {error}"))?;
    let mut failures = Vec::new();

    require_token(
        &index,
        release_version,
        "website/docs/index.md does not name the workspace version",
        &mut failures,
    );
    require_token(
        &status,
        release_version,
        "website/docs/status.md does not name the workspace version",
        &mut failures,
    );
    require_token(
        &cli,
        cli_contract_version,
        "website/docs/reference/cli.md does not name the current CLI contract",
        &mut failures,
    );

    let arguments = contract
        .get("arguments")
        .and_then(Value::as_object)
        .ok_or_else(|| "generated argv contract has no arguments object".to_owned())?;
    for rows in arguments.values() {
        let rows = rows
            .as_array()
            .ok_or_else(|| "generated argv contract has a non-array argument set".to_owned())?;
        for row in rows {
            let flag = row
                .get("long")
                .and_then(Value::as_str)
                .ok_or_else(|| "generated argv contract has an argument without long".to_owned())?;
            require_code_token(
                &cli,
                flag,
                &format!("CLI reference does not name `{flag}`"),
                &mut failures,
            );
        }
    }

    let subcommands = contract
        .get("subcommands")
        .and_then(Value::as_array)
        .ok_or_else(|| "generated argv contract has no subcommands array".to_owned())?;
    for command in subcommands {
        let command = command
            .as_str()
            .ok_or_else(|| "generated argv contract has a non-string subcommand".to_owned())?;
        require_code_token(
            &cli,
            command,
            &format!("CLI reference does not name `{command}`"),
            &mut failures,
        );
    }

    for path in markdown_files(&docs)? {
        let body = read(&path)?;
        for forbidden in FORBIDDEN_PUBLIC_REFERENCES {
            if body.contains(forbidden) {
                let relative = path.strip_prefix(root).unwrap_or(&path);
                failures.push(format!(
                    "{} points at private or internal material `{forbidden}`",
                    relative.display()
                ));
            }
        }
    }

    failures.sort();
    failures.dedup();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

fn require_token(body: &str, token: &str, message: &str, failures: &mut Vec<String>) {
    if !body.contains(token) {
        failures.push(format!("{message}: expected `{token}`"));
    }
}

fn require_code_token(body: &str, token: &str, message: &str, failures: &mut Vec<String>) {
    if !body.contains(token) {
        failures.push(message.to_owned());
    }
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| format!("reading `{}`: {error}", path.display()))
}

fn markdown_files(directory: &Path) -> Result<Vec<PathBuf>, String> {
    fn visit(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in std::fs::read_dir(directory)
            .map_err(|error| format!("reading `{}`: {error}", directory.display()))?
        {
            let entry = entry.map_err(|error| format!("reading directory entry: {error}"))?;
            let path = entry.path();
            if path.is_dir() {
                visit(&path, files)?;
            } else if path.extension().is_some_and(|extension| extension == "md") {
                files.push(path);
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    visit(directory, &mut files)?;
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARGV: &str = r#"{
      "arguments": {"product": [{"long": "--model"}, {"long": "--driver"}]},
      "subcommands": ["run", "workflow run"]
    }"#;

    fn fixture(cli: &str, index: &str, status: &str) -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("temporary directory");
        let reference = root.path().join("website/docs/reference");
        std::fs::create_dir_all(&reference).expect("reference directory");
        std::fs::write(reference.join("cli.md"), cli).expect("CLI reference");
        std::fs::write(root.path().join("website/docs/index.md"), index).expect("index");
        std::fs::write(root.path().join("website/docs/status.md"), status).expect("status");
        root
    }

    #[test]
    fn accepts_matching_public_documentation() {
        let root = fixture(
            "contract-2 `--model` `--driver` `run` `workflow run`",
            "release-1",
            "release-1",
        );
        assert!(check_with(root.path(), "release-1", "contract-2", ARGV).is_ok());
    }

    #[test]
    fn rejects_a_missing_flag() {
        let root = fixture(
            "contract-2 `--model` `run` `workflow run`",
            "release-1",
            "release-1",
        );
        let error = check_with(root.path(), "release-1", "contract-2", ARGV)
            .expect_err("missing flag must fail");
        assert!(error.contains("--driver"));
    }

    #[test]
    fn rejects_a_stale_version() {
        let root = fixture(
            "contract-2 `--model` `--driver` `run` `workflow run`",
            "release-0",
            "release-1",
        );
        let error = check_with(root.path(), "release-1", "contract-2", ARGV)
            .expect_err("stale version must fail");
        assert!(error.contains("workspace version"));
    }

    #[test]
    fn rejects_an_internal_link() {
        let root = fixture(
            "contract-2 `--model` `--driver` `run` `workflow run`",
            "release-1 [internal](../../docs/design/0001.md)",
            "release-1",
        );
        let error = check_with(root.path(), "release-1", "contract-2", ARGV)
            .expect_err("internal link must fail");
        assert!(error.contains("docs/design"));
    }
}
