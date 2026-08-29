#![forbid(unsafe_code)]

//! The b10x harness, driven from the command line.
//!
//! The command is a thin shell: it resolves an endpoint, a credential and a workspace, then hands
//! them to [`harness_loop`]. Everything interesting happens in the loop, which is what lets the
//! same core run embedded in another process or behind a bridge without a second implementation.

mod metaharness;
mod render;

use std::fmt::Write as _;
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
use harness_wire::{Risk, Sampling, StaticBearer};

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
/// So this text states **no effects of its own**. What follows it is the catalogue, rendered from
/// the live one by [`harness_tools::Catalogue::brief`], which cannot describe a tool this run does
/// not have.
const DEFAULT_INSTRUCTIONS: &str = "\
You are the b10x coding harness. Everything you can do reaches you through three tools: \
`tool_search` lists what this run has, `tool_describe` gives one entry's input schema, and \
`tool_invoke` calls it — `tool_invoke` is the only one that acts. Ground every claim about the \
workspace in something you actually read, and say plainly when you have not looked. Never report \
work as done unless a tool you called made it so. A call that was not approved did not happen: do \
not retry it — do what you can without it and say plainly what you could not do.";

/// The standing instruction, with this run's catalogue written into it.
///
/// **The catalogue belongs in the instructions, not in the conversation.** Discovering it through
/// `tool_search` and `tool_describe` cost 33–44% of every tool call across three measured runs —
/// four calls of ten spent finding out what exists, each a billed round trip that is then replayed
/// in every later turn. And the answers landed in the conversation, which grows and is re-sent at
/// the full input rate, rather than in the instructions, which are identical every turn and are
/// what a prompt cache can hold.
///
/// The verbs are unchanged and still the only way to act. What is removed is the requirement to
/// ask before doing anything.
fn standing_instruction(
    catalogue: &harness_tools::Catalogue,
    context: &str,
    announce: bool,
) -> String {
    let mut text = format!(
        "{DEFAULT_INSTRUCTIONS}\n\nThis run's catalogue, which is what `tool_invoke` will \
         accept — call `tool_search` only if you need to re-check it:\n\n{}",
        catalogue.brief()
    );
    // Where the run may write, in the instruction as well as in the tool. The tool is what makes
    // it true; this is what stops the model spending a turn discovering it by being refused.
    if announce && !catalogue.scope().is_empty() {
        text.push_str("\n\nWhere this run may write. A path no rule names is unrestricted:\n");
        for rule in catalogue.scope().rules() {
            let word = match rule.write {
                harness_tools::WriteScope::Allowed => "may be written or edited",
                harness_tools::WriteScope::PartialOnly => {
                    "may be edited in part, never replaced whole — use `file_edit`"
                }
                harness_tools::WriteScope::Denied => "must not be changed at all",
            };
            let _ = writeln!(text, "- `{}` {word}", rule.paths);
        }
    }
    if !context.is_empty() {
        text.push_str(
            "\n\nFiles you have been given. They are already read; do not read them again:\n",
        );
        text.push_str(context);
    }
    text
}

/// The highest risk a call may carry without a person being asked — `harness_wire::Risk`, as
/// the command line spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum ApproveUpTo {
    /// Wrong is cheap and visible: reads.
    Low,
    /// Wrong costs work to undo: `file_write`.
    Medium,
    /// Wrong costs something that is not work: `run`.
    High,
    /// Wrong cannot be undone.
    Destructive,
}

impl From<ApproveUpTo> for Risk {
    fn from(ceiling: ApproveUpTo) -> Self {
        match ceiling {
            ApproveUpTo::Low => Self::Low,
            ApproveUpTo::Medium => Self::Medium,
            ApproveUpTo::High => Self::High,
            ApproveUpTo::Destructive => Self::Destructive,
        }
    }
}

/// Whether a declared scope is stated in the instruction as well as bound into the tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum ScopeAnnounce {
    /// Say it up front, so no turn is spent discovering it by being refused.
    Stated,
    /// Bind it and say nothing. The refusal has to teach it — which is what makes a run under this
    /// a measurement of the toolset rather than of the prose.
    Silent,
}

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

// A command line is a struct of switches; counting them says nothing about the type.
#[allow(clippy::struct_excessive_bools)]
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
    /// and one that is not refuses the run by name rather than quietly writing somewhere else.
    #[arg(long, conflicts_with = "substrate")]
    substrate_embedded: bool,
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
    /// Admit a build toolchain read-only inside the confined workspace. Only `rust` today.
    ///
    /// Without one a confined run can execute anything whose implementation lives under `/usr` —
    /// an interpreter — and nothing whose compilers and package registry live in the operator's
    /// home, which is every build tool. The directories are mounted **read-only** and reported in
    /// the run's observation (substrate ADR 0010); the network stays unshared, so this brings a
    /// closure in and is not a way to reach out.
    ///
    /// Declared rather than implied, because it is the one place a confined run is given something
    /// substrate did not verify: there is no digest over a package registry. A run that does not
    /// need a toolchain should not name one.
    #[arg(long, value_name = "NAME")]
    toolchain: Option<String>,
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
    /// Wall-clock ceiling in milliseconds. Checked between turns and between the calls of one
    /// turn, and what is left is the bound on every `run` the tools start.
    #[arg(long)]
    max_duration_ms: Option<u64>,
    /// Where this run may write, as `<glob>=<allowed|partial-only|denied>`. Repeatable, ordered.
    ///
    /// First match wins; a path no rule mentions is allowed, because this declares where writing is
    /// **restricted** and a scope nobody wrote restricts nothing.
    ///
    /// `partial-only` is the one worth knowing: the path may be changed in part and never replaced
    /// whole. It is what a store whose frontmatter is owned by a CLI needs, and it is a distinction
    /// no list of operations can make — `file_write` and `file_edit` are both writes.
    ///
    /// Refused by the tool, per call, with a reason naming the way in. This loop has no decision
    /// seam and never grows one: the published toolset is the policy.
    #[arg(long, value_name = "GLOB=SCOPE")]
    write_scope: Vec<String>,
    /// Whether the declared scope is also stated in the instruction.
    ///
    /// `silent` is an experiment control, and it exists because the two are different claims. A run
    /// told the rule and a run refused the rule both end with the rule kept, and only the second
    /// shows that the **toolset** is what kept it. Stating it is cheaper — the model spends no call
    /// being refused — so a real run states it.
    #[arg(long, value_name = "MODE", default_value = "stated")]
    scope_announce: ScopeAnnounce,
    /// A file the run is given before it starts, instead of discovering it. Repeatable.
    ///
    /// Costs input tokens on **every** turn, because a stateless loop replays its conversation. It
    /// is still usually a saving: what it replaces is a read, a turn, *and* a result that joins the
    /// same replay. A file that is not there refuses the run — one given a smaller context than it
    /// was told to have is a run nobody can reproduce.
    #[arg(long, value_name = "FILE")]
    context: Vec<PathBuf>,
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
    /// Approve every call that asks for a decision.
    ///
    /// What asks is the loop's answer, taken per call from the catalogue entry's declared risk
    /// against a ceiling that defaults to low: every write and every `run` asks, and a `file_edit`
    /// — non-idempotent, so a repeat is not the same act as one call — asks whatever the ceiling
    /// is. Without this the default approver denies each of them and the model is told it was
    /// denied, which is a confined run that can do nothing but read.
    #[arg(long)]
    yes: bool,
    /// Run calls at or below this risk without asking. Default `low`.
    ///
    /// The ceiling the loop judges each call's declared risk against. A `file_write` is `medium`
    /// and a `run` is `high`, so at `high` both run unasked and only a destructive call asks.
    /// Above the ceiling the approver decides, and with none attached that is a refusal the model
    /// is told about. A `file_edit` asks whatever the ceiling — non-idempotent, so a repeat is not
    /// the same act as one call — and needs `--yes`. `--yes` approves everything and makes this
    /// moot, so the two do not combine.
    #[arg(long, value_name = "RISK", conflicts_with = "yes")]
    approve_up_to: Option<ApproveUpTo>,
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
    /// and one that is not refuses the run by name rather than quietly writing somewhere else.
    #[arg(long, conflicts_with = "substrate")]
    substrate_embedded: bool,
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
    /// Where a run may write, as `<glob>=<allowed|partial-only|denied>`, so `tools` answers with it.
    ///
    /// The same declaration `run` takes. It belongs in this answer because "what can this run do?"
    /// is not answered by a list of tools alone once some paths refuse some of them.
    #[arg(long, value_name = "GLOB=SCOPE")]
    write_scope: Vec<String>,
    /// Admit a build toolchain read-only, so `tools` describes the run a `run` would get.
    #[arg(long, value_name = "NAME")]
    toolchain: Option<String>,
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

/// The ceiling the loop judges each call against: what was asked for, or `Low`.
fn ceiling(options: &RunOptions) -> Risk {
    options.approve_up_to.map_or(Risk::Low, Into::into)
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

/// The standing instruction for this run: the operator's file, or the default plus the catalogue.
///
/// An operator-named file is used **verbatim**, catalogue and all. Appending to it would be this
/// function editing a document somebody wrote, and a run whose instruction is not the file it names
/// is one nobody can reproduce from the file.
fn instructions(
    options: &RunOptions,
    catalogue: &harness_tools::Catalogue,
    context: &str,
) -> Result<String, String> {
    match &options.instructions_file {
        Some(path) => fs::read_to_string(path)
            .map_err(|error| format!("reading `{}`: {error}", path.display())),
        None => Ok(standing_instruction(
            catalogue,
            context,
            options.scope_announce == ScopeAnnounce::Stated,
        )),
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
        &Confinement {
            substrate: options.substrate.as_deref(),
            embedded: options.substrate_embedded,
            cgroup_root: options.cgroup_root.as_deref(),
            workspace_id: &options.workspace_id,
            programs: &options.allow_program,
            toolchain: &toolchain(options.toolchain.as_deref())?,
            scope: write_scope(&options.write_scope)?,
        },
    )?;
    let mut approvals: Box<dyn ApprovalPort> = if options.yes {
        Box::new(ApproveAll)
    } else {
        Box::new(DenyAll)
    };
    let config = LoopConfig::new(
        &options.model,
        instructions(options, tools.catalogue(), &context(&options.context)?)?,
    )
    .with_sampling(sampling(options))
    .with_budget(budget(options))
    .with_prices(prices(options)?)
    .with_unattended_ceiling(ceiling(options));

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

/// The scope the caller declared, in order.
///
/// # Errors
///
/// Names the rule that could not be read. A misspelling silently becoming "allowed" would be a
/// boundary that quietly is not one.
fn write_scope(declarations: &[String]) -> Result<harness_tools::Scope, String> {
    declarations
        .iter()
        .map(|rule| harness_tools::ScopeRule::parse(rule))
        .collect::<Result<Vec<_>, _>>()
        .map(harness_tools::Scope::of)
}

/// The declared context files, read and labelled.
///
/// # Errors
///
/// Names the file that could not be read. A run given a smaller context than it was told to have is
/// one nobody can reproduce from the declaration.
fn context(files: &[PathBuf]) -> Result<String, String> {
    let mut text = String::new();
    for path in files {
        let body = fs::read_to_string(path)
            .map_err(|error| format!("reading the context file `{}`: {error}", path.display()))?;
        let _ = write!(text, "\n\n--- {} ---\n{body}", path.display());
    }
    Ok(text)
}

/// The toolchain the caller declared, or none.
///
/// # Errors
///
/// Names the toolchain that is not known, and lists the ones that are. A misspelling that silently
/// declared nothing would produce a run whose builds fail deep inside a build tool, which is a much
/// worse way to find out.
fn toolchain(name: Option<&str>) -> Result<harness_substrate::Toolchain, String> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    match name {
        None => Ok(harness_substrate::Toolchain::default()),
        Some("rust") => harness_substrate::Toolchain::rust(home.as_deref()),
        Some(other) => Err(format!(
            "`{other}` is not a toolchain this build knows; there is `rust`"
        )),
    }
}

/// Everything about *where and how* a run may act, as one value.
///
/// Grouped because they are one decision taken together — a socket or an embedded driver, the
/// cgroup that makes execution possible at all, the programs it may start and the toolchain it may
/// read — and because a function taking them one by one is a function whose call sites are seven
/// positional paths nobody can read.
struct Confinement<'a> {
    substrate: Option<&'a std::path::Path>,
    /// Whether the driver is held in this process, confining `--workspace` itself. A flag and not a
    /// path: the root is derived from the workspace, so there is nothing else to name.
    embedded: bool,
    cgroup_root: Option<&'a std::path::Path>,
    workspace_id: &'a str,
    programs: &'a [String],
    toolchain: &'a harness_substrate::Toolchain,
    /// Where the run may write. Part of the confinement because it is the same decision: what this
    /// run may do, taken before it starts and not by it.
    scope: harness_tools::Scope,
}

/// The tools this machine admits, which is the machine's answer and not a flag's.
///
/// **The publication gate, in one function**, and it is unchanged by the move to three verbs: what
/// the *model* sees is always `tool_search`, `tool_describe`, `tool_invoke`, and what the catalogue
/// behind them holds is what the machine can perform. Three entries with no backend; five with a
/// confined workspace; six inside a delegated cgroup. A tool the machine cannot confine is one
/// `tool_search` never lists.
///
/// A confinement **nobody asked for** is the read-only catalogue, which is how this harness has run
/// since it was written and a legitimate way to run now. A confinement the operator **named** and
/// the machine cannot provide refuses the run by name, because a read-only run that was asked to
/// write reports work as done that it never did.
///
/// # Errors
///
/// Names the confinement that was asked for and what stopped it being provided.
fn published(
    reading: harness_tools::LocalOperations,
    workspace_name: Option<String>,
    confinement: &Confinement<'_>,
) -> Result<harness_tools::Verbs, String> {
    let Confinement {
        substrate,
        embedded,
        cgroup_root,
        workspace_id,
        programs,
        toolchain,
        scope,
    } = confinement;
    let read_only = || {
        harness_tools::Verbs::new(
            harness_tools::Catalogue::of(reading.clone()).scoped(scope.clone()),
        )
    };

    if *embedded {
        let workspace = adopted(&reading, workspace_name, *cgroup_root, toolchain)?;
        return Ok(harness_tools::Verbs::new(
            harness_tools::Catalogue::of(harness_tools::Split::new(
                reading,
                workspace.confined(programs.to_vec()),
            ))
            .scoped(scope.clone()),
        ));
    }

    let Some(socket) = *substrate else {
        return Ok(read_only());
    };
    let client = harness_substrate::Client::at(socket);
    // `machine()` and not `probe()`. `probe`'s "unreachable is not an error" is for a harness
    // nobody pointed at a socket; here the operator did, so a daemon that is not there is the
    // answer to a question that was asked and not the absence of one.
    let facts = client.machine().map_err(|error| {
        format!(
            "no usable substrate daemon at `{}`: {error}",
            socket.display()
        )
    })?;
    // The client that probed is the client that serves: it holds the snapshot this document
    // stated, so what was published and what an exec is admitted against are one reading.
    let confined = harness_substrate::ConfinedOperations::new(
        client,
        &facts,
        *workspace_id,
        programs.to_vec(),
    );
    Ok(harness_tools::Verbs::new(
        harness_tools::Catalogue::of(harness_tools::Split::new(reading, confined))
            .scoped(scope.clone()),
    ))
}

/// One embedded driver, its machine facts and the adopted workspace, held together.
///
/// One driver and not two: the pair `published` used to open confined the same tree twice, which is
/// two `openat2` roots and two runtimes for one workspace. `machine` and `workspace_adopt` take
/// `&self`, so the instance that answered them is the one that goes on to serve the tools.
struct Adopted {
    driver: harness_substrate::Embedded,
    facts: harness_substrate::Facts,
    workspace: String,
}

impl Adopted {
    fn confined(self, programs: Vec<String>) -> harness_substrate::ConfinedOperations {
        harness_substrate::ConfinedOperations::new(
            self.driver,
            &self.facts,
            self.workspace,
            programs,
        )
    }
}

/// Opens the embedded driver over `--workspace`'s parent and adopts the workspace itself.
///
/// **One tree.** The workspace the reading provider sees *is* the confined workspace: its parent
/// becomes substrate's root and the directory itself is adopted. Opening a fresh empty workspace
/// beside it would mean a run read one tree and wrote into another, and was not doing the task it
/// had been given.
///
/// # Errors
///
/// Names which of the four things `--substrate-embedded` needs was not there. Each is a refusal
/// rather than a quiet fall-back to reading, because the operator asked for write and execute.
fn adopted(
    reading: &harness_tools::LocalOperations,
    workspace_name: Option<String>,
    cgroup_root: Option<&std::path::Path>,
    toolchain: &harness_substrate::Toolchain,
) -> Result<Adopted, String> {
    let tree = reading.root().display().to_string();
    let Some(root) = reading.root().parent() else {
        return Err(format!(
            "`--substrate-embedded` cannot adopt `{tree}`: it has no parent directory to be \
             substrate's root"
        ));
    };
    let Some(name) = workspace_name else {
        return Err(format!(
            "`--substrate-embedded` cannot adopt `{tree}`: its own name is not readable, so there \
             is nothing for substrate to represent the workspace as"
        ));
    };
    // Checked here, before the driver is opened, so the message a rename fixes is the harness's own
    // and names the flag the operator typed rather than surfacing a driver's refusal from inside.
    if !name.starts_with("ws_")
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(format!(
            "`--substrate-embedded` cannot adopt `{name}`: the directory must be named `ws_` \
             followed by alphanumerics and underscores, because substrate's guarded filesystem \
             represents no other name. Rename it, or drop the flag for a read-only run."
        ));
    }
    let driver = harness_substrate::Embedded::open_with(
        root,
        cgroup_root.map(std::path::Path::to_path_buf),
        toolchain.clone(),
    )
    .map_err(|error| format!("the embedded substrate driver did not open: {error}"))?;
    let facts = harness_substrate::Backend::machine(&driver)
        .map_err(|error| format!("the embedded substrate driver has no machine facts: {error}"))?;
    let workspace = driver
        .workspace_adopt(&name)
        .map_err(|error| format!("`--substrate-embedded` could not adopt `{name}`: {error}"))?;
    Ok(Adopted {
        driver,
        facts,
        workspace,
    })
}

fn tools_command(options: &ToolsOptions) -> Result<(), String> {
    let tools = published(
        harness_tools::LocalOperations::new(&options.workspace)?,
        workspace_name(&options.workspace),
        &Confinement {
            substrate: options.substrate.as_deref(),
            embedded: options.substrate_embedded,
            cgroup_root: options.cgroup_root.as_deref(),
            workspace_id: &options.workspace_id,
            programs: &options.allow_program,
            toolchain: &toolchain(options.toolchain.as_deref())?,
            scope: write_scope(&options.write_scope)?,
        },
    )?;
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
    fn the_approval_ceiling_reaches_the_loop_and_does_not_combine_with_yes() {
        assert_eq!(
            ceiling(&options(&[])),
            Risk::Low,
            "the default asks about everything but reads"
        );
        assert_eq!(
            ceiling(&options(&["--approve-up-to", "medium"])),
            Risk::Medium
        );
        assert_eq!(ceiling(&options(&["--approve-up-to", "high"])), Risk::High);
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
                "--approve-up-to",
                "high",
                "--yes",
            ])
            .is_err(),
            "--yes approves everything, so a ceiling beside it would be a flag that does nothing"
        );
        assert!(parse(&["b10x-harness", "run", "--approve-up-to", "critical"]).is_err());
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

    /// A read-only catalogue over a temporary tree, for the instruction tests.
    fn a_catalogue(dir: &std::path::Path) -> harness_tools::Catalogue {
        harness_tools::Catalogue::of(harness_tools::LocalOperations::new(dir).expect("opens"))
    }

    #[test]
    fn the_default_instruction_carries_the_catalogue_so_nothing_has_to_be_discovered() {
        // 33-44% of every tool call across three live runs was `tool_search` or `tool_describe`:
        // four of ten spent finding out what exists, each a billed round trip replayed in every
        // later turn. The answer is the same on every turn, so it belongs in the instructions -
        // which are also the only half of a request a prompt cache can hold.
        let dir = tempfile::tempdir().expect("temporary directory");
        let catalogue = a_catalogue(dir.path());
        let text = instructions(&options(&[]), &catalogue, "").expect("the default is available");

        for entry in ["file_read", "dir_list", "search"] {
            assert!(text.contains(entry), "`{entry}` is named up front: {text}");
        }
        assert!(text.contains("file.read"), "with its operation: {text}");
        assert!(text.contains("max_bytes"), "and its arguments: {text}");
        assert!(
            !text.contains("file_write"),
            "and nothing this read-only run does not have: {text}"
        );
    }

    #[test]
    fn the_default_instruction_points_at_the_catalogue_and_promises_no_effects() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let catalogue = a_catalogue(dir.path());
        let text = instructions(&options(&[]), &catalogue, "").expect("the default is available");
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
    fn a_declared_scope_is_stated_up_front_so_no_turn_is_spent_discovering_it() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let catalogue = a_catalogue(dir.path()).scoped(harness_tools::Scope::of(vec![
            harness_tools::ScopeRule::parse(".engineering/planning/**=partial-only")
                .expect("a rule"),
        ]));
        let text = instructions(&options(&[]), &catalogue, "").expect("the default is available");

        assert!(text.contains(".engineering/planning/**"), "{text}");
        assert!(text.contains("file_edit"), "and the way in: {text}");
    }

    #[test]
    fn a_silent_scope_still_binds_the_tools_it_is_simply_not_said_out_loud() {
        // The experiment control. A run told the rule and a run refused the rule both end with the
        // rule kept; only the second shows that the toolset is what kept it.
        let dir = tempfile::tempdir().expect("temporary directory");
        let scope = harness_tools::Scope::of(vec![
            harness_tools::ScopeRule::parse(".engineering/planning/**=partial-only")
                .expect("a rule"),
        ]);
        let catalogue = a_catalogue(dir.path()).scoped(scope);
        let text = instructions(&options(&["--scope-announce", "silent"]), &catalogue, "")
            .expect("the default is available");

        assert!(!text.contains(".engineering/planning/**"), "{text}");
        assert!(
            !catalogue.scope().is_empty(),
            "the tools are bound either way — that is the whole point"
        );
    }

    #[test]
    fn an_instruction_file_replaces_the_default() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("instructions.md");
        fs::write(&path, "be terse").expect("write");
        let options = options(&["--instructions-file", path.to_str().expect("utf-8 path")]);
        let dir2 = tempfile::tempdir().expect("temporary directory");
        // Verbatim, catalogue and all: appending to a document somebody wrote would make the run's
        // instruction something the file alone cannot reproduce.
        assert_eq!(
            instructions(&options, &a_catalogue(dir2.path()), "").expect("readable"),
            "be terse"
        );
    }
}
