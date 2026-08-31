//! Repository-owned gates and contract checkers.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use clap::{Parser, Subcommand};

mod cli_contract;
mod provider_contract;
mod website_contract;

#[derive(Debug, Parser)]
#[command(name = "cargo xtask")]
struct Args {
    #[command(subcommand)]
    command: Task,
}

#[derive(Debug, Subcommand)]
enum Task {
    /// Run the repository's complete local gate.
    Gate,
    /// Verify every immutable provider-wire contract.
    ProviderContracts,
    /// Verify every immutable command-line contract.
    CliContract {
        /// Prove the checker rejects planted defects.
        #[arg(long)]
        self_test: bool,
    },
    /// Verify the public website against shipped versions and the generated CLI surface.
    WebsiteContract,
    /// Verify built-in toolchain specs, or prove the checker rejects planted defects.
    ToolchainSpecs {
        #[arg(long)]
        self_test: bool,
    },
    /// Generate the built-in toolchain reference, or check its committed bytes.
    ToolchainDocs {
        #[arg(long)]
        check: bool,
    },
    /// Generate a new immutable command-line contract from clap.
    PinCli {
        /// Date-based contract version, for example `2026-08-31`.
        version: String,
    },
}

fn main() -> ExitCode {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("xtask is two levels below the repository")
        .to_owned();
    let result = match Args::parse().command {
        Task::Gate => gate(&root),
        Task::ProviderContracts => provider_contract::check(&root),
        Task::CliContract { self_test: true } => cli_contract::self_test(),
        Task::CliContract { self_test: false } => cli_contract::check(&root),
        Task::WebsiteContract => website_contract::check(&root),
        Task::ToolchainSpecs { self_test } => toolchain_specs(self_test),
        Task::ToolchainDocs { check } => toolchain_docs(&root, check),
        Task::PinCli { version } => cli_contract::pin(&root, &version),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn gate(root: &Path) -> Result<(), String> {
    run(root, "cargo", &["test", "--workspace", "--locked"])?;
    run(
        root,
        "cargo",
        &[
            "test",
            "-p",
            "b10x-harness-substrate",
            "--locked",
            "--test",
            "conformance",
        ],
    )?;
    run(root, "cargo", &["fmt", "--all", "--check"])?;
    run(
        root,
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )?;

    provider_contract::check(root)?;
    cli_contract::self_test()?;
    cli_contract::check(root)?;
    website_contract::check(root)?;
    toolchain_specs(true)?;
    toolchain_specs(false)?;
    toolchain_docs(root, true)?;
    check_http_boundary(root)?;

    // These two pre-existing checkers were not changed by this wave. They remain until their own
    // next material change; the gate itself and the two changed contract checkers are Rust now.
    run(root, "python3", &["scripts/check-app-server-profile.py"])?;
    run(
        root,
        "python3",
        &["scripts/check-no-home-paths.py", "--self-test"],
    )?;
    run(root, "python3", &["scripts/check-no-home-paths.py"])?;

    let mut docs = Command::new("cargo");
    docs.current_dir(root)
        .env("RUSTDOCFLAGS", "-D warnings")
        .args(["doc", "--workspace", "--no-deps", "--locked"]);
    run_command("strict rustdoc", &mut docs)?;
    println!("gate: green");
    Ok(())
}

fn toolchain_specs(self_test: bool) -> Result<(), String> {
    if !self_test {
        let registry = harness_toolchain::Registry::builtins()?;
        println!(
            "toolchain specs: {} built-ins valid",
            registry.names().len()
        );
        return Ok(());
    }
    let bad_version = "version: 2\ntoolchains: []\n";
    if harness_toolchain::Registry::validate(bad_version, "planted-version.yaml").is_ok() {
        return Err("toolchain spec self-test accepted an unsupported version".to_owned());
    }
    let unknown = "version: 1\nunknown: true\ntoolchains: []\n";
    if harness_toolchain::Registry::validate(unknown, "planted-unknown.yaml").is_ok() {
        return Err("toolchain spec self-test accepted an unknown field".to_owned());
    }
    let collision = "version: 1\ntoolchains:\n  - &p\n    name: duplicate\n    description: first\n    tools: [{name: duplicate_test, description: test, commands: [{argv: [\"true\"]}]}]\n    sandbox: {programs: [\"true\"]}\n  - *p\n";
    if harness_toolchain::Registry::validate(collision, "planted-collision.yaml").is_ok() {
        return Err("toolchain spec self-test accepted a provider collision".to_owned());
    }
    println!("toolchain spec checker self-test: rejected planted defects");
    Ok(())
}

fn toolchain_docs(root: &Path, check: bool) -> Result<(), String> {
    let path = root.join("website/docs/reference/toolchains.md");
    let expected = harness_toolchain::builtin_reference()?;
    if check {
        let actual = std::fs::read_to_string(&path)
            .map_err(|error| format!("reading generated `{}`: {error}", path.display()))?;
        if actual != expected {
            return Err(format!(
                "`{}` is stale; run `cargo xtask toolchain-docs`",
                path.display()
            ));
        }
        println!("toolchain docs: current");
        return Ok(());
    }
    std::fs::write(&path, expected)
        .map_err(|error| format!("writing generated `{}`: {error}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

fn run(root: &Path, program: &str, arguments: &[&str]) -> Result<(), String> {
    let mut command = Command::new(program);
    command.current_dir(root).args(arguments);
    run_command(&format!("{program} {}", arguments.join(" ")), &mut command)
}

fn run_command(label: &str, command: &mut Command) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|error| format!("starting `{label}`: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{label}` failed with {status}"))
    }
}

fn check_http_boundary(root: &Path) -> Result<(), String> {
    const FORBIDDEN: &[&str] = &[
        "anthropic",
        "openai",
        "harness-messages",
        "harness-responses",
        "oauth",
        "bearer",
        "authorization",
        "access_token",
        "refresh_token",
        "api-key",
        "x-api-key",
    ];
    let directory = root.join("crates/harness-http");
    let mut failures = Vec::new();
    for file in files_under(&directory)? {
        let body = std::fs::read_to_string(&file)
            .map_err(|error| format!("reading `{}`: {error}", file.display()))?;
        let lower = body.to_ascii_lowercase();
        for forbidden in FORBIDDEN {
            if lower.contains(forbidden) {
                failures.push(format!(
                    "{} names forbidden route semantics `{forbidden}`",
                    file.strip_prefix(root).unwrap_or(&file).display()
                ));
            }
        }
    }
    if failures.is_empty() {
        println!("generic HTTP boundary: clean");
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

fn files_under(directory: &Path) -> Result<Vec<PathBuf>, String> {
    fn visit(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in std::fs::read_dir(directory)
            .map_err(|error| format!("reading `{}`: {error}", directory.display()))?
        {
            let entry = entry.map_err(|error| format!("reading directory entry: {error}"))?;
            let path = entry.path();
            if path.is_dir() {
                visit(&path, files)?;
            } else if path.extension().is_some_and(|extension| extension == "rs") {
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
