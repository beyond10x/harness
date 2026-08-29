# harness

The b10x agent loop — ours, not a vendor's. It talks to LLM APIs directly and owns the cycle: one
turn out, tool calls back, results in, next turn out.

The problem it removes: driving a vendor's harness means booting a vendor binary whose only job is
to hold a loop, registering every tool through that vendor's mechanism, and living with whatever
budgets that vendor happens to enforce. Owning the loop makes tool names, budgets, cost accounting
and approval decisions ours.

It is deliberately small and carries no bridges to vendor binaries. **It depends on one other
component in `beyond10x` — [substrate](https://github.com/beyond10x/substrate), pinned by git
revision — and on nothing that could embed it.** The arrow points inward — something else embeds
this, never the reverse.

## Where it sits

| direction | what |
|---|---|
| observed by | [metaharness](https://github.com/beyond10x/metaharness) — its `b10x` adapter launches `b10x-harness run` and reads the `--json` record. Observed, not driven: the published toolset already *is* the policy |
| confined by | [substrate](https://github.com/beyond10x/substrate) — embedded in-process, or over the daemon's socket |
| reaches models through | any OpenAI-Responses endpoint, e.g. [llmgw](https://github.com/beyond10x/llmgw), or any Anthropic-Messages one |
| mapped in | [atlas](https://github.com/beyond10x/atlas) |

## Status

**Pre-v1. Tagged `0.1.0` (2026-08-24).** The per-area state, with the exact next piece of evidence
each area is waiting for, is [`STATUS.md`](STATUS.md) — read that before believing anything here.

| area | state |
|---|---|
| `openai-responses` wire | implemented, streaming, pinned by contract |
| `anthropic-messages` wire | implemented, streaming, pinned by contract. Selected with `--wire`; the loop below cannot tell which it got |
| the loop: turns, tool round trips, approvals, budgets, cancellation | implemented |
| command line (`run`, `tools`, `app-server`, `events`) | implemented |
| bridge mode (Codex app-server JSON-RPC over stdio) | implemented; **no real external bridge has ever driven it**, and no gate compares the two method inventories |
| substrate confinement, embedded | working, including execution — but `run` has been *published*, not yet *exercised* against a confined process |
| substrate over a socket | **blocked and parked** — see `STATUS.md` |
| live provider | first live run 2026-08-23. It found a real defect on turn 1 that the emulator could not: the whole workspace toolset was named illegally for that wire |
| embedding | **not started.** Nothing embeds this component yet |

Most evidence is `provider_emulated` and is never promoted to a claim about how a real provider
behaves. The wire contract pins are still emulator-derived.

## Build, test, run

The gate is **`bash scripts/gate.sh`**. Green here is the bar for main.

| step | command |
|---|---|
| tests | `cargo test --workspace --locked` |
| format | `cargo fmt --all --check` |
| lint | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| provider-wire pins | `python3 scripts/check-provider-wires.py` |
| app-server profile pin | `python3 scripts/check-app-server-profile.py` |
| brand | org-wide, from the atlas checkout: `bash ../atlas/scripts/check-org-brand.sh harness` |

Rust 1.97, edition 2024. The binary is `b10x-harness`.

```text
cargo run -p b10x-harness-cli -- run \
  --base-url https://llmgw.example/v1 \
  --model <alias> \
  --api-key-file <path> \
  --workspace . \
  --input "what does this workspace do?"
```

`--api-key-env <NAME>` is the alternative. **There is no ambient fallback** — the harness reads no
credential it was not pointed at, so a run can always be explained afterwards.

| flag | effect |
|---|---|
| `--json` | one event per line on stdout instead of prose |
| `--prices <card>` | a JSON document of rates, with its own `source` and `as_of`; the record then carries the cost and the card that produced it |
| `--substrate <socket>` / `--substrate-embedded` | write and execute inside a confined workspace. Named and not available — a directory not called `ws_…`, a driver that does not open, no daemon at the socket — **refuses the run** (exit 1) rather than quietly running read-only |
| `--cgroup-root` | the containing slice, when running inside a delegated cgroup |
| `--yes` | approve what asks. Every write and every `run` asks — the loop judges each call's declared risk against a ceiling that defaults to low — and without `--yes` the default approver denies them and the model is told |

Exit status distinguishes the three outcomes a caller acts on differently: `0` the model answered,
`2` the run stopped for a named reason, `1` the harness could not run.

### Running with confinement

The embedded driver serves execution only where the machine can confine one, and substrate decides:
its probe requires the process to be **inside** a delegated cgroup subtree whose root is
process-free and carries `cpu`, `memory` and `pids`. A process cannot move itself into one — the
kernel refuses a write across delegation domains — so it has to be *started* there:

```console
systemd-run --user --scope --property="Delegate=cpu memory pids" -- ./run.sh
```

and inside, pass the scope's containing slice as `--cgroup-root`. Without it the run has five tools
and no `run`; with it, six. That is the toolset following the machine rather than a flag.

The workspace is **adopted, not created**: `--workspace` is the tree, its parent becomes substrate's
root, and reads and writes land in the same place. The directory must therefore be named
`ws_something` — substrate's guarded filesystem will not represent any other name — and one that is
not refuses the run by name rather than quietly running read-only:

```text
error: `--substrate-embedded` cannot adopt `not_a_ws`: the directory must be named `ws_` followed by alphanumerics and underscores, because substrate's guarded filesystem represents no other name. Rename it, or drop the flag for a read-only run.
```

`--toolchain rust` mounts the operator's `~/.rustup` read-only inside the sandbox and points
`CARGO_HOME` at **`<workspace>/.cargo`** — never at the operator's `~/.cargo`, which holds a
registry credential. Nothing seeds that directory: a confined build has no network, so the caller
copies the package cache the task needs (`registry/` from `~/.cargo`) into `<workspace>/.cargo`
before the run, or the first `cargo build` fails inside cargo looking for a crate it cannot fetch.

## What the model sees

Exactly **three verbs** — `tool_search`, `tool_describe`, `tool_invoke` — over a
[catalogue](crates/harness-tools/src/catalogue.rs) whose entries are named by neutral operations.

This was originally the opposite: each admitted operation published directly as its own model tool.
The reason that gave still holds — the catalogue has six entries and would fit under any vendor
ceiling, so the indirection is not a dodge — but it was outweighed. The evaluation compares arms
across three harnesses that each name their tools differently (`Bash` here, `run` there, `Write`
and `workspace_write` for one act), so everything that read a run had to learn one vendor's
vocabulary. Three verbs over one catalogue makes the names **ours everywhere**. The cost is a turn
spent on `tool_describe` before a first call; whether that shows up in practice is an experiment,
not a claim.

The catalogue is what the machine can perform: three entries with no backend, five with a confined
workspace, six inside a delegated cgroup. Reads are bounded to the workspace root, and every path
is re-checked after canonicalization — including each entry `grep` walks into — so a symlink inside
the workspace cannot be used to read outside it.

With no `--substrate` or `--substrate-embedded`, nothing the run can call changes a file or starts
a process. **That is a fact about the machine, not a promise this README makes to the model**: the
standing instruction states no effects at all and tells the run to ask `tool_search`. It used to
name three tools that no longer exist and assert that none of them could write — and a measured run
given a write-and-execute catalogue believed it, read two files, changed nothing, and reported the
task done.

## Budgets and cost

`max_turns`, input and output token totals, a wall-clock deadline and — with `--prices` — a spend
ceiling are counted here, so they are enforced here. A bound that *cannot* be enforced is
[refused by name](crates/harness-loop/src/budget.rs) rather than accepted and ignored: a spend
ceiling on a run with no rates to measure it against stops the run before the first request instead
of pretending.

No provider on this wire returns a price. Rates are declared in a `--prices` card and never
compiled in — a table baked into this binary would be numbers nobody could date, wrong silently the
first time one moved. Without a card the run reports tokens and no price, and a model the card does
not list is **warned about by name** rather than reported as free.

Approval is a **blocking call**, not a protocol round trip that can land after the effect. What
asks is derived from the call, never declared by the tool: the loop reads the envelope of the
catalogue entry a `tool_invoke` names and asks when its risk is above the run's unattended ceiling
(default low), or when it is a non-idempotent write. The default approver denies; `--yes` approves.

## Two shells, one loop

| shell | what it is |
|---|---|
| embedded | `harness-loop` as a library; tools bound in-process, no IPC |
| command line | `b10x-harness run`, over a read-only workspace |
| bridge | `b10x-harness app-server`, a process speaking the Codex app-server JSON-RPC format |

The seam that makes this possible is [`ToolPort`](crates/harness-wire/src/port.rs): in-process it
is a direct call, under a bridge it is a callback over the wire, and the loop cannot tell.

Bridge mode declares the client's **operation-tools** profile, not the plain
`codex-app-server-stdio-v2` — that one is the *stable* profile and admits no dynamic tools, so
declaring it while emitting tool frames yields a server that looks compatible and fails at the
first tool call. The client therefore has to negotiate `experimentalApi`; registering tools without
it is refused by name, and a text-only thread works either way. `thread/resume` and `turn/steer`
are refused by name rather than answered with a silent success.

## Wires

| wire | endpoint | state |
| --- | --- | --- |
| `openai-responses` | `POST {base}/responses`, streaming | implemented |
| `anthropic-messages` | `POST {base}/messages`, streaming | implemented |

`b10x-harness run --wire anthropic-messages` picks the second one. The wire is a branch in exactly
one function; below it the loop holds a `ModelPort` and cannot tell which projection it got, which
is the whole reason a second wire cost a second projection instead of a second loop.

Turns are **stateless**: nothing is retained on the far side and the whole conversation is replayed
each time. Reasoning — `reasoning` items on one wire, `thinking` and `redacted_thinking` blocks on
the other — is carried verbatim as [`Item::Opaque`](crates/harness-wire/src/item.rs) tagged with the
wire that produced it, so the model keeps its own chain of thought across a tool round trip and
cannot have one provider's blob replayed into another's. Replaying one into a wire that did not
produce it is a typed refusal naming both wires, not a silent drop.

Both wires are exercised by the **same** loop suite over a real socket: the same case names against
the same scenario names, with a test that fails if either side grows a case the other lacks.

## Layout

| crate | owns |
|---|---|
| `crates/harness-wire` | neutral values plus `ModelPort`, `ToolPort` and `BearerSource`. No I/O, no clock, no vendor field name. It defines the credential types; it reads and sends none |
| `crates/harness-credential` | credential sources that read exactly what a caller pointed them at. Nothing vendor-shaped: how a fetched credential is *presented* belongs to the wire |
| `crates/harness-responses` | the Responses projection and its HTTP/SSE client |
| `crates/harness-messages` | the Messages projection and its HTTP/SSE client |
| `crates/harness-loop` | the loop: turn assembly, tool round trips, approvals, budgets, cancellation |
| `crates/harness-flow` | a workflow notation the loop runs natively: a DAG of sub-trees, validated before anything runs |
| `crates/harness-substrate` | a client of the substrate wire: what this machine can confine, and the tools that answer |
| `crates/harness-tools` | one catalogue, published to every harness under three verbs |
| `crates/harness-app-server` | the Codex-format JSON-RPC server, and the wire-backed `ToolPort` |
| `crates/harness-cli` | the `b10x-harness` binary and the read-only workspace tools |

| path | holds |
|---|---|
| `contracts/provider-wires/` | the exact request sent and the exact stream accepted, per dated pin |
| `contracts/app-server-profile/` | the JSON-RPC subset served, per dated pin |
| `docs/design/` | component design documents |
| `scripts/` | `gate.sh` and the checks it runs |

## Evidence

Every contract pin is checked **from both directions** — a Python checker verifies the manifest
against its fixtures, and a Rust test verifies the code produces those bytes or holds those
constants. Neither half is sufficient alone.

Three suites drive real processes over real sockets and pipes:

| suite | drives |
|---|---|
| `crates/harness-responses/tests/provider_emulated.rs` | the first wire's client and the loop against a local HTTP endpoint |
| `crates/harness-messages/tests/provider_emulated.rs` | the same suite, pointed at the second wire |
| `crates/harness-cli/tests/end_to_end.rs` | the shipped binary over a real workspace |
| `crates/harness-cli/tests/bridge_mode.rs` | the shipped binary driven as a bridge would drive it |

## Not owned here

- **No substrate confinement claim.** This harness's effects are exactly what its published toolset
  admits, and nothing constrains it further.
- **No live-provider conformance.** One live run has happened; the pins are still emulator-derived.
- No delegation, structured output, realtime media, provider-side sessions, or durable resume.
- No hosted service, no admission transport, no durable store.

## Read more

- [`STATUS.md`](STATUS.md) — per-area state and the exact next evidence each area waits for.
- [`ROADMAP.md`](ROADMAP.md) — the outcome roadmap; a phase advances only when its exit evidence
  exists.
- [`CHANGELOG.md`](CHANGELOG.md) — what changed and what each change cost to learn.
- [`docs/design/0001-tool-envelope-and-substrate-confinement.md`](docs/design/0001-tool-envelope-and-substrate-confinement.md)
- [`AGENTS.md`](AGENTS.md) — working agreements and invariants for anyone changing this repo.
