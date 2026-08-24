#![forbid(unsafe_code)]

//! The b10x harness, driven from the command line.
//!
//! The command is a thin shell: it resolves an endpoint, a credential and a workspace, then hands
//! them to [`harness_loop`]. Everything interesting happens in the loop, which is what lets the
//! same core run embedded in another process or behind a bridge without a second implementation.

mod metaharness;
mod render;

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

/// The standing instruction when the caller supplies none.
///
/// # What this used to say, and what it cost
///
/// Until the three verbs landed, this text named `workspace_list`, `workspace_read` and
/// `workspace_grep` and told the model *"nothing you can call changes a file or runs a command, so
/// say what you would change rather than claiming you changed it"*. Both halves went stale: those
/// tools no longer exist under any name, and the catalogue behind the verbs now reaches six entries
/// on a machine that can confine a process.
///
/// It was not a harmless leftover. A run given a write-and-execute catalogue and this instruction
/// was told in the same breath that it could do neither — and the measured result was a model that
/// searched for read-only tools, read two files, changed nothing, and reported the task done.
/// **The instruction had asked it to.**
///
/// So the text below states no effects at all. What a run can do differs from run to run — it is a
/// question about the machine, answered by `tool_search` — and an instruction that answers it in
/// advance can only ever be out of date.
const DEFAULT_INSTRUCTIONS: &str = "\
You are the b10x coding harness. Everything you can do reaches you through three tools: \
`tool_search` lists what this run has, `tool_describe` gives one entry's input schema, and \
`tool_invoke` calls it. Begin by calling `tool_search` with no arguments. The catalogue differs \
from run to run, so filtering it before you have seen it whole hides tools you may need, and what \
is missing from a filtered list is not missing from the run. Ground every claim about the \
workspace in something you actually read, and say plainly when you have not looked. Never report \
work as done unless a tool you called made it so.";

#[derive(Debug, Parser)]
#[command(
    name = "b10x-harness",
    version,
    about = "The b10x agent loop, run against an OpenAI-compatible endpoint"
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
    /// Rewrite a `--json` loop record as the `metaharness.event/1` stream every evaluation arm is
    /// judged from.
    ///
    /// A converter and **not** a metaharness adapter, and the difference is the point. Every other
    /// arm reaches the matrix through an adapter that spawns a vendor binary and decides each tool
    /// call at a seam — which is what arm `driven` measures. Arm `native` measures the opposite
    /// claim, that the published toolset *is* the policy, so wrapping this loop in a seam would put
    /// the driven arm's treatment back on top of it and measure that instead. What crosses is the
    /// record, not the control.
    Events(EventsOptions),
}

#[derive(Debug, Args)]
struct EventsOptions {
    /// The loop record to read. Standard input when absent.
    #[arg(long)]
    r#in: Option<PathBuf>,
    /// Where to write. Standard output when absent.
    #[arg(long)]
    out: Option<PathBuf>,
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
    /// The substrate daemon's socket, so a run may write and execute inside a confined workspace.
    ///
    /// Without it the toolset is read-only, which is what this harness has always published. With
    /// it, what appears is what the daemon says the machine can confine — a host with no delegated
    /// cgroup root serves workspaces and no execution, and the run then has the write tools and no
    /// `run` tool. Named rather than discovered, for the reason the credential is: a harness that
    /// picked up a confinement boundary from the environment is one whose effects depend on where
    /// it happened to be started.
    #[arg(long)]
    substrate: Option<PathBuf>,
    /// Hold substrate's driver in this process instead, confining `--workspace` itself.
    ///
    /// The same confinement — guarded IO, `openat2` containment, cgroups and namespaces around an
    /// exec are the driver's, and they are here. What a socket adds and this does not is an
    /// authenticated subject derived from kernel peer credentials; embedded there is no peer. Right
    /// for a run on the operator's own machine, wrong for anything multi-tenant.
    ///
    /// The workspace is **adopted, not created**: `--workspace` is the tree, its parent becomes
    /// substrate's root, and reads and writes land in the same place. The directory must therefore
    /// be named `ws_something` — substrate's guarded filesystem will not represent any other name —
    /// and one that is not leaves the run read-only rather than quietly writing somewhere else.
    ///
    /// Takes no value today; the root is derived from `--workspace`.
    #[arg(long, conflicts_with = "substrate")]
    substrate_embedded: Option<PathBuf>,
    /// Which confined workspace the write and execute tools act in.
    ///
    /// Ignored under `--substrate-embedded`, which opens one and names it: the driver mints the
    /// identity there, and a caller-supplied name would have to satisfy a rule only the driver
    /// knows.
    #[arg(long, default_value = "default")]
    workspace_id: String,
    /// A delegated cgroup subtree, so the embedded driver may confine a process.
    ///
    /// Without one substrate's probe reports no exec facts and no `run` tool is published — which
    /// is correct and is also why a test-first task cannot be attempted: a run that may not execute
    /// its suite cannot see a test fail before writing the code, so it will not write the code. The
    /// subtree must be delegated to this user, hold `cpu`, `memory` and `pids`, and be free of
    /// processes.
    #[arg(long)]
    cgroup_root: Option<PathBuf>,
    /// A program `run` may start. Repeatable, and an empty set publishes no `run` at all.
    ///
    /// Declared rather than open, because an argv whose program could be anything is a shell with
    /// extra steps. A set nobody named means nobody wanted one.
    #[arg(long)]
    allow_program: Vec<String>,
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
    /// A rate card, so the run reports what it cost.
    ///
    /// A JSON document naming its own `source` and `as_of` date and holding
    /// `input_usd_per_mtok`, `cached_input_usd_per_mtok` and `output_usd_per_mtok` per model.
    /// Declared rather than compiled in: a table baked into this binary would be a set of numbers
    /// nobody could date, and wrong silently the first time a rate moved.
    ///
    /// Without one the run reports tokens and no price — which is what it has always done, and is
    /// also why a b10x record could not be compared with a Claude Code record on cost. A model the
    /// card does not list is warned about by name rather than reported as free.
    #[arg(long)]
    prices: Option<PathBuf>,
    /// Total spend ceiling for the run, in millionths of a US dollar. Needs `--prices`.
    ///
    /// Checked after each turn, because that is when the provider reports what it charged for.
    /// Refused rather than ignored when the run cannot price itself.
    #[arg(long)]
    max_cost_microunits: Option<u64>,
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
    /// The substrate daemon's socket, so a run may write and execute inside a confined workspace.
    ///
    /// Without it the toolset is read-only, which is what this harness has always published. With
    /// it, what appears is what the daemon says the machine can confine — a host with no delegated
    /// cgroup root serves workspaces and no execution, and the run then has the write tools and no
    /// `run` tool. Named rather than discovered, for the reason the credential is: a harness that
    /// picked up a confinement boundary from the environment is one whose effects depend on where
    /// it happened to be started.
    #[arg(long)]
    substrate: Option<PathBuf>,
    /// Hold substrate's driver in this process instead, confining `--workspace` itself.
    ///
    /// The same confinement — guarded IO, `openat2` containment, cgroups and namespaces around an
    /// exec are the driver's, and they are here. What a socket adds and this does not is an
    /// authenticated subject derived from kernel peer credentials; embedded there is no peer. Right
    /// for a run on the operator's own machine, wrong for anything multi-tenant.
    ///
    /// The workspace is **adopted, not created**: `--workspace` is the tree, its parent becomes
    /// substrate's root, and reads and writes land in the same place. The directory must therefore
    /// be named `ws_something` — substrate's guarded filesystem will not represent any other name —
    /// and one that is not leaves the run read-only rather than quietly writing somewhere else.
    ///
    /// Takes no value today; the root is derived from `--workspace`.
    #[arg(long, conflicts_with = "substrate")]
    substrate_embedded: Option<PathBuf>,
    /// Which confined workspace the write and execute tools act in.
    ///
    /// Ignored under `--substrate-embedded`, which opens one and names it: the driver mints the
    /// identity there, and a caller-supplied name would have to satisfy a rule only the driver
    /// knows.
    #[arg(long, default_value = "default")]
    workspace_id: String,
    /// A delegated cgroup subtree, so the embedded driver may confine a process.
    ///
    /// Without one substrate's probe reports no exec facts and no `run` tool is published — which
    /// is correct and is also why a test-first task cannot be attempted: a run that may not execute
    /// its suite cannot see a test fail before writing the code, so it will not write the code. The
    /// subtree must be delegated to this user, hold `cpu`, `memory` and `pids`, and be free of
    /// processes.
    #[arg(long)]
    cgroup_root: Option<PathBuf>,
    /// A program `run` may start. Repeatable, and an empty set publishes no `run` at all.
    ///
    /// Declared rather than open, because an argv whose program could be anything is a shell with
    /// extra steps. A set nobody named means nobody wanted one.
    #[arg(long)]
    allow_program: Vec<String>,
}

/// Reads a credential from exactly the place the caller named, or none.
///
/// There is no ambient fallback: a harness that quietly picks up a key from the environment is one
/// whose runs cannot be explained afterwards. **Naming neither source is itself a declaration** —
/// the run sends no `authorization` header, which is right for a gateway on this machine that
/// authenticates nobody, and for a run deliberately started with no credential whose first request
/// is meant to be refused by the far end.
fn resolve_credential(options: &RunOptions) -> Result<Option<String>, String> {
    credential_from(options.api_key_file.as_ref(), options.api_key_env.as_ref())
}

fn credential_from(
    file: Option<&PathBuf>,
    variable: Option<&String>,
) -> Result<Option<String>, String> {
    let value = match (file, variable) {
        (None, None) => return Ok(None),
        (Some(path), None) => fs::read_to_string(path)
            .map(|value| value.trim().to_owned())
            .map_err(|error| format!("reading the credential file `{}`: {error}", path.display())),
        (None, Some(name)) => std::env::var(name)
            .map(|value| value.trim().to_owned())
            .map_err(|_| format!("the environment variable `{name}` is not set")),
        // `conflicts_with` makes this unreachable from the command line and reachable from a
        // caller that built the options itself.
        (Some(_), Some(_)) => {
            Err("supply at most one of `--api-key-file` or `--api-key-env`".to_owned())
        }
    }?;
    // A source that was named and answered with nothing is an error, not a declaration: the caller
    // meant to authenticate and something went wrong upstream of here.
    if value.is_empty() {
        return Err("the credential source is empty".to_owned());
    }
    Ok(Some(value))
}

/// A client for this endpoint, authenticated or deliberately not.
fn model_client(
    endpoint: Endpoint,
    credential: Option<String>,
    cancel: &LoopCancel,
) -> Result<ResponsesClient, String> {
    match credential {
        Some(value) => ResponsesClient::new(endpoint, Arc::new(StaticBearer::new(value))),
        None => ResponsesClient::unauthenticated(endpoint),
    }
    .map(|client| client.with_cancel(cancel.clone()))
    .map_err(|error| error.to_string())
}

fn app_server_command(options: &AppServerOptions) -> Result<(), String> {
    let credential = credential_from(options.api_key_file.as_ref(), options.api_key_env.as_ref())?;
    let endpoint = Endpoint::new(&options.base_url, &options.model, options.context_window)
        .map_err(|error| error.to_string())?;
    let bearer: Option<Arc<dyn harness_wire::BearerSource>> = credential
        .map(|value| Arc::new(StaticBearer::new(value)) as Arc<dyn harness_wire::BearerSource>);
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
        match &bearer {
            Some(source) => ResponsesClient::new(endpoint.clone(), Arc::clone(source)),
            None => ResponsesClient::unauthenticated(endpoint.clone()),
        }
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
        max_cost_microunits: options.max_cost_microunits,
    }
}

/// Reads the rate card the caller named, or none.
///
/// A card that cannot be read is fatal rather than skipped: a caller who asked to be told the cost
/// and got silence would read the silence as free.
fn prices(options: &RunOptions) -> Result<Option<harness_loop::RateCard>, String> {
    let Some(path) = options.prices.as_ref() else {
        return Ok(None);
    };
    let text = fs::read_to_string(path)
        .map_err(|error| format!("reading the rate card `{}`: {error}", path.display()))?;
    harness_loop::RateCard::parse(&text)
        .map(Some)
        .map_err(|error| format!("the rate card `{}`: {error}", path.display()))
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
    let mut client = model_client(endpoint, credential, &cancel)?;
    let mut tools = published(
        harness_tools::LocalOperations::new(&options.workspace)?,
        workspace_name(&options.workspace),
        options.substrate.as_deref(),
        options.substrate_embedded.as_deref(),
        options.cgroup_root.as_deref(),
        &options.workspace_id,
        &options.allow_program,
    );
    let mut approvals: Box<dyn ApprovalPort> = if options.yes {
        Box::new(ApproveAll)
    } else {
        Box::new(DenyAll)
    };
    let config = LoopConfig::new(&options.model, instructions(options)?)
        .with_sampling(sampling(options))
        .with_budget(budget(options))
        .with_prices(prices(options)?);

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

/// The tools this machine admits, which is the machine's answer and not a flag's.
///
/// **The publication gate, in one function**, and it is unchanged by the move to three verbs: what
/// the *model* sees is always `tool_search`, `tool_describe`, `tool_invoke`, and what the catalogue
/// behind them holds is what the machine can perform. Three entries with no backend; five with a
/// confined workspace; six inside a delegated cgroup. A tool the machine cannot confine is one
/// `tool_search` never lists.
///
/// A backend that cannot be reached is not an error, for the reason `Client::probe` gives. A daemon
/// that answers something unreadable is, and every other failure here degrades to the read-only
/// catalogue rather than to a run that thinks it can write.
fn published(
    reading: harness_tools::LocalOperations,
    workspace_name: Option<String>,
    substrate: Option<&std::path::Path>,
    embedded: Option<&std::path::Path>,
    cgroup_root: Option<&std::path::Path>,
    workspace_id: &str,
    programs: &[String],
) -> harness_tools::Verbs {
    let read_only = || harness_tools::Verbs::new(harness_tools::Catalogue::of(reading.clone()));

    if embedded.is_some() {
        // **One tree.** The workspace the reading provider sees *is* the confined workspace: its
        // parent becomes substrate's root and the directory itself is adopted. Opening a fresh empty
        // workspace beside it would mean a run read one tree and wrote into another, and was not
        // doing the task it had been given.
        let (Some(root), Some(name)) = (reading.root().parent(), workspace_name) else {
            return read_only();
        };
        let cgroup = cgroup_root.map(std::path::Path::to_path_buf);
        let (Ok(driver), Ok(tools)) = (
            harness_substrate::Embedded::open(root, cgroup.clone()),
            harness_substrate::Embedded::open(root, cgroup),
        ) else {
            return read_only();
        };
        let (Ok(facts), Ok(workspace)) = (
            harness_substrate::Backend::machine(&driver),
            driver.workspace_adopt(&name),
        ) else {
            return read_only();
        };
        let confined =
            harness_substrate::ConfinedOperations::new(tools, &facts, workspace, programs.to_vec());
        return harness_tools::Verbs::new(harness_tools::Catalogue::of(harness_tools::Split::new(
            reading, confined,
        )));
    }

    let Some(socket) = substrate else {
        return read_only();
    };
    let client = harness_substrate::Client::at(socket);
    let Ok(facts) = client.probe() else {
        return read_only();
    };
    let confined = harness_substrate::ConfinedOperations::new(
        harness_substrate::Client::at(socket),
        &facts,
        workspace_id,
        programs.to_vec(),
    );
    harness_tools::Verbs::new(harness_tools::Catalogue::of(harness_tools::Split::new(
        reading, confined,
    )))
}

fn tools_command(options: &ToolsOptions) -> Result<(), String> {
    let tools = published(
        harness_tools::LocalOperations::new(&options.workspace)?,
        workspace_name(&options.workspace),
        options.substrate.as_deref(),
        options.substrate_embedded.as_deref(),
        options.cgroup_root.as_deref(),
        &options.workspace_id,
        &options.allow_program,
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "workspace": options.workspace.display().to_string(),
            "tools": tools_specs(&tools),
            // What the three verbs stand in front of, so `b10x-harness tools` still answers the
            // question a reader is actually asking: what can this run do?
            "catalogue": tools.catalogue().search(None, None),
        }))
        .map_err(|error| error.to_string())?
    );
    Ok(())
}

/// The directory's own name, which is what substrate will represent the workspace as.
fn workspace_name(workspace: &std::path::Path) -> Option<String> {
    workspace
        .canonicalize()
        .ok()?
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
}

fn tools_specs(tools: &dyn harness_wire::ToolPort) -> serde_json::Value {
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
        Command::Events(options) => match events_command(options) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::FAILURE
            }
        },
    }
}

/// `b10x-harness events`
fn events_command(options: &EventsOptions) -> Result<(), String> {
    let mut input: Box<dyn std::io::BufRead> = match &options.r#in {
        Some(path) => Box::new(std::io::BufReader::new(
            std::fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?,
        )),
        None => Box::new(std::io::BufReader::new(std::io::stdin())),
    };
    let mut output: Box<dyn std::io::Write> = match &options.out {
        Some(path) => Box::new(
            std::fs::File::create(path).map_err(|error| format!("{}: {error}", path.display()))?,
        ),
        None => Box::new(std::io::stdout()),
    };
    metaharness::convert(&mut input, &mut output, env!("CARGO_PKG_VERSION"))
        .map(|_| ())
        .map_err(|error| error.to_string())
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
        assert!(parse(&["b10x-harness", "run"]).is_err());
        assert!(
            parse(&[
                "b10x-harness",
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
                "b10x-harness",
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
            "b10x-harness",
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
    fn an_unset_environment_variable_refuses_by_name() {
        let options = options(&["--api-key-env", "B10X_HARNESS_ABSENT_TEST_KEY"]);
        let error = resolve_credential(&options).expect_err("an unset variable refuses");
        assert!(error.contains("B10X_HARNESS_ABSENT_TEST_KEY"), "{error}");
    }

    #[test]
    fn a_credential_file_is_read_and_trimmed() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("key");
        fs::write(&path, "  sk-test\n").expect("write");
        let options = options(&["--api-key-file", path.to_str().expect("utf-8 path")]);
        assert_eq!(
            resolve_credential(&options).expect("readable"),
            Some("sk-test".to_owned())
        );
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
    fn naming_no_credential_source_is_a_declaration_and_not_an_error() {
        // The shape a gateway on this machine needs, and the one a run started deliberately
        // without a credential needs: no `authorization` header, refused by the far end rather
        // than by this process. An *empty* named source stays an error, above — that is a
        // credential that went wrong, not one nobody asked for.
        assert_eq!(resolve_credential(&options(&[])).expect("declared"), None);
    }

    #[test]
    fn a_missing_credential_file_names_the_path() {
        let options = options(&["--api-key-file", "/definitely/not/here"]);
        let error = resolve_credential(&options).expect_err("a missing file refuses");
        assert!(error.contains("/definitely/not/here"), "{error}");
    }

    #[test]
    fn budget_flags_reach_the_loop() {
        let budget = budget(&options(&[
            "--max-turns",
            "4",
            "--max-output-tokens",
            "900",
        ]));
        assert_eq!(budget.max_turns, Some(4));
        assert_eq!(budget.max_output_tokens, Some(900));
        assert_eq!(budget.max_cost_microunits, None, "none was asked for");
        assert!(budget.validate(false).is_ok());
    }

    #[test]
    fn a_spend_ceiling_reaches_the_loop_and_is_refused_without_rates_to_measure_it_against() {
        let budget = budget(&options(&["--max-cost-microunits", "50000"]));
        assert_eq!(budget.max_cost_microunits, Some(50_000));
        assert!(
            budget.validate(false).is_err(),
            "a ceiling nobody can compute is refused rather than ignored"
        );
        assert!(budget.validate(true).is_ok());
    }

    #[test]
    fn a_rate_card_reaches_the_loop_and_an_unreadable_one_stops_the_run() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("rates.json");
        fs::write(
            &path,
            r#"{"source": "a table someone read", "as_of": "2026-08-24", "models": {
                "m": {"input_usd_per_mtok": 1.0, "cached_input_usd_per_mtok": 0.1,
                      "output_usd_per_mtok": 2.0}}}"#,
        )
        .expect("write");
        let loaded = prices(&options(&["--prices", path.to_str().expect("utf-8 path")]))
            .expect("readable")
            .expect("a card");
        assert!(loaded.rates_for("m").is_some());

        assert!(prices(&options(&[])).expect("no flag is no card").is_none());

        // Fatal rather than skipped: a caller who asked to be told the cost and got silence would
        // read the silence as free.
        let bad = dir.path().join("bad.json");
        fs::write(
            &bad,
            r#"{"source": "", "as_of": "2026-08-24", "models": {}}"#,
        )
        .expect("write");
        let error = prices(&options(&["--prices", bad.to_str().expect("utf-8 path")]))
            .expect_err("a card with no provenance refuses");
        assert!(error.contains("provenance"), "{error}");
    }

    #[test]
    fn the_default_instruction_points_at_the_catalogue_and_promises_no_effects() {
        let text = instructions(&options(&[])).expect("the default is available");
        assert!(text.contains("tool_search"), "{text}");
        for stale in ["workspace_read", "workspace_list", "workspace_grep"] {
            assert!(!text.contains(stale), "`{stale}` no longer exists: {text}");
        }
        // The regression that made a live run change nothing and say it had: the standing
        // instruction told a write-capable run that it could not write.
        assert!(
            !text.contains("read-only"),
            "what a run can do is `tool_search`'s answer, not this text's: {text}"
        );
        assert!(text.contains("Never report work as done"), "{text}");
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
