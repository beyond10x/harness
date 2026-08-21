#![forbid(unsafe_code)]

//! Daemonloom's own harness, driven from the command line.
//!
//! The command is a thin shell: it resolves an endpoint, a credential and a workspace, then hands
//! them to [`harness_loop`]. Everything interesting happens in the loop, which is what lets the
//! same core run embedded in another process or behind a bridge without a second implementation.

mod render;
mod workspace;

use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};
use harness_app_server::ServerConfig;
use harness_loop::{
    AgentLoop, ApprovalPort, ApproveAll, Budget, DenyAll, LoopCancel, LoopConfig, LoopStop,
};
use harness_responses::{Endpoint, ResponsesClient};
use harness_wire::{Sampling, StaticBearer};

pub use render::Renderer;
pub use workspace::{GREP_TOOL, LIST_TOOL, READ_TOOL, WorkspaceTools};

/// The standing instruction when the caller supplies none.
const DEFAULT_INSTRUCTIONS: &str = "\
You are Daemonloom's own coding harness. You are looking at one workspace through three read-only \
tools: workspace.list, workspace.read and workspace.grep. Nothing you can call changes a file or \
runs a command, so say what you would change rather than claiming you changed it. Ground every \
claim about the workspace in something you actually read, and say plainly when you have not \
looked. Prefer a few precise reads over broad guessing.";

#[derive(Debug, Parser)]
#[command(
    name = "daemonloom-harness",
    version,
    about = "Daemonloom's own agent loop, run against an OpenAI-compatible endpoint"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run one request to completion.
    Run(Box<RunOptions>),
    /// Print the tools this harness publishes, without contacting an endpoint.
    Tools(ToolsOptions),
    /// Serve one connection over the pinned Codex app-server JSON-RPC format, on stdio.
    ///
    /// Tools arrive from the client on `thread/start`; the workspace toolset is not published
    /// here. The endpoint and credential stay outside the protocol.
    AppServer(Box<AppServerOptions>),
}

#[derive(Debug, Args)]
struct AppServerOptions {
    /// Endpoint origin plus API prefix, for example `https://llmgw.example/v1`.
    #[arg(long)]
    base_url: String,
    /// Exact model identifier the endpoint serves.
    #[arg(long)]
    model: String,
    /// Context window the endpoint serves for that model.
    #[arg(long, default_value_t = 128_000)]
    context_window: u64,
    /// File holding the bearer credential. Its contents are trimmed and never logged.
    #[arg(long, conflicts_with = "api_key_env")]
    api_key_file: Option<PathBuf>,
    /// Name of an environment variable holding the bearer credential.
    #[arg(long, conflicts_with = "api_key_file")]
    api_key_env: Option<String>,
    /// Ceiling on model turns, applied to every turn on the connection.
    #[arg(long)]
    max_turns: Option<u64>,
    /// Ceiling on total reported output tokens per turn.
    #[arg(long)]
    max_output_tokens: Option<u64>,
}

#[derive(Debug, Args)]
struct RunOptions {
    /// Endpoint origin plus API prefix, for example `https://llmgw.example/v1`.
    #[arg(long)]
    base_url: String,
    /// Exact model identifier the endpoint serves.
    #[arg(long)]
    model: String,
    /// Context window the endpoint serves for that model.
    #[arg(long, default_value_t = 128_000)]
    context_window: u64,
    /// File holding the bearer credential. Its contents are trimmed and never logged.
    #[arg(long, conflicts_with = "api_key_env")]
    api_key_file: Option<PathBuf>,
    /// Name of an environment variable holding the bearer credential. Naming it is deliberate:
    /// the harness reads no credential it was not pointed at.
    #[arg(long, conflicts_with = "api_key_file")]
    api_key_env: Option<String>,
    /// Directory the read-only workspace tools may see.
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
    /// The request.
    #[arg(long)]
    input: String,
    /// File holding the standing instruction. Defaults to the built-in one.
    #[arg(long)]
    instructions_file: Option<PathBuf>,
    /// Ceiling on model turns.
    #[arg(long)]
    max_turns: Option<u64>,
    /// Ceiling on total reported output tokens.
    #[arg(long)]
    max_output_tokens: Option<u64>,
    /// Ceiling on output tokens offered for any single turn.
    #[arg(long)]
    max_output_tokens_per_turn: Option<u64>,
    /// Wall-clock ceiling in milliseconds, checked between turns.
    #[arg(long)]
    max_duration_ms: Option<u64>,
    /// Sampling temperature. Left out entirely when unset, so the endpoint's own default stands.
    #[arg(long)]
    temperature: Option<f64>,
    /// Nucleus sampling mass. Left out entirely when unset.
    #[arg(long)]
    top_p: Option<f64>,
    /// Reasoning effort, for an endpoint that reads one. An endpoint that fixes effort when it
    /// starts will ignore this; the route registry is what knows which ones do.
    #[arg(long)]
    reasoning_effort: Option<String>,
    /// Approve every tool that asks for a decision. The published toolset is read-only, so this
    /// exists for a toolset that is not.
    #[arg(long)]
    yes: bool,
    /// Emit one JSON event per line on stdout instead of prose.
    #[arg(long)]
    json: bool,
    /// Keep progress off stderr. Warnings are still reported.
    #[arg(long)]
    quiet: bool,
}

#[derive(Debug, Args)]
struct ToolsOptions {
    /// Directory the read-only workspace tools may see.
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

/// Reads a credential from exactly the place the caller named.
///
/// There is no ambient fallback: a harness that quietly picks up a key from the environment is one
/// whose runs cannot be explained afterwards.
fn resolve_credential(options: &RunOptions) -> Result<String, String> {
    credential_from(options.api_key_file.as_ref(), options.api_key_env.as_ref())
}

fn credential_from(file: Option<&PathBuf>, variable: Option<&String>) -> Result<String, String> {
    match (file, variable) {
        (Some(path), None) => fs::read_to_string(path)
            .map(|value| value.trim().to_owned())
            .map_err(|error| format!("reading the credential file `{}`: {error}", path.display())),
        (None, Some(name)) => std::env::var(name)
            .map(|value| value.trim().to_owned())
            .map_err(|_| format!("the environment variable `{name}` is not set")),
        _ => Err("supply exactly one of `--api-key-file` or `--api-key-env`".to_owned()),
    }
    .and_then(|value| {
        if value.is_empty() {
            Err("the credential source is empty".to_owned())
        } else {
            Ok(value)
        }
    })
}

fn app_server_command(options: &AppServerOptions) -> Result<(), String> {
    let credential = credential_from(options.api_key_file.as_ref(), options.api_key_env.as_ref())?;
    let endpoint = Endpoint::new(&options.base_url, &options.model, options.context_window)
        .map_err(|error| error.to_string())?;
    let bearer: Arc<dyn harness_wire::BearerSource> = Arc::new(StaticBearer::new(credential));
    let config = ServerConfig {
        model: options.model.clone(),
        budget: Budget {
            max_turns: options.max_turns,
            max_output_tokens: options.max_output_tokens,
            ..Budget::default()
        },
        version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    // One client per turn: a turn that was interrupted leaves its token set, and reusing the
    // client would end the next turn before it started.
    let mut new_model = |cancel: harness_wire::Cancel| {
        ResponsesClient::new(endpoint.clone(), Arc::clone(&bearer))
            .map(|client| Box::new(client.with_cancel(cancel)) as Box<dyn harness_wire::ModelPort>)
            .map_err(|error| error.to_string())
    };
    harness_app_server::serve(
        &config,
        &mut new_model,
        io::BufReader::new(io::stdin()),
        io::stdout(),
    )
    .map_err(|error| error.to_string())
}

fn budget(options: &RunOptions) -> Budget {
    Budget {
        max_turns: options.max_turns,
        max_input_tokens: None,
        max_output_tokens: options.max_output_tokens,
        max_output_tokens_per_turn: options.max_output_tokens_per_turn,
        max_duration_ms: options.max_duration_ms,
        max_cost_microunits: None,
    }
}

fn sampling(options: &RunOptions) -> Sampling {
    Sampling {
        temperature: options.temperature,
        top_p: options.top_p,
        reasoning_effort: options.reasoning_effort.clone(),
    }
}

fn instructions(options: &RunOptions) -> Result<String, String> {
    match &options.instructions_file {
        Some(path) => fs::read_to_string(path)
            .map_err(|error| format!("reading `{}`: {error}", path.display())),
        None => Ok(DEFAULT_INSTRUCTIONS.to_owned()),
    }
}

fn run_command(options: &RunOptions) -> Result<LoopStop, String> {
    let credential = resolve_credential(options)?;
    let endpoint = Endpoint::new(&options.base_url, &options.model, options.context_window)
        .map_err(|error| error.to_string())?;
    let cancel = LoopCancel::new();
    let mut client = ResponsesClient::new(endpoint, Arc::new(StaticBearer::new(credential)))
        .map_err(|error| error.to_string())?
        .with_cancel(cancel.clone());
    let mut tools = WorkspaceTools::new(&options.workspace)?;
    let mut approvals: Box<dyn ApprovalPort> = if options.yes {
        Box::new(ApproveAll)
    } else {
        Box::new(DenyAll)
    };
    let config = LoopConfig::new(&options.model, instructions(options)?)
        .with_sampling(sampling(options))
        .with_budget(budget(options));

    install_interrupt(&cancel);

    let mut renderer = Renderer::new(io::stdout(), io::stderr(), options.json, options.quiet);
    let outcome = AgentLoop::new(&mut client, &mut tools, approvals.as_mut(), config)
        .with_cancel(cancel)
        .run(options.input.clone(), &mut renderer)
        .map_err(|error| error.to_string())?;
    Ok(outcome.stop)
}

/// Makes Ctrl-C end the run rather than the process.
///
/// One token reaches both the loop and the response body being read, so the layer that is actually
/// blocked — during a turn, almost always the model read — is the one that stops.
fn install_interrupt(cancel: &LoopCancel) {
    let cancel = cancel.clone();
    let installed = ctrlc::set_handler(move || {
        cancel.cancel();
    });
    if installed.is_err() {
        eprintln!("warning [interrupt] Ctrl-C will end the process rather than the run");
    }
}

fn tools_command(options: &ToolsOptions) -> Result<(), String> {
    let tools = WorkspaceTools::new(&options.workspace)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "workspace": tools.root().display().to_string(),
            "tools": tools_specs(&tools),
        }))
        .map_err(|error| error.to_string())?
    );
    Ok(())
}

fn tools_specs(tools: &WorkspaceTools) -> serde_json::Value {
    use harness_wire::ToolPort as _;
    serde_json::to_value(tools.specs()).unwrap_or(serde_json::Value::Null)
}

/// Runs the parsed command.
///
/// Exit status distinguishes the three things a caller acts on differently: the model answered,
/// the run stopped for a named reason, or the harness could not run at all.
pub fn dispatch(cli: &Cli) -> ExitCode {
    match &cli.command {
        Command::Run(options) => match run_command(options) {
            Ok(stop) if stop.is_completed() => ExitCode::SUCCESS,
            Ok(stop) => {
                eprintln!("the run stopped without an answer: {stop:?}");
                ExitCode::from(2)
            }
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::FAILURE
            }
        },
        Command::Tools(options) => match tools_command(options) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::FAILURE
            }
        },
        Command::AppServer(options) => match app_server_command(options) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::FAILURE
            }
        },
    }
}

pub fn run() -> ExitCode {
    dispatch(&Cli::parse())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory as _;

    fn parse(arguments: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(arguments)
    }

    #[test]
    fn the_command_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn a_run_needs_an_endpoint_a_model_and_an_input() {
        assert!(parse(&["daemonloom-harness", "run"]).is_err());
        assert!(
            parse(&[
                "daemonloom-harness",
                "run",
                "--base-url",
                "https://gw.example/v1",
                "--model",
                "m",
                "--input",
                "hi",
                "--api-key-env",
                "KEY",
            ])
            .is_ok()
        );
    }

    #[test]
    fn the_two_credential_sources_are_mutually_exclusive() {
        assert!(
            parse(&[
                "daemonloom-harness",
                "run",
                "--base-url",
                "https://gw.example/v1",
                "--model",
                "m",
                "--input",
                "hi",
                "--api-key-env",
                "KEY",
                "--api-key-file",
                "/tmp/key",
            ])
            .is_err()
        );
    }

    fn options(arguments: &[&str]) -> RunOptions {
        let base = vec![
            "daemonloom-harness",
            "run",
            "--base-url",
            "https://gw.example/v1",
            "--model",
            "m",
            "--input",
            "hi",
        ];
        let Command::Run(options) = parse(&[base, arguments.to_vec()].concat())
            .expect("the arguments parse")
            .command
        else {
            panic!("the run subcommand parses to run options");
        };
        *options
    }

    #[test]
    fn naming_no_credential_source_refuses() {
        let error = resolve_credential(&options(&[])).expect_err("no source refuses");
        assert!(error.contains("exactly one"), "{error}");
    }

    #[test]
    fn an_unset_environment_variable_refuses_by_name() {
        let options = options(&["--api-key-env", "DAEMONLOOM_HARNESS_ABSENT_TEST_KEY"]);
        let error = resolve_credential(&options).expect_err("an unset variable refuses");
        assert!(
            error.contains("DAEMONLOOM_HARNESS_ABSENT_TEST_KEY"),
            "{error}"
        );
    }

    #[test]
    fn a_credential_file_is_read_and_trimmed() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("key");
        fs::write(&path, "  sk-test\n").expect("write");
        let options = options(&["--api-key-file", path.to_str().expect("utf-8 path")]);
        assert_eq!(resolve_credential(&options).expect("readable"), "sk-test");
    }

    #[test]
    fn an_empty_credential_refuses_rather_than_reaching_the_endpoint() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("key");
        fs::write(&path, "\n \n").expect("write");
        let options = options(&["--api-key-file", path.to_str().expect("utf-8 path")]);
        assert!(resolve_credential(&options).is_err());
    }

    #[test]
    fn a_missing_credential_file_names_the_path() {
        let options = options(&["--api-key-file", "/definitely/not/here"]);
        let error = resolve_credential(&options).expect_err("a missing file refuses");
        assert!(error.contains("/definitely/not/here"), "{error}");
    }

    #[test]
    fn budget_flags_reach_the_loop_and_carry_no_spend_ceiling() {
        let budget = budget(&options(&[
            "--max-turns",
            "4",
            "--max-output-tokens",
            "900",
        ]));
        assert_eq!(budget.max_turns, Some(4));
        assert_eq!(budget.max_output_tokens, Some(900));
        assert_eq!(
            budget.max_cost_microunits, None,
            "the CLI must not offer a bound the loop refuses"
        );
        assert!(budget.validate().is_ok());
    }

    #[test]
    fn the_default_instruction_names_the_read_only_toolset() {
        let text = instructions(&options(&[])).expect("the default is available");
        assert!(text.contains("workspace.read"), "{text}");
        assert!(text.contains("read-only"), "{text}");
    }

    #[test]
    fn an_instruction_file_replaces_the_default() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("instructions.md");
        fs::write(&path, "be terse").expect("write");
        let options = options(&["--instructions-file", path.to_str().expect("utf-8 path")]);
        assert_eq!(instructions(&options).expect("readable"), "be terse");
    }
}
