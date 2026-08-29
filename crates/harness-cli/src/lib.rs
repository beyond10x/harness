#![forbid(unsafe_code)]

//! The b10x harness, driven from the command line.
//!
//! The command is a thin shell: it resolves an endpoint, a credential and a workspace, then hands
//! them to [`harness_loop`]. Everything interesting happens in the loop, which is what lets the
//! same core run embedded in another process or behind a bridge without a second implementation.

pub mod agents;
pub mod approve;
pub mod contract;
pub mod environment;
pub mod hooks;
mod metaharness;
pub mod profile;
pub mod provider;
mod render;
pub mod skills;
pub mod transcript;
mod workflow;

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
    RunLedger,
};
use harness_wire::{ModelPort, Risk, Sampling, StaticBearer};

pub use render::Renderer;

/// The sentences every surface carries, whatever the model is offered.
///
/// # What this used to say, and what it cost
///
/// Until the three verbs landed, this text named `workspace_list`, `workspace_read` and
/// `workspace_grep` and told the model *"nothing you can call changes a file or runs a command, so
/// say what you would change rather than claiming you changed it"*. Both halves went stale: those
/// tools no longer exist under any name, and the catalogue behind the verbs now reaches seven
/// entries on a machine that can confine a process.
///
/// It was not a harmless leftover. A run given a write-and-execute catalogue and this instruction
/// was told in the same breath that it could do neither — and the measured result was a model that
/// searched for read-only tools, read two files, changed nothing, and reported the task done.
/// **The instruction had asked it to.**
///
/// So this text states **no effects of its own**. What names the effects is the surface below,
/// rendered from the live catalogue, which cannot describe a tool this run does not have.
const GROUNDING_INSTRUCTIONS: &str = "\
Ground every claim about the workspace in something you actually read, and say plainly when you \
have not looked. Never report work as done unless a tool you called made it so. A call that was \
not approved did not happen: do not retry it — do what you can without it and say plainly what \
you could not do.";

/// The opening sentence under [`Surface::Verbs`], which is unchanged.
const VERBS_INSTRUCTIONS: &str = "\
You are the b10x coding harness. Everything you can do reaches you through three tools: \
`tool_search` lists what this run has, `tool_describe` gives one entry's input schema, and \
`tool_invoke` calls it — `tool_invoke` is the only one that acts. ";

/// The opening sentence under [`Surface::Flat`].
const FLAT_INSTRUCTIONS: &str = "\
You are the b10x coding harness. Everything you can do is one of the tools you have been given, \
called by its own name; there is no other way to act, and there is nothing to discover first. ";

/// The tools the **loop** owns for this run, when it was asked for them.
///
/// Neither is a catalogue entry and no `ToolPort` ever sees either, so the surface rendered from
/// the catalogue cannot name them: `answer` performs no operation on a machine, and `delegate`
/// performs whatever this run's own tools perform, through this run's own gate.
#[derive(Debug, Default, Clone, Copy)]
struct Owned<'a> {
    /// The delegate tool's name, under `--delegate`.
    delegate: Option<&'a str>,
    /// The answer tool's name, under `--output-schema`.
    answer: Option<&'a str>,
    /// The named agents this run may delegate as, under `--agents-dir` and `--plugin-dir`.
    agents: Option<&'a harness_loop::Agents>,
    /// The skills this run offers, under `--skills-dir` and `--plugin-dir`.
    ///
    /// The whole value rather than a name, because the instruction needs the one-line-per-skill
    /// block and the name check needs the tool name, and reading them out of one place is how
    /// they cannot disagree.
    skills: Option<&'a harness_loop::Skills>,
}

/// The standing instruction for this run, written for the surface the model is offered.
///
/// # Under `verbs`, the catalogue goes in the instruction
///
/// **The catalogue belongs in the instructions, not in the conversation.** Discovering it through
/// `tool_search` and `tool_describe` cost 33–44% of every tool call across three measured runs —
/// four calls of ten spent finding out what exists, each a billed round trip that is then replayed
/// in every later turn. And the answers landed in the conversation, which grows and is re-sent at
/// the full input rate, rather than in the instructions, which are identical every turn and are
/// what a prompt cache can hold.
///
/// # Under `flat`, only the names go in — and that is deliberate
///
/// [`harness_tools::Catalogue::brief`] renders every entry's **input schema**, and under a flat
/// surface those schemas are already in `tools`: one per published tool, on every request, in the
/// half of the request a prompt cache holds. Pasting them here would send each schema twice per
/// turn to say the same thing, and the copy in the instruction is the one the provider cannot
/// validate against. So the instruction names the entries in one line — that is what a model needs
/// to plan with — and the shapes stay where the provider reads them.
fn standing_instruction(
    catalogue: &harness_tools::Catalogue,
    surface: Surface,
    context: &str,
    announce: bool,
    owned: Owned<'_>,
) -> String {
    let mut text = match surface {
        Surface::Flat => format!(
            "{FLAT_INSTRUCTIONS}{GROUNDING_INSTRUCTIONS}\n\nThe tools this run has: {}. Their \
             arguments are in the tool definitions themselves.",
            catalogue
                .entries()
                .iter()
                .map(|entry| format!("`{}`", entry.name))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Surface::Verbs => format!(
            "{VERBS_INSTRUCTIONS}{GROUNDING_INSTRUCTIONS}\n\nThis run's catalogue, which is what \
             `tool_invoke` will accept — call `tool_search` only if you need to re-check \
             it:\n\n{}",
            catalogue.brief()
        ),
    };
    // The two tools the loop owns are not in the catalogue and cannot be rendered from it, so
    // they are named here — one line each. Their own descriptions carry the rest, and repeating
    // those here would send the same words twice per turn.
    if let Some(name) = owned.delegate {
        let _ = write!(
            text,
            "\n\n`{name}` hands one self-contained sub-task to a fresh context with these same \
             tools. It cannot see this conversation, so the task must say everything it needs, and \
             it reports back once, in text."
        );
    }
    if let Some(name) = owned.answer {
        let _ = write!(
            text,
            "\n\nFinish by calling `{name}` with the result, once and on its own. Nothing you \
             write outside it is read as the answer."
        );
    }
    // **The descriptions and never the bodies.** This is the whole of what a run that never loads
    // a skill is told about it, and it is what makes the tool reachable at all — a `skill` entry
    // whose library is unlisted is a tool the model has no reason to call. The bodies stay behind
    // the call, because a stateless loop replays its conversation and a body here is billed on
    // every turn of every run, including the ones that never wanted it.
    if let Some(skills) = owned.skills.filter(|skills| !skills.is_empty()) {
        text.push_str(&skills.brief());
    }
    // Only where a delegate exists to run them with: an agent named in the instruction that the
    // model has no tool to invoke is a turn spent discovering that.
    if let Some(agents) = owned
        .agents
        .filter(|agents| !agents.is_empty() && owned.delegate.is_some())
    {
        text.push_str(&agents.brief());
    }
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

/// Which model API this run speaks.
///
/// A flag and not a guess. Two wires reach different endpoints under the same `--base-url`, take
/// different credential headers, and carry opaque items that may not cross between them — so a
/// harness that inferred the wire from the URL would be one whose run failed at the far side
/// instead of at the command line. The default is the wire this harness shipped with, so every
/// existing invocation means what it did before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
enum Wire {
    /// `POST {base-url}/responses`.
    #[default]
    OpenaiResponses,
    /// `POST {base-url}/messages`.
    AnthropicMessages,
}

impl Wire {
    /// The identifier this wire tags its opaque items with.
    ///
    /// Taken from each wire crate's own constant rather than written out again here: a session
    /// refused for being on the wrong wire has to compare the same bytes the loop compares, or the
    /// refusal would be about a name only this file believes in.
    ///
    /// # Panics
    ///
    /// Only if a wire crate's own constant stops being a legal wire identifier, which its own
    /// tests pin.
    fn id(self) -> harness_wire::WireId {
        let name = match self {
            Self::OpenaiResponses => harness_responses::WIRE,
            Self::AnthropicMessages => harness_messages::WIRE,
        };
        harness_wire::WireId::new(name).expect("a wire crate's own identifier is valid")
    }
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

/// How the catalogue reaches the model.
///
/// Two surfaces over one catalogue, and the choice is a deployment decision rather than a claim
/// about what the run may do — the catalogue is the same either way, and so is the approval gate.
///
/// `flat` is the default because of what the indirection measured: across three live runs **33–44%
/// of every tool call was `tool_search` or `tool_describe`**, and `tool_invoke.arguments` is an
/// untyped object the provider cannot validate, so a misspelled field becomes a failed call and
/// another turn. The reason the verbs existed — neutral names across harnesses — is met by the
/// entry names themselves, which is what `harness_tools::operation_of` maps.
///
/// `verbs` stays, and is not a legacy setting: metaharness serves the three-verb surface over MCP,
/// and an evaluation arm that compares the two needs both to be reachable from a flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
enum Surface {
    /// Every catalogue entry as its own tool, with its own schema.
    #[default]
    Flat,
    /// `tool_search`, `tool_describe`, `tool_invoke` over the catalogue.
    Verbs,
}

/// Who decides a call the run's ceiling does not cover.
///
/// The library's default approver is `DenyAll` and stays so (`AGENTS.md` invariant 12): what this
/// chooses is the **command line's** approver, which is a different question. Until this existed a
/// person at the terminal had `--yes` — approve everything for the whole run because one write was
/// wanted — or nothing, and a run that needed one approved write could not be done at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
enum Approve {
    /// Ask a person when there is one, and refuse when there is not.
    #[default]
    Auto,
    /// Ask a person over `/dev/tty`, or refuse the run by name when there is no terminal.
    Prompt,
    /// Refuse everything above the ceiling, and tell the model it was refused.
    Deny,
    /// Approve everything above the ceiling. The declared unattended run; `--yes` spells it too.
    All,
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

/// `profiles <verb>`.
#[derive(Debug, Subcommand)]
enum ProfilesCommand {
    /// Every profile, and the file it came from.
    List,
    /// One profile's effective table.
    Show { name: String },
    /// What a `-p` expands to, and who set each key.
    Explain {
        #[arg(short = 'p', long = "profile", value_name = "NAME")]
        profile: Vec<String>,
    },
    /// Write a starter config, and print where it went.
    Init,
}

/// `providers <verb>`.
#[derive(Debug, Subcommand)]
enum ProvidersCommand {
    /// Every provider this build ships, and which the config overrides.
    List,
    /// One provider's effective endpoint, wire, model and credential.
    Show { name: String },
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run one request to completion.
    Run(Box<RunCommand>),
    /// Ask one question after another over the same session, reading a line at a time.
    ///
    /// The same flags as `run` without `--input`: each line of standard input is a follow-up turn
    /// on one conversation, and the session is written after every one of them. `exit`, or the end
    /// of the input, ends it. There is no line editing and no history — a shell already has both,
    /// and a harness that reimplemented them would own a terminal library forever.
    Chat(Box<RunOptions>),
    /// List the sessions on this machine, newest first.
    Sessions(SessionsOptions),
    /// Print the tools this harness publishes, without contacting an endpoint.
    Tools(Box<ToolsOptions>),
    /// Read the profiles this machine is configured with, without running one.
    ///
    /// A profile decides what a run may do, so it has to be readable before it does it — that is
    /// the condition on which one is allowed to carry a permission at all. `explain` is the verb
    /// that matters: it prints the argv a `-p` expands to, and who set each key, for nothing.
    #[command(subcommand)]
    Profiles(ProfilesCommand),
    /// Read the providers this build ships, and any the config overrides.
    #[command(subcommand)]
    Providers(ProvidersCommand),
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
    /// Walk a workflow document: one turn of the loop per step, one conversation per section.
    ///
    /// Two verbs over one notation. `plan` validates a document and says what runs in what order,
    /// contacting nothing; `run` walks it. The scheduler is `harness-flow`'s and knows nothing
    /// about a model — what a step *is* lives in `workflow.rs`, which is what lets the whole walk
    /// be tested without a provider.
    #[command(subcommand)]
    Workflow(workflow::WorkflowCommand),
}

#[derive(Debug, Args)]
struct SessionsOptions {
    /// Where sessions are kept. Defaults to `$XDG_STATE_HOME/b10x-harness/sessions`.
    #[arg(long, value_name = "PATH")]
    session_dir: Option<PathBuf>,
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
#[command(group = clap::ArgGroup::new("oauth_source").args(["oauth_token_file", "oauth_token_env"]))]
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
    /// File holding a subscription OAuth token, for a route that takes one instead of an API key.
    ///
    /// Named, like every other credential source here: there is no default path and no vendor
    /// directory this looks in. The file is re-read on **every** call, so a token an owner outside
    /// this process renews is picked up without restarting the run — nothing here renews one.
    #[arg(long, conflicts_with_all = ["api_key_file", "api_key_env", "oauth_token_env"])]
    oauth_token_file: Option<PathBuf>,
    /// Name of an environment variable holding a subscription OAuth token.
    #[arg(long, conflicts_with_all = ["api_key_file", "api_key_env", "oauth_token_file"])]
    oauth_token_env: Option<String>,
    /// JSON pointer to the token inside the named source, when that source is a JSON document.
    ///
    /// Absent means the whole source is the token. Named rather than known: which field a given
    /// credential store puts its access token in is that store's business, and a built-in path
    /// would silently read the wrong field the day it changed.
    #[arg(long, value_name = "POINTER", requires = "oauth_source")]
    oauth_token_pointer: Option<String>,
    /// Which model API to speak. Both reach `--base-url`; they are different endpoints under it.
    #[arg(long, value_name = "WIRE", default_value = "openai-responses")]
    wire: Wire,
    /// Ceiling on model turns, applied to every turn on the connection.
    #[arg(long)]
    max_turns: Option<u64>,
    /// Ceiling on total reported output tokens per turn.
    #[arg(long)]
    max_output_tokens: Option<u64>,
}

// A command line is a struct of switches; counting them says nothing about the type.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Args)]
#[command(group = clap::ArgGroup::new("oauth_source").args(["oauth_token_file", "oauth_token_env"]))]
struct RunOptions {
    /// Profiles to apply, in order. Later wins a contested key; a typed flag beats them all.
    ///
    /// Read from `$XDG_CONFIG_HOME/b10x/harness.toml`. A profile decides what a run may **do**, so
    /// `b10x-harness profiles explain -p <name>` prints the argv it expands to before a token is
    /// spent, and `session.started` names every profile that contributed with a digest of what it
    /// said. That record is the condition on which a file is allowed to carry a permission at all.
    #[arg(short = 'p', long = "profile", value_name = "NAME")]
    profile: Vec<String>,
    /// The provider whose default supplied the credential, when one did.
    ///
    /// Not a flag: nothing sets it but [`apply_profiles`]. It exists so the run's record can say
    /// `credential_source: "provider:<name>"` rather than the flat `"named"` — which is the audit
    /// that pays for a defaulted credential path at all.
    #[arg(skip)]
    credential_from_provider: Option<String>,
    /// The profiles that configured this run, for its record. Set by [`apply_profiles`] alone.
    #[arg(skip)]
    applied_profiles: Vec<profile::ProfileRef>,
    /// Endpoint origin plus API prefix, for example `https://llmgw.example/v1`.
    ///
    /// Optional only because a provider can supply it — `[default] provider = "claude"`. A run with
    /// neither is refused by name rather than aimed at a default endpoint nobody chose.
    #[arg(long)]
    base_url: Option<String>,
    /// Exact model identifier the endpoint serves. As `--base-url`: a provider may supply it.
    #[arg(long)]
    model: Option<String>,
    /// Context window the endpoint serves for that model.
    ///
    /// It bounds the request the wire will build, and it is also what makes compaction
    /// token-aware: the loop compacts at 80% of this figure — measured by the provider's own last
    /// reported input count where there is one — and frees down to 50%, instead of the fixed
    /// 192 KiB byte rule that left roughly 60% of a 128k window unused.
    #[arg(long, default_value_t = 128_000)]
    context_window: u64,
    /// File holding the bearer credential. Its contents are trimmed and never logged.
    #[arg(long, conflicts_with = "api_key_env")]
    api_key_file: Option<PathBuf>,
    /// Name of an environment variable holding the bearer credential. Naming it is deliberate:
    /// the harness reads no credential it was not pointed at.
    #[arg(long, conflicts_with = "api_key_file")]
    api_key_env: Option<String>,
    /// File holding a subscription OAuth token, for a route that takes one instead of an API key.
    ///
    /// Named, like every other credential source here: there is no default path and no vendor
    /// directory this looks in. The file is re-read on **every** call, so a token an owner outside
    /// this process renews is picked up without restarting the run — nothing here renews one.
    #[arg(long, conflicts_with_all = ["api_key_file", "api_key_env", "oauth_token_env"])]
    oauth_token_file: Option<PathBuf>,
    /// Name of an environment variable holding a subscription OAuth token.
    #[arg(long, conflicts_with_all = ["api_key_file", "api_key_env", "oauth_token_file"])]
    oauth_token_env: Option<String>,
    /// JSON pointer to the token inside the named source, when that source is a JSON document.
    ///
    /// Absent means the whole source is the token. Named rather than known: which field a given
    /// credential store puts its access token in is that store's business, and a built-in path
    /// would silently read the wrong field the day it changed.
    #[arg(long, value_name = "POINTER", requires = "oauth_source")]
    oauth_token_pointer: Option<String>,
    /// Which model API to speak. Both reach `--base-url`; they are different endpoints under it.
    #[arg(long, value_enum)]
    wire: Option<Wire>,
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
    /// A program on this host, staged and admitted read-only so a confined `run` can execute it.
    ///
    /// **Allow-listing a program by absolute path admits its name, not its bytes.** The sandbox
    /// reaches `/usr`, `/bin`, `/lib`, `/lib64` and the workspace; a path outside those is not
    /// there, so the exec dies at `ENOENT` and the model reads that as *the command is wrong*.
    /// Naming it here links exactly that one file into a private directory and mounts it read-only
    /// at `/toolchain/driver`, and adds the mounted path to the allow-list, so declaring it once
    /// is the whole declaration.
    ///
    /// Read-only on purpose: a run that could rewrite the program recording its own evidence has
    /// no evidence. The digest of what was staged is reported by `tools`.
    #[arg(long, value_name = "PATH")]
    driver: Option<PathBuf>,
    /// A program `run` may start. Repeatable, and an empty set publishes no `run` at all.
    ///
    /// Declared rather than open, because an argv whose program could be anything is a shell with
    /// extra steps. A set nobody named means nobody wanted one.
    #[arg(long)]
    allow_program: Vec<String>,
    /// How the catalogue reaches the model: every entry as its own tool, or three verbs over it.
    ///
    /// `flat` by default. The three-verb surface cost 33–44% of every tool call on discovery
    /// across three measured runs and hands the provider an untyped argument object it cannot
    /// validate; the neutral names it existed to protect are the entry names, which `flat`
    /// publishes directly. `verbs` is still fully served — metaharness offers it over MCP — and is
    /// what an arm comparing the two surfaces asks for.
    #[arg(long, value_name = "SURFACE", default_value = "flat")]
    surface: Surface,
    /// File holding the standing instruction. Defaults to the built-in one.
    #[arg(long)]
    instructions_file: Option<PathBuf>,
    /// Where this run's conversation is written, so a later one can resume it.
    ///
    /// Defaults to `$XDG_STATE_HOME/b10x-harness/sessions`, or `$HOME/.local/state/…` — never the
    /// workspace, because a transcript carries whatever the model read and a file beside the code
    /// is one `git add -A` from being committed.
    #[arg(long, value_name = "PATH")]
    session_dir: Option<PathBuf>,
    /// Continue a session: its identifier, or `latest`.
    ///
    /// The stored conversation is replayed into the run before this run's input, opaque reasoning
    /// items included, so the model keeps what it already worked out instead of paying to think it
    /// again. A session recorded on another wire is refused by name — an opaque item may not cross
    /// wires (`AGENTS.md` invariant 5), and saying so here is cheaper than the far end saying it.
    #[arg(long, value_name = "ID", conflicts_with = "no_session")]
    resume: Option<String>,
    /// Write no session file at all.
    ///
    /// For an evaluation arm that must leave nothing on the machine it ran on: a transcript is
    /// evidence about a run, and an arm whose runs must be identically reproducible from their
    /// flags cannot have one of them silently pick up the previous one's state. Nothing is
    /// resumable afterwards, which is the trade.
    #[arg(long)]
    no_session: bool,
    /// Leave the project's own `AGENTS.md` or `CLAUDE.md` out of the standing instruction.
    ///
    /// An experiment control, and only that: the environment block — workspace, OS, date, git
    /// branch — is always there, because a run that does not know where it is spends a turn
    /// finding out. What this removes is the project's own words, so a run can be measured against
    /// the toolset rather than against the instructions a repository happens to carry.
    #[arg(long)]
    no_project_instructions: bool,
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
    /// A directory of skills the run may load by name. Repeatable.
    ///
    /// `<DIR>/<name>/SKILL.md`, YAML frontmatter with `name` and `description`, the document
    /// after. That is the on-disk shape Claude Code writes, read here so a plugin written for it
    /// runs unchanged and a comparison between the two harnesses is a comparison of harnesses.
    /// Reading a vendor's **file format** is not becoming a client of a vendor **protocol** — the
    /// distinction `README.md` draws where it refuses an MCP client; nothing here speaks to a
    /// server or gives anyone a say in what this run may do.
    ///
    /// **The descriptions are given to the model and the bodies are not.** A skill costs a `skill`
    /// call when the model decides it wants one, rather than input tokens on every turn of every
    /// run — which is what `--context` does, deliberately, for the files a run genuinely needs
    /// throughout. A document this build cannot read refuses the run by name rather than being
    /// skipped: a skill half-read is a rule its author wrote that the run would not apply.
    #[arg(long, value_name = "DIR")]
    skills_dir: Vec<PathBuf>,
    /// A directory of named agents a delegate may be run as. Repeatable.
    ///
    /// `<DIR>/<name>.md`, YAML frontmatter with `name`, `description` and an optional `tools`
    /// list, the body after as the agent's own standing instruction. The vendor's shape, read
    /// here for the same reason `--skills-dir` reads theirs.
    ///
    /// **A declared toolset can only narrow this run, never widen it.** The names are mapped to
    /// this harness's own and then intersected with what this run was admitted, so an agent
    /// naming a tool the machine did not give the parent does not get it — and the child's record
    /// says by name what it asked for and did not get, rather than going silent. A vendor tool
    /// name this build does not map refuses the document: a permission its author granted and
    /// this build quietly dropped is one the run would not have, with nothing saying so.
    ///
    /// Only where `--delegate` published a delegate to run them with.
    #[arg(long, value_name = "DIR")]
    agents_dir: Vec<PathBuf>,
    /// A plugin directory, whose `skills/` and `agents/` are read as the two flags above read them.
    ///
    /// Exactly equivalent to `--skills-dir <DIR>/skills` and `--agents-dir <DIR>/agents`, with
    /// each name qualified `<plugin>:<name>` from the plugin's own manifest — the vendor's
    /// namespacing, and not cosmetic: two plugins may both ship a `planning`, and a bare name is
    /// also not what an expectation comparing two harnesses names.
    ///
    /// Named separately so this and the
    /// vendor arm of a comparison take the same flag with the same argument. Repeatable, and
    /// composes with both.
    #[arg(long, value_name = "DIR")]
    plugin_dir: Vec<PathBuf>,
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
    /// Approve every call that asks for a decision. The same as `--approve all`.
    ///
    /// What asks is the loop's answer, taken per call from the catalogue entry's declared risk
    /// against a ceiling that defaults to low: every write and every `run` asks. Kept as its own
    /// spelling because it is what every existing unattended invocation says, and it wins over
    /// `--approve` when both are given.
    #[arg(long)]
    yes: bool,
    /// Who decides a call above the ceiling: `auto`, `prompt`, `deny` or `all`.
    ///
    /// `auto` — the default — asks a person over `/dev/tty` when there is a terminal to ask on and
    /// stdin and stderr are one, and otherwise denies, saying so in one line before the run rather
    /// than leaving it to be discovered from a refusal. `prompt` asks or refuses the run by name.
    /// `deny` is the library default, `DenyAll`. `all` is the declared unattended run.
    ///
    /// This chooses the **command line's** approver. The library's default is `DenyAll` whatever
    /// this says, and a harness that approved by default would turn a review gate into decoration
    /// (`AGENTS.md` invariant 12).
    #[arg(long, value_name = "MODE", default_value = "auto")]
    approve: Approve,
    /// Run calls at or below this risk without asking. Default `low`.
    ///
    /// The ceiling the loop judges each call's declared risk against. A `file_write` and a
    /// `file_edit` are `medium` and a `run` is `high`, so at `high` all three run unasked and only
    /// a destructive call asks. Above the ceiling the approver decides, and with `--approve deny`
    /// that is a refusal the model is told about. `--yes` approves everything and makes this moot,
    /// so the two do not combine.
    #[arg(long, value_name = "RISK", conflicts_with = "yes")]
    approve_up_to: Option<ApproveUpTo>,
    /// Let the model hand a self-contained sub-task to a fresh context on the same gate.
    ///
    /// Off by default: a new tool is a change in what the model can do. The delegate sees this
    /// run's standing instruction and its task and **not** this conversation, gets the same tool
    /// port, the same approver and the same hooks, and spends the run's remaining budget — so it
    /// widens nothing and costs the parent one tool result instead of forty reads. It cannot
    /// delegate in turn.
    #[arg(long)]
    delegate: bool,
    /// Model turns one delegate may take before it reports what it got to.
    ///
    /// Its own ceiling and not the parent's remainder, so a child that loops does not spend the
    /// run's remaining turns finding out.
    ///
    /// **At least one**, refused here rather than in the child. The parent's own `--max-turns 0`
    /// is refused before the first request, and a bound that admits no turn has to be refused the
    /// same way wherever it is written; left to the delegate's own `Budget::validate` a typed zero
    /// becomes a failed tool result on *every* delegation, each one after a paid parent turn had
    /// already asked for one.
    #[arg(
        long,
        value_name = "N",
        default_value_t = harness_loop::DELEGATE_MAX_TURNS,
        value_parser = clap::value_parser!(u64).range(1..),
        requires = "delegate"
    )]
    delegate_turns: u64,
    /// A file declaring the operator's own programs to run at each call and at the end.
    ///
    /// Named here and **never discovered**: a hook found in the workspace would be a program the
    /// repository runs on this machine, which is the ambient fallback this harness refuses for
    /// credentials. A hook can only narrow — `before-call` fires after the approver said yes and
    /// its block is one more refusal — and it is spawned as an argv, never through a shell.
    ///
    /// It is otherwise **unconfined**: the operator's own program, with the environment this run
    /// was started in — minus the variable `--api-key-env` or `--oauth-token-env` named. A hook is
    /// trusted to act; it is not handed the key this run authenticates with.
    #[arg(long, value_name = "FILE")]
    hooks: Option<PathBuf>,
    /// Emit one JSON event per line on stdout instead of prose.
    #[arg(long)]
    json: bool,
    /// Keep progress off stderr. Warnings are still reported.
    #[arg(long)]
    quiet: bool,
}

/// `run`: everything `chat` takes, plus the one question this invocation asks.
///
/// Split from [`RunOptions`] rather than made optional, so that `run` without `--input` is a parse
/// error and `chat` never carries a flag it would ignore. A flag that exists and does nothing is
/// how `--substrate-embedded` came to demand a value it threw away.
#[derive(Debug, Clone, Args)]
struct RunCommand {
    #[command(flatten)]
    options: RunOptions,
    /// The request.
    #[arg(long)]
    input: String,
    /// A JSON Schema for an object, published as a tool the model calls to finish.
    ///
    /// **Standard output is then the answer and nothing else** — one line of compact JSON — so
    /// the command composes with anything that reads JSON; the model's prose goes to stderr as
    /// progress. A run that ends in prose anyway is nudged once and then stops
    /// `Unstructured`, exiting 2: prose on stdout with a success status is the silent failure this
    /// harness refuses to produce.
    ///
    /// Under `--json` stdout is the event record instead and there is no bare answer line at all:
    /// the answer is the **last** `answered` event before a `finished` whose `stop` is
    /// `completed`. Last and not first, because a `stop` hook may withdraw an ending — the loop
    /// clears the structured answer and turns again — so an earlier `answered` can be followed by
    /// a second one, or by a `finished` that is `unstructured`. A driver taking the first would
    /// take the value the operator's hook refused.
    ///
    /// `run` only. A conversation has no single end, so there is nothing for `chat` to shape.
    #[arg(long, value_name = "FILE")]
    output_schema: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ToolsOptions {
    /// How the catalogue would reach the model: every entry as its own tool, or three verbs.
    ///
    /// The same flag `run` takes and the same default, because the question this command answers
    /// is *what would that run publish* — and answering it for a surface the run would not use is
    /// how a consumer comes to pin a tool list nothing serves.
    #[arg(long, value_name = "SURFACE", default_value = "flat")]
    surface: Surface,
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
    /// A program on this host, staged and admitted read-only so a confined `run` can execute it.
    ///
    /// **Allow-listing a program by absolute path admits its name, not its bytes.** The sandbox
    /// reaches `/usr`, `/bin`, `/lib`, `/lib64` and the workspace; a path outside those is not
    /// there, so the exec dies at `ENOENT` and the model reads that as *the command is wrong*.
    /// Naming it here links exactly that one file into a private directory and mounts it read-only
    /// at `/toolchain/driver`, and adds the mounted path to the allow-list, so declaring it once
    /// is the whole declaration.
    ///
    /// Read-only on purpose: a run that could rewrite the program recording its own evidence has
    /// no evidence. The digest of what was staged is reported by `tools`.
    #[arg(long, value_name = "PATH")]
    driver: Option<PathBuf>,
    /// The skills a `run` would be offered, so `tools` answers with them.
    ///
    /// The same declaration `run` takes. It belongs in this answer for the reason `--write-scope`
    /// does: "what can this run do?" is not answered by a list of tools alone once some of what
    /// the model is given arrives from somewhere else.
    #[arg(long, value_name = "DIR")]
    skills_dir: Vec<PathBuf>,
    /// The named agents a `run` would be offered, so `tools` answers with them.
    #[arg(long, value_name = "DIR")]
    agents_dir: Vec<PathBuf>,
    /// A plugin directory, whose `skills/` and `agents/` are read as the two flags above read them.
    #[arg(long, value_name = "DIR")]
    plugin_dir: Vec<PathBuf>,
}

/// Reads a credential from exactly the place the caller named, or none.
///
/// There is no ambient fallback: a harness that quietly picks up a key from the environment is one
/// whose runs cannot be explained afterwards. **Naming neither source is itself a declaration** —
/// the run sends no `authorization` header, which is right for a gateway on this machine that
/// authenticates nobody, and for a run deliberately started with no credential whose first request
/// is meant to be refused by the far end.
impl RunOptions {
    /// The endpoint, after profiles and providers have been applied.
    ///
    /// # Panics
    ///
    /// Never after [`apply_profiles`], which fills it or refuses the run. Reached before that only
    /// by a caller that skipped resolution, which is a programming error rather than an operator's.
    fn base_url(&self) -> &str {
        self.base_url
            .as_deref()
            .expect("`apply_profiles` fills the endpoint or refuses the run")
    }

    /// The model, after profiles and providers have been applied.
    ///
    /// # Panics
    ///
    /// As [`RunOptions::base_url`].
    fn model(&self) -> &str {
        self.model
            .as_deref()
            .expect("`apply_profiles` fills the model or refuses the run")
    }

    /// The wire, defaulted last so a provider can set it and a typed flag can beat it.
    fn wire(&self) -> Wire {
        self.wire.unwrap_or_default()
    }
}

/// What the record will say about where this run's credential came from.
///
/// `provider:<name>` where a provider defaulted it, `named` where the operator pointed at it. The
/// distinction is the whole of what makes a defaulted vendor path acceptable rather than the
/// ambient fallback `resolve_credential` refuses.
fn credential_source(options: &RunOptions) -> String {
    options
        .credential_from_provider
        .as_ref()
        .map_or_else(|| "named".to_owned(), |name| format!("provider:{name}"))
}

/// The profiles that configured this run, in the loop's own shape.
fn applied_profiles(options: &RunOptions) -> Vec<harness_loop::ProfileRef> {
    options
        .applied_profiles
        .iter()
        .map(|used| harness_loop::ProfileRef {
            name: used.name.clone(),
            source: used.source.clone(),
            sha256: used.sha256.clone(),
        })
        .collect()
}

/// Fills the endpoint, wire, model and credential from the provider a profile named.
///
/// Split out of [`apply_profiles`] because it is the one part that talks about *who the run is
/// talking to* rather than what it may do, and because that function was one field from a lint.
///
/// # Errors
///
/// Names a provider that does not exist, a wire this build does not speak, or a credential file
/// that is not there.
fn apply_provider(
    options: &mut RunOptions,
    provider_name: Option<&str>,
    overrides: &std::collections::BTreeMap<String, provider::ProviderOverride>,
) -> Result<(), String> {
    let Some(name) = provider_name else {
        return Ok(());
    };

    // Guarded by the caller: a typed `--base-url` means the bundle is not consulted at all.
    if options.base_url.is_some() {
        return Ok(());
    }
    let provider = provider::resolve(name, overrides)?;
    if options.base_url.is_none() {
        options
            .base_url
            .clone_from(&Some(provider.base_url.clone()));
    }
    // **The alias is expanded whether the model was typed or defaulted.** `--model haiku` is the
    // point of having aliases at all, and a name the table does not know passes through, so a
    // model released after this binary is still reachable.
    options.model = Some(options.model.as_deref().map_or_else(
        || provider.model.clone(),
        |wanted| provider.exact_model(wanted),
    ));
    if options.wire.is_none() {
        options.wire = Some(match provider.wire.as_str() {
            "anthropic-messages" => Wire::AnthropicMessages,
            "openai-responses" => Wire::OpenaiResponses,
            other => {
                return Err(format!(
                    "provider `{name}` names the wire `{other}`, which this build does not                          speak. It speaks `anthropic-messages` and `openai-responses`."
                ));
            }
        });
    }
    // **Only when the operator named no credential themselves.** A provider's is a default,
    // and a default that overrode something typed would be the ambient fallback this harness
    // refuses outright rather than the accountable one it now allows.
    let named_already = options.api_key_file.is_some()
        || options.api_key_env.is_some()
        || options.oauth_token_file.is_some()
        || options.oauth_token_env.is_some();
    if !named_already {
        match provider.credential {
            provider::Credential::OauthFile { path, pointer } => {
                let path = provider::expand_home(&path);
                if !std::path::Path::new(&path).is_file() {
                    return Err(format!(
                        "provider `{name}` reads its credential from `{path}`, which is not                              there. Log in to that vendor, or name another source with                              `--oauth-token-file` or `[providers.{name}]`."
                    ));
                }
                options.oauth_token_file = Some(std::path::PathBuf::from(path));
                if options.oauth_token_pointer.is_none() {
                    options.oauth_token_pointer = Some(pointer);
                }
            }
            provider::Credential::ApiKeyEnv { name } => options.api_key_env = Some(name),
        }
        // What the record will say. Not `named`: the operator chose the provider, and the
        // provider chose the path, and a reader is entitled to know which.
        options.credential_from_provider = Some(name.to_owned());
    }
    Ok(())
}

/// Fills whatever the operator did not type, from the profiles they named.
///
/// **Precedence is the shape of the data, not a rule applied on top.** A flag clap did not see is
/// `None`, an empty `Vec` or a `false` bool; this only ever fills those. So a typed flag wins
/// because there is nothing left to fill, and no `ValueSource` bookkeeping can drift from it.
///
/// Returns the profiles that contributed, for `session.started` — the record that makes a file
/// carrying a permission accountable.
///
/// # Errors
///
/// Names the missing endpoint or model, the profile that is not in the file, the provider that
/// does not exist, or a configuration that declares programs without `write`.
fn apply_profiles(options: &mut RunOptions) -> Result<Vec<profile::ProfileRef>, String> {
    let Some(path) = profile::config_path() else {
        return Ok(Vec::new());
    };
    let source = path.display().to_string();
    let config = profile::load(&path)?;
    let resolved = profile::resolve(&config, &options.profile, &source)?;
    let wanted = resolved.profile;

    // **A typed `--base-url` means the provider is not consulted at all.**
    //
    // A provider is a bundle whose parts belong together: an endpoint, the wire that endpoint
    // speaks, and the credential it accepts. Half-applying one over somebody else's endpoint
    // points anthropic's dialect at a server that has never heard of it — which is exactly what
    // happened the first time this ran, against a test's own fake server: `--base-url` was typed,
    // `--wire` was not, and the config supplied `anthropic-messages` for a 404.
    //
    // So naming the endpoint yourself opts out of the whole bundle, rather than a piece of it
    // arriving to keep you company. The profile's *permission* keys below still apply: those are
    // about what the run may do, not about who it is talking to.
    apply_provider(options, wanted.provider.as_deref(), &config.providers)?;

    if let Some(model) = wanted.model
        && options.model.is_none()
    {
        options.model = Some(model);
    }
    if wanted.write == Some(true) {
        options.substrate_embedded = true;
        if options.write_scope.is_empty() {
            options.write_scope = wanted
                .write_scope
                .unwrap_or_else(profile::default_write_scope);
        }
        if options.allow_program.is_empty() {
            options.allow_program = wanted.allow_program.unwrap_or_default();
        }
    }
    if options.approve_up_to.is_none()
        && let Some(ceiling) = wanted.approve_up_to
    {
        options.approve_up_to = Some(
            <ApproveUpTo as clap::ValueEnum>::from_str(&ceiling, false).map_err(|_| {
                format!(
                    "`{ceiling}` is not a risk this build knows. It knows the values \
                     `--approve-up-to` takes."
                )
            })?,
        );
    }
    if options.plugin_dir.is_empty() {
        options.plugin_dir = wanted
            .plugin_dir
            .unwrap_or_default()
            .into_iter()
            .map(std::path::PathBuf::from)
            .collect();
    }
    if options.max_turns.is_none() {
        options.max_turns = wanted.max_turns;
    }

    if options.base_url.is_none() || options.model.is_none() {
        return Err(format!(
            "no endpoint or model: type `--base-url` and `--model`, or name a provider in              `{source}` with `[default] provider = \"claude\"`.              `b10x-harness providers list` shows the ones this build ships, and              `b10x-harness profiles init` writes a starter config."
        ));
    }
    Ok(resolved.used)
}

fn resolve_credential(options: &RunOptions) -> Result<Credential, String> {
    if let Some(token) = subscription_token(
        options.oauth_token_file.as_ref(),
        options.oauth_token_env.as_ref(),
        options.oauth_token_pointer.as_ref(),
    ) {
        return Ok(Credential::Subscription(token));
    }
    Ok(
        match credential_from(options.api_key_file.as_ref(), options.api_key_env.as_ref())? {
            Some(value) => Credential::Key(value),
            None => Credential::Unnamed,
        },
    )
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

/// Where this run's credential comes from, resolved once from the flags.
///
/// Three states and not two. **Naming nothing is itself a declaration** — the run sends no
/// credential header at all, which is right for a gateway on this machine that authenticates
/// nobody, and for a run deliberately started with no credential whose first request is meant to be
/// refused by the far end.
enum Credential {
    /// The caller named no source.
    Unnamed,
    /// A key issued to a program, read once from the file or variable the caller named.
    Key(String),
    /// A token obtained on a person's behalf, re-read from its named source on every call.
    ///
    /// Held as the source and not as a value: nothing in this process ever holds the token, which
    /// is what makes an expired one recoverable without restarting the run.
    Subscription(harness_credential::SubscriptionToken),
    /// A source the caller already built, for a server that makes one client per turn and must
    /// hand each of them the same source rather than resolving the flags again.
    Shared(Arc<dyn harness_wire::BearerSource>),
}

impl std::fmt::Debug for Credential {
    /// Redacted, and deliberately. The `Key` variant **is** the secret, so a derived `Debug` would
    /// print it into every assertion failure and every panic message — which is the one thing
    /// `harness_wire::Bearer` has no `Display` in order to prevent, undone one layer up.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Unnamed => "Credential::Unnamed",
            Self::Key(_) => "Credential::Key(<redacted>)",
            Self::Subscription(_) => "Credential::Subscription(<redacted>)",
            Self::Shared(_) => "Credential::Shared(<redacted>)",
        })
    }
}

impl Credential {
    fn source(self) -> Option<Arc<dyn harness_wire::BearerSource>> {
        match self {
            Self::Unnamed => None,
            Self::Key(value) => Some(Arc::new(StaticBearer::new(value))),
            Self::Subscription(token) => Some(Arc::new(token)),
            Self::Shared(source) => Some(source),
        }
    }
}

fn subscription_token(
    file: Option<&PathBuf>,
    variable: Option<&String>,
    pointer: Option<&String>,
) -> Option<harness_credential::SubscriptionToken> {
    let source = match (file, variable) {
        (Some(path), _) => harness_credential::NamedSource::file(path),
        (None, Some(name)) => harness_credential::NamedSource::environment(name),
        (None, None) => return None,
    };
    let token = harness_credential::SubscriptionToken::new(source);
    Some(match pointer {
        Some(pointer) => token.at_pointer(pointer),
        None => token,
    })
}

/// A client for this endpoint on the wire the caller named, authenticated or deliberately not.
///
/// The wire is a branch **here and nowhere else**: below this line the loop holds a
/// [`ModelPort`] and cannot tell which projection it got, which is the whole reason a second wire
/// cost a second projection instead of a second loop.
fn model_client(
    wire: Wire,
    base_url: &str,
    model: &str,
    context_window: u64,
    max_output_tokens_per_turn: Option<u64>,
    credential: Credential,
    cancel: &LoopCancel,
) -> Result<Box<dyn ModelPort>, String> {
    let bearer = credential.source();
    match wire {
        Wire::OpenaiResponses => {
            let endpoint = harness_responses::Endpoint::new(base_url, model, context_window)
                .map_err(|error| error.to_string())?;
            match bearer {
                Some(source) => harness_responses::ResponsesClient::new(endpoint, source),
                None => harness_responses::ResponsesClient::unauthenticated(endpoint),
            }
            .map(|client| Box::new(client.with_cancel(cancel.clone())) as Box<dyn ModelPort>)
        }
        Wire::AnthropicMessages => {
            let mut endpoint = harness_messages::Endpoint::new(base_url, model, context_window)
                .map_err(|error| error.to_string())?;
            // This route requires an output bound, so a run that named one has named this too;
            // one that did not gets the endpoint's declared number rather than a silent absence.
            if let Some(limit) = max_output_tokens_per_turn {
                endpoint = endpoint
                    .with_max_output_tokens(limit)
                    .map_err(|error| error.to_string())?;
            }
            match bearer {
                Some(source) => harness_messages::MessagesClient::new(endpoint, source),
                None => harness_messages::MessagesClient::unauthenticated(endpoint),
            }
            .map(|client| Box::new(client.with_cancel(cancel.clone())) as Box<dyn ModelPort>)
        }
    }
    .map_err(|error| error.to_string())
}

fn app_server_command(options: &AppServerOptions) -> Result<(), String> {
    let bearer: Option<Arc<dyn harness_wire::BearerSource>> = match subscription_token(
        options.oauth_token_file.as_ref(),
        options.oauth_token_env.as_ref(),
        options.oauth_token_pointer.as_ref(),
    ) {
        Some(token) => Credential::Subscription(token).source(),
        None => {
            match credential_from(options.api_key_file.as_ref(), options.api_key_env.as_ref())? {
                Some(value) => Credential::Key(value).source(),
                None => None,
            }
        }
    };
    let config = ServerConfig {
        model: options.model.clone(),
        budget: Budget {
            max_turns: options.max_turns,
            max_output_tokens: options.max_output_tokens,
            ..Budget::default()
        },
        context_window: Some(options.context_window),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    // One client per turn: a turn that was interrupted leaves its token set, and reusing the
    // client would end the next turn before it started.
    let mut new_model = |cancel: harness_wire::Cancel| {
        let credential = match &bearer {
            None => Credential::Unnamed,
            Some(source) => Credential::Shared(Arc::clone(source)),
        };
        model_client(
            options.wire,
            &options.base_url,
            &options.model,
            options.context_window,
            options.max_output_tokens,
            credential,
            &cancel,
        )
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
    owned: Owned<'_>,
) -> Result<String, String> {
    if let Some(path) = &options.instructions_file {
        return fs::read_to_string(path)
            .map_err(|error| format!("reading `{}`: {error}", path.display()));
    }
    let mut text = standing_instruction(
        catalogue,
        options.surface,
        context,
        options.scope_announce == ScopeAnnounce::Stated,
        owned,
    );
    // Last, after the tools, the scope and the given files: what the model needs first is what it
    // can do, and where it is is what it needs in order to choose.
    let mut environment = environment::discover(&options.workspace, std::time::SystemTime::now());
    if options.no_project_instructions {
        environment.instructions = None;
    }
    text.push_str("\n\n");
    text.push_str(&environment.render());
    Ok(text)
}

/// A run that produced no answer, and which half of the harness it failed in.
///
/// The distinction is the whole point of this type. A caller above this process — metaharness's
/// `b10x` adapter, a shell script, a person — reads the exit status and the record; a run that
/// **never started** writes no record at all, and a driver that saw only `exited 1` had two hours
/// to guess why. So a refusal before the first request says so, in the record, under `--json`.
enum RunFailure {
    /// Nothing was sent. A credential, a workspace, a confinement or a session refused first.
    Refused(String),
    /// The loop started and could not finish. Its own events are already in the record.
    Failed(String),
}

impl RunFailure {
    fn message(&self) -> &str {
        match self {
            Self::Refused(message) | Self::Failed(message) => message,
        }
    }

    /// Whether this is the run that never started, which is the one that needs a stated terminal.
    fn never_started(&self) -> bool {
        matches!(self, Self::Refused(_))
    }
}

/// Everything a run needs, resolved before the first request.
///
/// One struct because every one of these can refuse, and a refusal here is a run that never
/// started — which is a different thing from a run that failed, and is reported as one.
struct Prepared {
    client: Box<dyn ModelPort>,
    tools: Published,
    approvals: Box<dyn ApprovalPort>,
    config: LoopConfig,
    cancel: LoopCancel,
    /// The conversation this run continues, and the one it will leave behind.
    session: transcript::Session,
    /// Where that session is written, or [`None`] under `--no-session`.
    session_dir: Option<PathBuf>,
    /// The operator's hooks, or [`None`] when nobody named a file. The loop consults none then.
    hooks: Option<hooks::Hooks>,
}

/// Resolves the endpoint, the credential, the toolset, the approver, the session and the hooks.
///
/// `answer` is the schema the run ends under, already read: `run` reads it from the file
/// `--output-schema` named, `workflow run` derives one, and `chat` passes [`None`] because a
/// conversation has no single end for a schema to shape. It arrives resolved rather than as a path
/// because the standing instruction written below **names the tool it publishes** — a caller that
/// held its own schema and passed [`None`] would get a run publishing `answer` and never told to
/// call it.
///
/// # Errors
///
/// Names whichever of them refused. Every one of these happens **before** the first request, so a
/// caller can state that the run never started rather than that it failed. A schema that is not an
/// object schema and a hooks file this build cannot read are refusals of exactly that kind: both
/// are read here, in this harness's own words, rather than at the far end of a paid turn.
fn prepare(
    options: &RunOptions,
    answer: Option<harness_loop::OutputSchema>,
) -> Result<Prepared, String> {
    let credential = resolve_credential(options)?;
    let cancel = LoopCancel::new();
    let client = model_client(
        options.wire(),
        options.base_url(),
        options.model(),
        options.context_window,
        options.max_output_tokens_per_turn,
        credential,
        &cancel,
    )?;
    let confined_toolchain = toolchain(options.toolchain.as_deref(), options.driver.as_deref())?;
    let confined_programs = programs(&options.allow_program, &confined_toolchain);
    let Publication { tools, withheld } = published(
        harness_tools::LocalOperations::new(&options.workspace)?,
        workspace_name(&options.workspace),
        options.surface,
        &Confinement {
            substrate: options.substrate.as_deref(),
            embedded: options.substrate_embedded,
            cgroup_root: options.cgroup_root.as_deref(),
            workspace_id: &options.workspace_id,
            programs: &confined_programs,
            toolchain: &confined_toolchain,
            scope: write_scope(&options.write_scope)?,
        },
    )?;
    let approvals = approver(options)?;
    let session_dir = session_dir(options)?;
    let session = open_session(options, session_dir.as_deref())?;
    // The schema arrives already read — from a file under `--output-schema`, or derived by the
    // caller, as `workflow run` derives one per section — because the instruction below names the
    // tool it publishes. A caller that resolved its own schema and left this `None` would get a run
    // under a schema that was never told to finish by calling `answer`.
    let delegation = delegation(options);
    let skills = skills_from(&options.skills_dir, &options.plugin_dir)?;
    let agents = agents_from(&options.agents_dir, &options.plugin_dir)?;
    let owned = Owned {
        delegate: delegation.as_ref().map(|tool| tool.name.as_str()),
        answer: answer.as_ref().map(|tool| tool.name.as_str()),
        skills: skills.as_ref(),
        agents: agents.as_ref(),
    };
    let config = LoopConfig::new(
        options.model(),
        instructions(
            options,
            tools.catalogue(),
            &context(&options.context)?,
            owned,
        )?,
    )
    .with_sampling(sampling(options))
    .with_budget(budget(options))
    .with_prices(prices(options)?)
    .with_unattended_ceiling(ceiling(options))
    // What makes compaction token-aware rather than a fixed 192 KiB of bytes.
    .with_context_window(Some(options.context_window))
    .with_output_schema(answer)
    .with_delegation(delegation)
    .with_skills(skills)
    .with_agents(agents)
    // Reported in the record and acted on nowhere: what the machine would not admit was already
    // decided when the catalogue was built, and this is the sentence saying so.
    .with_credential_source(credential_source(options))
    .with_profiles(applied_profiles(options))
    .with_withheld(withheld_events(&withheld));
    // A hook is unconfined — it is the operator's own program — but it is not handed this run's
    // credential. The child would otherwise inherit the whole environment, including whichever
    // variable `--api-key-env` or `--oauth-token-env` named, and a hook that echoed it into its
    // `{"note": …}` would put the key in the conversation and in the record.
    let credential_variables: Vec<&str> = [
        options.api_key_env.as_deref(),
        options.oauth_token_env.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect();
    let hooks = options
        .hooks
        .as_deref()
        .map(hooks::Hooks::load)
        .transpose()?
        .map(|hooks| {
            hooks
                .in_workspace(&options.workspace)
                .without_env(credential_variables)
        });
    Ok(Prepared {
        client,
        tools,
        approvals,
        config,
        cancel,
        session,
        session_dir,
        hooks,
    })
}

/// The shape this run's answer must take, read from the file `--output-schema` named.
///
/// # Errors
///
/// Names the file it could not read, and refuses a schema that is not a JSON Schema for an object
/// **in the loop's own words** — a tool's `input_schema` must be one on both wires, and a refusal
/// before the run beats a 400 after it.
fn schema(path: Option<&std::path::Path>) -> Result<Option<harness_loop::OutputSchema>, String> {
    let Some(path) = path else {
        return Ok(None);
    };
    let text = fs::read_to_string(path)
        .map_err(|error| format!("reading the output schema `{}`: {error}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("the output schema `{}`: {error}", path.display()))?;
    harness_loop::OutputSchema::new(value)
        .map(Some)
        .map_err(|error| format!("the output schema `{}`: {error}", path.display()))
}

/// Whether this run may delegate, and how long a delegate may work.
///
/// [`None`] publishes no `delegate` at all, which is what every run did before this flag existed.
/// The skills this run offers, from every directory the caller named.
///
/// `--plugin-dir <D>` is read as `--skills-dir <D>/skills`, in the order the flags were given so a
/// caller can predict which of two same-named skills wins — the first, because a later directory
/// silently replacing an earlier one is how a run ends up following instructions nobody chose.
///
/// # Errors
///
/// Names the directory or the document that could not be read. A named directory that is not there
/// refuses the run, exactly as `--context` does: a run given fewer skills than it was told to have
/// is one nobody can reproduce.
fn skills_from(
    skills_dir: &[PathBuf],
    plugin_dir: &[PathBuf],
) -> Result<Option<harness_loop::Skills>, String> {
    if skills_dir.is_empty() && plugin_dir.is_empty() {
        return Ok(None);
    }
    let mut skills: Vec<harness_loop::Skill> = Vec::new();
    let mut take = |loaded: Vec<harness_loop::Skill>| {
        for skill in loaded {
            // First wins. A later directory silently replacing an earlier one is how a run ends up
            // following instructions nobody chose.
            if !skills.iter().any(|held| held.name == skill.name) {
                skills.push(skill);
            }
        }
    };
    for directory in skills_dir {
        if !directory.is_dir() {
            return Err(format!(
                "the skills directory `{}` is not there",
                directory.display()
            ));
        }
        take(skills::skills_in(directory)?);
    }
    for directory in plugin_dir {
        if !directory.is_dir() {
            return Err(format!(
                "the plugin directory `{}` is not there",
                directory.display()
            ));
        }
        // Qualified `<plugin>:<skill>` from the plugin's own manifest, as the vendor does: two
        // plugins may both ship a `planning`, and a bare name is also not what an expectation
        // comparing the two arms names.
        take(skills::skills_in_plugin(directory)?);
    }
    Ok(Some(harness_loop::Skills::new(skills)))
}

/// The named agents this run may delegate as, from every directory the caller named.
///
/// Same shape and same first-wins rule as [`skills_from`], for the same reason: a later directory
/// silently replacing an earlier one is how a run ends up running an agent nobody chose.
///
/// # Errors
///
/// Names the directory or document that could not be read, including a vendor tool name this
/// build does not map — which refuses rather than dropping the tool, because a permission an
/// author granted and this build ignored is one the run would not have with nothing saying so.
fn agents_from(
    agents_dir: &[PathBuf],
    plugin_dir: &[PathBuf],
) -> Result<Option<harness_loop::Agents>, String> {
    if agents_dir.is_empty() && plugin_dir.is_empty() {
        return Ok(None);
    }
    let mut agents: Vec<harness_loop::Agent> = Vec::new();
    let mut take = |loaded: Vec<harness_loop::Agent>| {
        for agent in loaded {
            if !agents.iter().any(|held| held.name == agent.name) {
                agents.push(agent);
            }
        }
    };
    for directory in agents_dir {
        if !directory.is_dir() {
            return Err(format!(
                "the agents directory `{}` is not there",
                directory.display()
            ));
        }
        take(agents::agents_in(directory)?);
    }
    for directory in plugin_dir {
        if !directory.is_dir() {
            return Err(format!(
                "the plugin directory `{}` is not there",
                directory.display()
            ));
        }
        take(agents::agents_in_plugin(directory)?);
    }
    Ok(Some(harness_loop::Agents::new(agents)))
}

fn delegation(options: &RunOptions) -> Option<harness_loop::Delegation> {
    options
        .delegate
        .then(|| harness_loop::Delegation::default().with_max_turns(options.delegate_turns))
}

/// Who decides a call above this run's ceiling.
///
/// **The library's default is untouched.** `harness_loop`'s own default approver is `DenyAll` and
/// stays so (`AGENTS.md` invariant 12); what this picks is the command line's, for a person who is
/// usually sitting in front of it. `--yes` wins over `--approve` because it is the older spelling
/// of `all` and every unattended invocation already written says it.
///
/// # Errors
///
/// Only under `--approve prompt`: a person was asked for by name and there is no terminal to ask
/// on, so the run refuses rather than quietly denying every call it was meant to put to them.
fn approver(options: &RunOptions) -> Result<Box<dyn ApprovalPort>, String> {
    let mode = if options.yes {
        Approve::All
    } else {
        options.approve
    };
    match mode {
        Approve::All => Ok(Box::new(ApproveAll)),
        Approve::Deny => Ok(Box::new(DenyAll)),
        Approve::Prompt => match approve::Terminal::open() {
            Ok(terminal) => Ok(Box::new(terminal)),
            Err(reason) => Err(format!(
                "`--approve prompt` asks a person about each call and there is no terminal to ask \
                 on: {reason}. Run it from a terminal, or choose `--approve deny` or `--yes`."
            )),
        },
        // The one branch that decides for itself, and it says so out loud. A run that silently
        // fell back to refusing everything would look like a harness whose tools do not work.
        Approve::Auto => {
            if approve::stdio_is_interactive()
                && let Ok(terminal) = approve::Terminal::open()
            {
                return Ok(Box::new(terminal));
            }
            eprintln!(
                "no terminal to ask; calls above the ceiling will be refused — pass --yes or \
                 --approve-up-to"
            );
            Ok(Box::new(DenyAll))
        }
    }
}

/// Where this run's session is written, or [`None`] when it must leave nothing behind.
///
/// # Errors
///
/// Names the reason there is no default directory, when the caller named none either.
fn session_dir(options: &RunOptions) -> Result<Option<PathBuf>, String> {
    if options.no_session {
        return Ok(None);
    }
    match &options.session_dir {
        Some(path) => Ok(Some(path.clone())),
        None => transcript::default_dir().map(Some),
    }
}

/// The session this run continues, or a new one.
///
/// # Errors
///
/// Names a session that cannot be read, one that does not exist, and — **before the first
/// request** — one recorded on another wire. The loop would refuse the opaque items anyway
/// (`AGENTS.md` invariant 5); saying it here says it in this harness's own words, with the flag
/// that fixes it, instead of as a typed refusal from inside a turn nobody has paid for yet.
fn open_session(
    options: &RunOptions,
    dir: Option<&std::path::Path>,
) -> Result<transcript::Session, String> {
    let workspace = options
        .workspace
        .canonicalize()
        .unwrap_or_else(|_| options.workspace.clone());
    let Some(which) = options.resume.as_deref() else {
        return Ok(transcript::Session::new(
            options.wire().id(),
            options.model(),
            options.base_url(),
            workspace,
        ));
    };
    // `--resume` and `--no-session` conflict at the parser, so this is only reachable from a
    // caller that built the options itself.
    let Some(dir) = dir else {
        return Err(
            "`--resume` needs a session directory, and `--no-session` removed it".to_owned(),
        );
    };
    let session = if which == "latest" {
        transcript::Session::latest(dir)?.ok_or_else(|| {
            format!(
                "there is no session in `{}` to resume; `b10x-harness sessions` lists what there is",
                dir.display()
            )
        })?
    } else {
        transcript::Session::load(dir, which)?
    };
    if session.wire.as_str() != options.wire().id().as_str() {
        return Err(format!(
            "session `{}` was recorded on the `{}` wire and this run speaks `{}`; a provider's \
             own reasoning items are replayed verbatim and may not cross wires, so resume it with \
             `--wire {}` or start a new session",
            session.id,
            session.wire.as_str(),
            options.wire().id().as_str(),
            session.wire.as_str()
        ));
    }
    // A warning and not a refusal: reading a different tree is a legitimate thing to do — the same
    // conversation about a second checkout — and only the person running it knows whether the
    // files it already read are the ones it will find.
    if session.workspace != workspace {
        eprintln!(
            "warning [session-workspace] session `{}` was recorded over `{}` and this run reads \
             `{}`; what it already read may not be there",
            session.id,
            session.workspace.display(),
            workspace.display()
        );
    }
    Ok(session)
}

/// Writes the session, and answers where it went.
///
/// # Errors
///
/// Names the path it could not write. The caller reports it as a warning: the run itself already
/// happened, and calling it failed because its transcript could not be filed would be a second
/// wrong answer.
fn persist(
    session: &transcript::Session,
    dir: Option<&std::path::Path>,
) -> Result<Option<PathBuf>, String> {
    match dir {
        None => Ok(None),
        Some(dir) => session.save(dir).map(Some),
    }
}

/// Saves the session and says so, however the run ended.
///
/// Both halves matter. A run that failed on turn twenty is exactly the one whose nineteen turns
/// must survive, and a person who cannot see the identifier cannot resume it.
fn close_session(prepared: &mut Prepared, options: &RunOptions, out: &mut dyn io::Write) {
    prepared.session.updated_unix = unix_now();
    match persist(&prepared.session, prepared.session_dir.as_deref()) {
        Err(error) => eprintln!("warning [session] {error}"),
        Ok(None) => {}
        Ok(Some(path)) => {
            if options.json {
                // The last line of the record, so a driver reading the stream ends up holding the
                // identifier it needs to continue the conversation.
                let _ = writeln!(
                    out,
                    "{}",
                    serde_json::json!({
                        "kind": "session",
                        "id": prepared.session.id,
                        "path": path.display().to_string(),
                    })
                );
            } else if !options.quiet {
                eprintln!(
                    "session {} saved to {}",
                    prepared.session.id,
                    path.display()
                );
            }
        }
    }
}

/// Seconds since the epoch, or zero on a clock set before it.
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn run_command(command: &RunCommand) -> Result<LoopStop, RunFailure> {
    let options = &command.options;
    let answer = schema(command.output_schema.as_deref()).map_err(RunFailure::Refused)?;
    let mut prepared = prepare(options, answer).map_err(RunFailure::Refused)?;
    install_interrupt(&prepared.cancel);

    // Under `--output-schema` stdout is the answer and nothing else, so the prose goes to stderr
    // with the rest of the progress. `--json` is unchanged either way.
    let mut renderer = Renderer::new(io::stdout(), io::stderr(), options.json, options.quiet)
        .structured(command.output_schema.is_some());
    let mut items = std::mem::take(&mut prepared.session.items);
    // Held here rather than borrowed out of `prepared`, so the loop below can borrow the client,
    // the tools and the approver from it at the same time.
    let mut hooks = prepared.hooks.take();
    let mut agent = AgentLoop::new(
        prepared.client.as_mut(),
        &mut prepared.tools,
        prepared.approvals.as_mut(),
        prepared.config.clone(),
    )
    .with_cancel(prepared.cancel.clone());
    if let Some(hooks) = hooks.as_mut() {
        agent = agent.with_hooks(hooks);
    }
    let mut spend = RunLedger::default();
    let outcome = agent.run_in(&mut items, &mut spend, command.input.clone(), &mut renderer);

    let answer = match outcome {
        Ok(outcome) => {
            prepared.session.extend(&outcome);
            Ok(outcome.stop)
        }
        // The twenty-turn run a blip threw away: the conversation the loop handed back is the
        // conversation, and it is saved exactly as a finished one is — **with what it cost**. The
        // usage and the cost of the turns that did happen scrolled past on stderr while the run was
        // alive; after it, the session file is the only place left holding them, and one that
        // showed nineteen turns and no tokens would say the failure was free.
        Err(error) => {
            prepared.session.items = items;
            prepared.session.spent(&spend);
            Err(RunFailure::Failed(error.to_string()))
        }
    };
    close_session(&mut prepared, options, &mut io::stdout());
    answer
}

/// `b10x-harness chat` — one conversation, a line at a time.
///
/// The smallest thing that removes *one question, one answer, exit*: every line of standard input
/// is one more turn on the same conversation, the session is written after each of them, and
/// `exit` or the end of the input stops. There is no line editing, no history and no completion —
/// a shell has all three, and a harness that grew them would own a terminal library forever.
///
/// A session is **named, never picked up**: `chat` starts a new one unless `--resume` says which,
/// exactly as `run` does. Continuing whatever ran last by default would make two runs with the
/// same flags mean different things.
fn chat_command(options: &RunOptions) -> Result<LoopStop, RunFailure> {
    use std::io::{BufRead as _, Write as _};

    // `chat` takes no `--output-schema`: a conversation has no single end for one to shape.
    let mut prepared = prepare(options, None).map_err(RunFailure::Refused)?;
    install_interrupt(&prepared.cancel);

    let mut renderer = Renderer::new(io::stdout(), io::stderr(), options.json, options.quiet);
    let mut items = std::mem::take(&mut prepared.session.items);
    let mut hooks = prepared.hooks.take();
    let mut stop = LoopStop::Completed;
    let stdin = io::stdin();
    let mut line = String::new();
    loop {
        if !options.quiet && !options.json {
            let _ = write!(io::stderr(), "> ");
            let _ = io::stderr().flush();
        }
        line.clear();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) => {
                close_session(&mut prepared, options, &mut io::stdout());
                return Err(RunFailure::Failed(format!(
                    "reading the next question: {error}"
                )));
            }
        }
        let question = line.trim();
        if question.is_empty() {
            continue;
        }
        if question == "exit" {
            break;
        }
        let mut agent = AgentLoop::new(
            prepared.client.as_mut(),
            &mut prepared.tools,
            prepared.approvals.as_mut(),
            prepared.config.clone(),
        )
        .with_cancel(prepared.cancel.clone());
        if let Some(hooks) = hooks.as_mut() {
            agent = agent.with_hooks(hooks);
        }
        let mut spend = RunLedger::default();
        let outcome = agent.run_in(&mut items, &mut spend, question, &mut renderer);
        match outcome {
            Ok(outcome) => {
                stop = outcome.stop.clone();
                prepared.session.extend(&outcome);
                prepared.session.updated_unix = unix_now();
                if let Err(error) = persist(&prepared.session, prepared.session_dir.as_deref()) {
                    eprintln!("warning [session] {error}");
                }
                // A turn that stopped for a named reason — a budget, a cancel — is the end of the
                // conversation, not a prompt for the next line: the next one would hit the same
                // bound and cost another request to find out.
                if !stop.is_completed() {
                    break;
                }
            }
            // The line that broke still bought whatever turns it got through, exactly as `run`'s
            // failed run does — and the session it is written into already holds the lines before
            // it, so a total that stopped counting here would be wrong about the whole
            // conversation and not only about this line.
            Err(error) => {
                prepared.session.items = items;
                prepared.session.spent(&spend);
                close_session(&mut prepared, options, &mut io::stdout());
                return Err(RunFailure::Failed(error.to_string()));
            }
        }
    }
    close_session(&mut prepared, options, &mut io::stdout());
    Ok(stop)
}

/// `b10x-harness sessions` — what is on this machine to resume.
///
/// # Errors
///
/// Names the directory it could not read, and any file in it that is not a session this build
/// understands. A corrupt file is not skipped: a listing that quietly left one out is a listing a
/// person would resume the wrong session from.
fn sessions_command(options: &SessionsOptions) -> Result<(), String> {
    let dir = match &options.session_dir {
        Some(path) => path.clone(),
        None => transcript::default_dir()?,
    };
    let rows = transcript::Session::list(&dir)?;
    if rows.is_empty() {
        println!("no sessions in {}", dir.display());
        return Ok(());
    }
    for row in rows {
        println!(
            "{}  {}  {}  {} turn(s)",
            row.id,
            utc_stamp(row.updated_unix),
            row.model,
            row.turns
        );
    }
    Ok(())
}

/// A session's `updated` time as `YYYY-MM-DD hh:mm:ssZ`.
///
/// The time of day and not only the date, because the sessions worth resuming are usually today's
/// and a column of identical dates distinguishes none of them. UTC, and it says so: the date
/// arithmetic is `environment::utc_date`'s, and inventing a local zone here would be a second
/// answer to what day it is.
fn utc_stamp(unix: u64) -> String {
    let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(unix);
    let seconds = unix % 86_400;
    format!(
        "{} {:02}:{:02}:{:02}Z",
        environment::utc_date(when),
        seconds / 3_600,
        (seconds % 3_600) / 60,
        seconds % 60
    )
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
fn toolchain(
    name: Option<&str>,
    driver: Option<&std::path::Path>,
) -> Result<harness_substrate::Toolchain, String> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let toolchain = match name {
        None => harness_substrate::Toolchain::default(),
        Some("rust") => harness_substrate::Toolchain::rust(home.as_deref())?,
        Some(other) => {
            return Err(format!(
                "`{other}` is not a toolchain this build knows; there is `rust`"
            ));
        }
    };
    // Composed rather than alternative: a run can want a compiler and the program that drives it,
    // and substrate admits four roots for exactly this kind of assembly.
    match driver {
        None => Ok(toolchain),
        Some(program) => toolchain.with_driver(program, &std::env::temp_dir()),
    }
}

/// The allow-list the caller declared, plus the staged driver where there is one.
///
/// Declaring a program worth mounting and then having to allow-list it again by its sandbox path
/// is two declarations of one decision, and the second is the one a caller forgets — which
/// produces a run that can see the program and not start it.
fn programs(declared: &[String], toolchain: &harness_substrate::Toolchain) -> Vec<String> {
    let mut programs = declared.to_vec();
    if let Some(driver) = toolchain.driver()
        && !programs.iter().any(|program| program == driver.program())
    {
        programs.push(driver.program().to_owned());
    }
    programs
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

/// One catalogue, published under whichever surface the run asked for.
///
/// An enum rather than a `Box<dyn ToolPort>` because the command needs two things from it: the
/// port, for the loop, and the **catalogue**, for the instruction and for `tools` — and the
/// catalogue is not on the trait, because no loop has any business asking what stands behind the
/// tools it was given.
enum Published {
    Flat(harness_tools::Flat),
    Verbs(harness_tools::Verbs),
}

impl Published {
    /// This run's catalogue, which is the same document whichever surface publishes it.
    fn catalogue(&self) -> &harness_tools::Catalogue {
        match self {
            Self::Flat(flat) => flat.catalogue(),
            Self::Verbs(verbs) => verbs.catalogue(),
        }
    }

    fn as_port(&self) -> &dyn harness_wire::ToolPort {
        match self {
            Self::Flat(flat) => flat,
            Self::Verbs(verbs) => verbs,
        }
    }

    fn as_port_mut(&mut self) -> &mut dyn harness_wire::ToolPort {
        match self {
            Self::Flat(flat) => flat,
            Self::Verbs(verbs) => verbs,
        }
    }
}

/// Every method delegated, none defaulted.
///
/// Taking the trait's defaults here would silently drop the surface's own answers — `invoked`,
/// which is what the approval gate reads, and `call_batch`, which is what makes six reads cost one
/// round trip. Both are overridden by at least one of the two surfaces.
impl harness_wire::ToolPort for Published {
    fn specs(&self) -> &[harness_wire::ToolSpec] {
        self.as_port().specs()
    }

    fn subjects(&self, call: &harness_wire::ToolCall) -> Vec<harness_wire::Subject> {
        self.as_port().subjects(call)
    }

    fn invoked(&self, call: &harness_wire::ToolCall) -> Option<harness_wire::ToolSpec> {
        self.as_port().invoked(call)
    }

    fn operations(&self) -> Vec<&'static str> {
        self.as_port().operations()
    }

    fn call(&mut self, call: &harness_wire::ToolCall) -> harness_wire::ToolOutcome {
        self.as_port_mut().call(call)
    }

    fn call_within(
        &mut self,
        call: &harness_wire::ToolCall,
        remaining: Option<std::time::Duration>,
    ) -> harness_wire::ToolOutcome {
        self.as_port_mut().call_within(call, remaining)
    }

    fn call_batch(
        &mut self,
        calls: &[harness_wire::ToolCall],
        remaining: Option<std::time::Duration>,
    ) -> Vec<harness_wire::ToolOutcome> {
        self.as_port_mut().call_batch(calls, remaining)
    }
}

/// What this machine publishes, and what it would not.
///
/// Two answers to one question, and the second is the one that used to be missing. The publication
/// gate takes a tool away **silently** — which is right for the model, and is why a run whose only
/// legal route was starting a program got six entries instead of seven and nobody could tell that
/// from a run that never wanted a seventh. So the toolset travels with the record of what was
/// declared and refused, and every place that states the run's shape states both.
struct Publication {
    tools: Published,
    /// Declared and not admitted, with the predicate that decided. Empty is the ordinary answer.
    withheld: Vec<harness_substrate::Withheld>,
}

/// The record substrate computed, in the loop's own vocabulary.
///
/// Two types and one mapping rather than one shared type, because `harness-loop` depends on
/// `harness-wire` alone and neither of them may read a machine (`AGENTS.md` invariant 3). The fact
/// is substrate's; the record is the loop's; this shell is the one place holding both.
fn withheld_events(withheld: &[harness_substrate::Withheld]) -> Vec<harness_loop::Withheld> {
    withheld
        .iter()
        .map(|withheld| harness_loop::Withheld {
            tool: withheld.tool.clone(),
            reason: withheld.reason.clone(),
        })
        .collect()
}

/// The tools this machine admits, which is the machine's answer and not a flag's.
///
/// **The publication gate, in one function**, and it is unchanged by either surface: what the
/// catalogue holds is what the machine can perform. Four entries with no backend; six with a
/// confined workspace; seven inside a delegated cgroup. A tool the machine cannot confine is one
/// no surface ever lists. The `surface` argument decides only *how* those entries are offered —
/// one tool each, or three verbs over them.
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
    surface: Surface,
    confinement: &Confinement<'_>,
) -> Result<Publication, String> {
    let Confinement {
        substrate,
        embedded,
        cgroup_root,
        workspace_id,
        programs,
        toolchain,
        scope,
    } = confinement;
    let publish = |catalogue: harness_tools::Catalogue| match surface {
        Surface::Flat => Published::Flat(harness_tools::Flat::new(catalogue)),
        Surface::Verbs => Published::Verbs(harness_tools::Verbs::new(catalogue)),
    };
    let read_only = || publish(harness_tools::Catalogue::of(reading.clone()).scoped(scope.clone()));

    if *embedded {
        let workspace = adopted(&reading, workspace_name, *cgroup_root, toolchain)?;
        let confined = workspace.confined(programs.to_vec());
        let withheld = confined.withheld().to_vec();
        return Ok(Publication {
            tools: publish(
                harness_tools::Catalogue::of(harness_tools::Split::new(reading, confined))
                    .scoped(scope.clone()),
            ),
            withheld,
        });
    }

    let Some(socket) = *substrate else {
        // No confinement was asked for, so no workspace was expected and its absence is not
        // reported. A **program set** declared with nowhere to run it is a different thing: the
        // operator named commands and this run has no tool that could start one, which is exactly
        // the silence this record exists to break.
        return Ok(Publication {
            tools: read_only(),
            withheld: harness_substrate::Facts::none().withheld(programs, false),
        });
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
    let withheld = confined.withheld().to_vec();
    Ok(Publication {
        tools: publish(
            harness_tools::Catalogue::of(harness_tools::Split::new(reading, confined))
                .scoped(scope.clone()),
        ),
        withheld,
    })
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
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.starts_with('-')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(format!(
            "`--substrate-embedded` cannot adopt `{name}`: a workspace directory's name must be \
             one path component of alphanumerics, `_` and `-`, and may not begin with `-`. That \
             is what keeps it a single component the guarded filesystem can open beneath its \
             root. Point at a directory whose name qualifies, or drop the flag for a read-only run."
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

/// Reads the config, or says where it should be.
fn read_config() -> Result<(profile::Config, String), String> {
    let path = profile::config_path().ok_or_else(|| {
        "no `XDG_CONFIG_HOME` and no `HOME`, so there is nowhere a config could be".to_owned()
    })?;
    let config = profile::load(&path)?;
    Ok((config, path.display().to_string()))
}

/// `profiles list|show|explain|init`.
fn profiles_command(verb: &ProfilesCommand) -> Result<(), String> {
    if let ProfilesCommand::Init = verb {
        let path = profile::config_path().ok_or_else(|| "nowhere to write a config".to_owned())?;
        if path.exists() {
            // Never overwritten: a config is the operator's, and one silently replaced is a set of
            // rules they thought they had.
            return Err(format!(
                "`{}` is already there. Read it, or move it aside first.",
                path.display()
            ));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("creating `{}`: {error}", parent.display()))?;
        }
        std::fs::write(&path, profile::starter())
            .map_err(|error| format!("writing `{}`: {error}", path.display()))?;
        println!("{}", path.display());
        return Ok(());
    }
    let (config, source) = read_config()?;
    match verb {
        ProfilesCommand::Init => unreachable!("handled above"),
        ProfilesCommand::List => {
            println!("default    {source}");
            for profile in &config.profiles {
                if let Some(name) = &profile.name {
                    println!("{name:<10} {source}");
                }
            }
        }
        ProfilesCommand::Show { name } => {
            let resolved = profile::resolve(&config, std::slice::from_ref(name), &source)?;
            println!("{:#?}", resolved.profile);
        }
        ProfilesCommand::Explain { profile: wanted } => {
            let resolved = profile::resolve(&config, wanted, &source)?;
            // The point of the verb: what this will actually run as, before a token is spent.
            for used in &resolved.used {
                println!(
                    "profile {} <- {} ({})",
                    used.name,
                    used.source,
                    &used.sha256[..12]
                );
            }
            if let Some(name) = &resolved.profile.provider {
                let provider = provider::resolve(name, &config.providers)?;
                println!("--base-url {}", provider.base_url);
                println!("--wire {}", provider.wire);
                println!(
                    "--model {}",
                    resolved.profile.model.as_ref().unwrap_or(&provider.model)
                );
                match &provider.credential {
                    provider::Credential::OauthFile { path, pointer } => {
                        println!("--oauth-token-file {}", provider::expand_home(path));
                        println!("--oauth-token-pointer {pointer}");
                    }
                    provider::Credential::ApiKeyEnv { name } => println!("--api-key-env {name}"),
                }
            }
            if resolved.profile.write == Some(true) {
                println!("--substrate-embedded");
                for rule in resolved
                    .profile
                    .write_scope
                    .clone()
                    .unwrap_or_else(profile::default_write_scope)
                {
                    println!("--write-scope {rule}");
                }
            } else {
                println!("(no --substrate-embedded: `write` is not true, so four read-only tools)");
            }
            if let Some(ceiling) = &resolved.profile.approve_up_to {
                println!("--approve-up-to {ceiling}");
            }
            for program in resolved.profile.allow_program.iter().flatten() {
                println!("--allow-program {program}");
            }
        }
    }
    Ok(())
}

/// `providers list|show`.
fn providers_command(verb: &ProvidersCommand) -> Result<(), String> {
    let (config, source) = read_config()?;
    match verb {
        ProvidersCommand::List => {
            for built in provider::built_in() {
                let note = if config.providers.contains_key(&built.name) {
                    format!("built-in, overridden in {source}")
                } else {
                    "built-in".to_owned()
                };
                println!("{:<8} {note}", built.name);
            }
        }
        ProvidersCommand::Show { name } => {
            let provider = provider::resolve(name, &config.providers)?;
            println!("base-url  {}", provider.base_url);
            println!("wire      {}", provider.wire);
            println!("model     {}", provider.model);
            match &provider.credential {
                provider::Credential::OauthFile { path, pointer } => {
                    // Printed before a token is spent: a defaulted credential path is only
                    // acceptable because it is readable without running anything.
                    println!("oauth-token-file     {}", provider::expand_home(path));
                    println!("oauth-token-pointer  {pointer}");
                }
                provider::Credential::ApiKeyEnv { name } => println!("api-key-env  {name}"),
            }
        }
    }
    Ok(())
}

fn tools_command(options: &ToolsOptions) -> Result<(), String> {
    let tools_skills = skills_from(&options.skills_dir, &options.plugin_dir)?;
    let tools_agents = agents_from(&options.agents_dir, &options.plugin_dir)?;
    let confined_toolchain = toolchain(options.toolchain.as_deref(), options.driver.as_deref())?;
    let confined_programs = programs(&options.allow_program, &confined_toolchain);
    let Publication { tools, withheld } = published(
        harness_tools::LocalOperations::new(&options.workspace)?,
        workspace_name(&options.workspace),
        options.surface,
        &Confinement {
            substrate: options.substrate.as_deref(),
            embedded: options.substrate_embedded,
            cgroup_root: options.cgroup_root.as_deref(),
            workspace_id: &options.workspace_id,
            programs: &confined_programs,
            toolchain: &confined_toolchain,
            scope: write_scope(&options.write_scope)?,
        },
    )?;
    // On stderr, where it cannot be mistaken for part of the document and cannot be missed by a
    // person reading a screen of JSON — the same line the run's own renderer prints, so the
    // command a person checks a machine with and the run they then start say the same thing.
    for withheld in &withheld {
        eprintln!(
            "{}",
            render::withheld_line(&withheld.tool, &withheld.reason)
        );
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "workspace": options.workspace.display().to_string(),
            "surface": match options.surface {
                Surface::Flat => "flat",
                Surface::Verbs => "verbs",
            },
            "tools": tools_specs(&tools),
            // What the surface stands in front of, so `b10x-harness tools` answers the question a
            // reader is actually asking — what can this run do? — under either of them. Under
            // `flat` the two lists name the same entries; under `verbs` they do not, and that
            // difference is exactly what a reader has to be able to see.
            "catalogue": tools.catalogue().search(None, None),
            // And what it **cannot** do that it was asked to. Always present, empty included:
            // this command exists to state a machine's shape, and a reader who found no key could
            // not tell *nothing was withheld* from *this build does not answer that*. It is the
            // difference between a six-entry catalogue nobody wanted a seventh entry in and one
            // that was refused it.
            "withheld": withheld,
            // And what the run is given beyond its tools. Always a list, empty included, for the
            // reason `withheld` is: a reader cannot otherwise tell *none were declared* from
            // *this build does not say*.
            "skills": tools_skills
                .as_ref()
                .map_or_else(Vec::new, harness_loop::Skills::names),
            "agents": tools_agents
                .as_ref()
                .map_or_else(Vec::new, harness_loop::Agents::names),
            // The staged driver, by its sandbox path and the digest of the bytes that were staged.
            // substrate mounts a declared root read-only and reports it, and computes no digest
            // over one — so "this run pinned the build its evidence is recorded against" is a
            // claim only somebody writing this value down can make. `null` when none was declared.
            "driver": confined_toolchain.driver().map(|driver| serde_json::json!({
                "program": driver.program(),
                "sha256": driver.sha256(),
            })),
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
        Command::Run(command) => {
            let mut command = command.clone();
            // Resolved once, here, before anything reads an option: `run_command` and everything
            // under it sees a fully-filled `RunOptions` and never has to know a profile existed.
            match apply_profiles(&mut command.options) {
                Err(refusal) => reported(Err(refusal)),
                Ok(profiles) => {
                    command.options.applied_profiles = profiles;
                    stopped(run_command(&command), command.options.json)
                }
            }
        }
        Command::Chat(options) => {
            let mut options = options.clone();
            match apply_profiles(&mut options) {
                Err(refusal) => reported(Err(refusal)),
                Ok(profiles) => {
                    options.applied_profiles = profiles;
                    stopped(chat_command(&options), options.json)
                }
            }
        }
        Command::Sessions(options) => reported(sessions_command(options)),
        Command::Tools(options) => reported(tools_command(options)),
        Command::Profiles(verb) => reported(profiles_command(verb)),
        Command::Providers(verb) => reported(providers_command(verb)),
        Command::AppServer(options) => reported(app_server_command(options)),
        Command::Events(options) => reported(events_command(options)),
        Command::Workflow(command) => workflow::dispatch(command),
    }
}

/// The exit status of a command that runs the loop, and the record it leaves when it could not.
///
/// Under `--json` a run that **never started** writes one line saying so, because otherwise it
/// writes nothing at all and a driver above it sees an exit status and no record. `1` and not
/// clap's `2`: on this command line `2` already means *the run stopped for a named reason*, which
/// is a run that happened.
fn stopped(outcome: Result<LoopStop, RunFailure>, json: bool) -> ExitCode {
    match outcome {
        Ok(stop) if stop.is_completed() => ExitCode::SUCCESS,
        Ok(stop) => {
            eprintln!("the run stopped without an answer: {stop:?}");
            ExitCode::from(2)
        }
        Err(failure) => {
            if json && failure.never_started() {
                println!("{}", refused_line(failure.message()));
            }
            eprintln!("error: {}", failure.message());
            ExitCode::FAILURE
        }
    }
}

/// The exit status of a command that answers once and has no loop to report.
fn reported(outcome: Result<(), String>) -> ExitCode {
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

/// The one-line record of a run that never started.
///
/// The same shape every other loop event has, so a reader parsing the `--json` stream line by line
/// does not need a second parser for it. `b10x-harness events` maps it onto the terminal record
/// the stream already has.
fn refused_line(reason: &str) -> String {
    serde_json::json!({"kind": "refused", "reason": one_line(reason)}).to_string()
}

/// A message flattened onto one line, so a line-delimited record stays line-delimited.
///
/// clap's refusals are several lines — the error, the usage, a suggestion — and every one of them
/// is worth keeping, so they are joined rather than cut (`AGENTS.md` invariant 8).
fn one_line(message: &str) -> String {
    message
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
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

/// Parses the command line and runs it.
///
/// # Why the parse failure is caught rather than left to clap
///
/// A peer's driver launched this binary with a flag that had changed shape, clap wrote its usage
/// to stderr and exited **2** before any harness code ran, and the driver — which reads the
/// `--json` record and the exit status — saw a status it already had a meaning for and no record
/// at all. Two hours went into working out that the run had never started.
///
/// So a refused command line writes the same terminal line every other unstartable run writes, and
/// exits **1**: on this command line `2` means *the run stopped for a named reason*, which is a run
/// that happened. clap's own message still goes to stderr, unchanged, because it is the one that
/// tells a person what to type.
pub fn run() -> ExitCode {
    match Cli::try_parse() {
        Ok(cli) => dispatch(&cli),
        // `--help` and `--version` come back as errors too, and they are answers: clap writes them
        // to stdout and this must not turn them into a failure.
        Err(error) if !error.use_stderr() => {
            let _ = error.print();
            ExitCode::SUCCESS
        }
        Err(error) => {
            // The raw argv, because there is no parse to ask: a command line clap refused has no
            // `--json` flag in any structured sense, only in the bytes the caller typed.
            if std::env::args_os().any(|argument| argument == *"--json") {
                println!("{}", refused_line(&error.render().to_string()));
            }
            let _ = error.print();
            ExitCode::FAILURE
        }
    }
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
        options.options
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
        let Credential::Key(value) = resolve_credential(&options).expect("readable") else {
            panic!("an api key file resolves to a key");
        };
        assert_eq!(value, "sk-test");
    }

    #[test]
    fn a_credential_never_reaches_a_panic_message() {
        // `Bearer` has no `Display` so a secret cannot reach a log line by accident. That is
        // undone one layer up the moment this enum derives `Debug`, and every assertion in this
        // module prints it on failure.
        assert_eq!(
            format!("{:?}", Credential::Key("sk-do-not-print-me".to_owned())),
            "Credential::Key(<redacted>)"
        );
    }

    #[test]
    fn a_named_oauth_source_is_held_as_a_source_rather_than_read_at_startup() {
        // The token is re-read on every call, so the flags resolve to something that knows *where*
        // rather than to a value. A source that read at startup would serve an expired token for
        // the rest of the run.
        let options = options(&[
            "--oauth-token-file",
            "/named/by/the/caller",
            "--oauth-token-pointer",
            "/store/accessToken",
        ]);
        let credential =
            resolve_credential(&options).expect("naming a source is not the same as reading it");
        assert!(matches!(credential, Credential::Subscription(_)));
        // And a file that is not there is not an error *yet*: it becomes one at the call, where
        // the wire can report it as a typed refusal the loop understands.
    }

    #[test]
    fn the_credential_kinds_are_mutually_exclusive_on_the_command_line() {
        for pair in [
            vec!["--api-key-env", "KEY", "--oauth-token-file", "/t"],
            vec!["--api-key-file", "/k", "--oauth-token-env", "TOKEN"],
            vec!["--oauth-token-file", "/t", "--oauth-token-env", "TOKEN"],
        ] {
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
            assert!(
                parse(&[base, pair.clone()].concat()).is_err(),
                "{pair:?} must not parse: a run has one credential, not two"
            );
        }
    }

    #[test]
    fn a_pointer_without_a_source_to_point_into_is_refused() {
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
                "--oauth-token-pointer",
                "/store/accessToken",
            ])
            .is_err()
        );
    }

    #[test]
    fn the_wire_defaults_to_the_one_this_harness_shipped_with() {
        // Every invocation written before the second wire existed must still mean what it did.
        //
        // The flag is `Option<Wire>` now, and `None` is what makes a profile able to set it: a
        // clap `default_value` would have beaten the file, so the default moved to
        // `RunOptions::wire()` where it is applied last.
        assert_eq!(
            options(&[]).wire,
            None,
            "untyped, so a provider may still speak"
        );
        assert_eq!(options(&[]).wire(), Wire::OpenaiResponses);
        assert_eq!(
            options(&["--wire", "anthropic-messages"]).wire(),
            Wire::AnthropicMessages
        );
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
                "--wire",
                "not-a-wire",
            ])
            .is_err(),
            "an unknown wire is refused by name rather than falling back to a default"
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
        assert!(matches!(
            resolve_credential(&options(&[])).expect("declared"),
            Credential::Unnamed
        ));
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
    fn the_default_surface_names_every_tool_and_leaves_the_schemas_where_the_provider_reads_them() {
        // Flat is the default because of what the verbs measured: 33-44% of every tool call across
        // three live runs was `tool_search` or `tool_describe`. Under this surface there is nothing
        // to discover — the names are in the instruction and the shapes are in `tools`, which the
        // provider validates against and a prompt cache holds.
        let dir = tempfile::tempdir().expect("temporary directory");
        let catalogue = a_catalogue(dir.path());
        let text = instructions(&options(&[]), &catalogue, "", Owned::default())
            .expect("the default is available");

        for entry in ["file_read", "dir_list", "search", "find"] {
            assert!(text.contains(entry), "`{entry}` is named up front: {text}");
        }
        assert!(
            !text.contains("max_bytes"),
            "the schemas are in `tools` on every request; a second copy here would be paid for \
             twice and validated never: {text}"
        );
        assert!(
            !text.contains("tool_invoke"),
            "there is no verb to reach a tool through: {text}"
        );
        assert!(
            !text.contains("file_write"),
            "and nothing this read-only run does not have: {text}"
        );
    }

    #[test]
    fn the_verbs_surface_still_carries_the_whole_catalogue_in_the_instruction() {
        // The measurement that put it there stands: discovering the catalogue through the verbs
        // cost four calls in ten, each a billed round trip replayed in every later turn.
        let dir = tempfile::tempdir().expect("temporary directory");
        let catalogue = a_catalogue(dir.path());
        let text = instructions(
            &options(&["--surface", "verbs"]),
            &catalogue,
            "",
            Owned::default(),
        )
        .expect("the default is available");

        assert!(text.contains("tool_search"), "{text}");
        assert!(text.contains("file.read"), "with its operation: {text}");
        assert!(text.contains("max_bytes"), "and its arguments: {text}");
    }

    #[test]
    fn every_surface_keeps_the_sentences_that_stop_a_run_reporting_work_it_did_not_do() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let catalogue = a_catalogue(dir.path());
        for surface in [vec![], vec!["--surface", "verbs"]] {
            let text = instructions(&options(&surface), &catalogue, "", Owned::default())
                .expect("available");
            for stale in ["workspace_read", "workspace_list", "workspace_grep"] {
                assert!(!text.contains(stale), "`{stale}` no longer exists: {text}");
            }
            // The regression that made a live run change nothing and say it had: the standing
            // instruction told a write-capable run that it could not write.
            assert!(
                !text.contains("read-only"),
                "what a run can do is the toolset's answer, not this text's: {text}"
            );
            assert!(text.contains("Never report work as done"), "{text}");
            assert!(text.contains("was not approved did not happen"), "{text}");
        }
    }

    #[test]
    fn the_instruction_says_where_the_run_is_and_what_day_it_is() {
        // A run that does not know its own directory spends a turn asking for `pwd`, and one that
        // does not know the date writes a dated note with the wrong date.
        let dir = tempfile::tempdir().expect("temporary directory");
        let workspace = dir.path().canonicalize().expect("canonical");
        let text = instructions(
            &options(&["--workspace", workspace.to_str().expect("utf-8 path")]),
            &a_catalogue(&workspace),
            "",
            Owned::default(),
        )
        .expect("the default is available");

        assert!(
            text.contains(&workspace.display().to_string()),
            "the absolute workspace path: {text}"
        );
        assert!(
            text.contains(&environment::utc_date(std::time::SystemTime::now())),
            "and today's date: {text}"
        );
    }

    #[test]
    fn the_project_own_instruction_file_is_carried_and_can_be_left_out_for_a_control() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let workspace = dir.path().canonicalize().expect("canonical");
        fs::write(
            workspace.join("AGENTS.md"),
            "never touch the generated directory",
        )
        .expect("write");
        let path = workspace.to_str().expect("utf-8 path");

        let text = instructions(
            &options(&["--workspace", path]),
            &a_catalogue(&workspace),
            "",
            Owned::default(),
        )
        .expect("the default is available");
        assert!(
            text.contains("never touch the generated directory"),
            "the project's own words reach the run: {text}"
        );

        // The control. The environment block stays either way — where a run is, is not an
        // experimental treatment.
        let text = instructions(
            &options(&["--workspace", path, "--no-project-instructions"]),
            &a_catalogue(&workspace),
            "",
            Owned::default(),
        )
        .expect("the default is available");
        assert!(
            !text.contains("never touch the generated directory"),
            "{text}"
        );
        assert!(text.contains("## Environment"), "{text}");
    }

    #[test]
    fn a_declared_scope_is_stated_up_front_so_no_turn_is_spent_discovering_it() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let catalogue = a_catalogue(dir.path()).scoped(harness_tools::Scope::of(vec![
            harness_tools::ScopeRule::parse(".engineering/planning/**=partial-only")
                .expect("a rule"),
        ]));
        let text = instructions(&options(&[]), &catalogue, "", Owned::default())
            .expect("the default is available");

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
        let text = instructions(
            &options(&["--scope-announce", "silent"]),
            &catalogue,
            "",
            Owned::default(),
        )
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
            instructions(&options, &a_catalogue(dir2.path()), "", Owned::default())
                .expect("readable"),
            "be terse"
        );
    }

    /// The whole `run` invocation, so the flags `chat` does not take can be exercised.
    fn a_run(arguments: &[&str]) -> RunCommand {
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
        let Command::Run(command) = parse(&[base, arguments.to_vec()].concat())
            .expect("the arguments parse")
            .command
        else {
            panic!("the run subcommand parses to run options");
        };
        *command
    }

    #[test]
    fn delegation_is_off_until_it_is_asked_for_and_then_carries_its_own_turn_ceiling() {
        assert_eq!(delegation(&options(&[])), None, "a new tool is opt-in");
        let on = delegation(&options(&["--delegate"])).expect("published");
        assert_eq!(on.name.as_str(), harness_loop::DEFAULT_DELEGATE_NAME);
        assert_eq!(on.max_turns, harness_loop::DELEGATE_MAX_TURNS);
        assert_eq!(
            delegation(&options(&["--delegate", "--delegate-turns", "3"]))
                .expect("published")
                .max_turns,
            3
        );
    }

    #[test]
    fn a_turn_ceiling_for_a_delegate_nobody_published_is_a_parse_error() {
        // And its own default must not trip that rule, or `run` would never parse at all.
        parse(&[
            "b10x-harness",
            "run",
            "--base-url",
            "u",
            "--model",
            "m",
            "--input",
            "hi",
            "--delegate-turns",
            "3",
        ])
        .expect_err("a ceiling on a tool this run does not publish means nothing");
    }

    #[test]
    fn a_delegate_that_may_take_no_turn_at_all_is_a_parse_error_naming_the_flag() {
        // Refused by clap, so the run never starts and the message names what the operator typed.
        // Left to the child's `Budget::validate` this same zero is discovered once per delegation
        // — after a paid parent turn asked for one — while the parent's `--max-turns 0` is
        // refused before the first request. One rule, both bounds.
        let error = parse(&[
            "b10x-harness",
            "run",
            "--base-url",
            "u",
            "--model",
            "m",
            "--input",
            "hi",
            "--delegate",
            "--delegate-turns",
            "0",
        ])
        .expect_err("a delegate that may take no turn is a delegate that can report nothing");
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::ValueValidation,
            "a refused value, which clap exits 2 for, not a run that starts: {error}"
        );
        let message = error.to_string();
        assert!(
            message.contains("--delegate-turns"),
            "the message names the flag: {message}"
        );
    }

    #[test]
    fn chat_takes_no_output_schema_because_a_conversation_has_no_single_end() {
        parse(&[
            "b10x-harness",
            "chat",
            "--base-url",
            "u",
            "--model",
            "m",
            "--output-schema",
            "s.json",
        ])
        .expect_err("`--output-schema` is `run`'s alone");
        // The two `chat` does take.
        parse(&[
            "b10x-harness",
            "chat",
            "--base-url",
            "u",
            "--model",
            "m",
            "--delegate",
            "--hooks",
            "h.json",
        ])
        .expect("delegation and hooks are a conversation's business too");
    }

    #[test]
    fn an_output_schema_is_read_from_the_file_and_refused_in_the_loops_own_words() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("schema.json");
        fs::write(
            &path,
            r#"{"type": "object", "properties": {"verdict": {}}}"#,
        )
        .expect("write");
        let command = a_run(&["--output-schema", path.to_str().expect("utf-8 path")]);
        let published = schema(command.output_schema.as_deref())
            .expect("an object schema")
            .expect("published");
        assert_eq!(published.name.as_str(), harness_loop::DEFAULT_ANSWER_NAME);

        fs::write(&path, r#"{"type": "string"}"#).expect("write");
        let error = schema(Some(path.as_path())).expect_err("refused before the run");
        assert!(error.contains("JSON Schema for an object"), "{error}");
        assert!(error.contains("schema.json"), "the file is named: {error}");

        let missing = schema(Some(dir.path().join("absent.json").as_path()))
            .expect_err("a file that is not there refuses the run");
        assert!(missing.contains("absent.json"), "{missing}");
    }

    #[test]
    fn a_hooks_file_that_this_build_cannot_read_refuses_the_run_rather_than_running_without_it() {
        // The pre-loop refusal: a run started with `--hooks` and no hooks is a run whose gate the
        // operator thinks is there.
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("hooks.json");
        fs::write(&path, r#"{"version": 9, "hooks": []}"#).expect("write");
        let error = hooks::Hooks::load(&path).expect_err("refused");
        assert!(error.contains("version 1"), "{error}");
    }

    #[test]
    fn the_instruction_names_the_two_tools_the_loop_owns_only_when_the_run_has_them() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let catalogue = a_catalogue(dir.path());
        let without = instructions(&options(&[]), &catalogue, "", Owned::default())
            .expect("the default is available");
        assert!(!without.contains("`delegate`"), "{without}");
        assert!(!without.contains("`answer`"), "{without}");

        let with = instructions(
            &options(&[]),
            &catalogue,
            "",
            Owned {
                delegate: Some("delegate"),
                answer: Some("answer"),
                skills: None,
                agents: None,
            },
        )
        .expect("the default is available");
        assert!(
            with.contains("`delegate` hands one self-contained sub-task"),
            "one line, because the tool's own description carries the rest: {with}"
        );
        assert!(with.contains("Finish by calling `answer`"), "{with}");
    }
}
