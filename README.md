# b10x harness

**Status: pre-v1, development implementation.** The b10x agent loop — our own, not a vendor's. It talks to LLM APIs
directly and owns the cycle: one turn out, tool calls back, results in, next turn out.

It is deliberately small, and it carries no bridges. [`runtime/agent`](https://github.com/daemonloom/daemonloom/blob/1e0749233b711744b6e50f9106bba2c33dbbf396/runtime/agent) is where the Codex
and Claude bridges live; this component drives no vendor binary and depends on nothing else in the
monorepo. The arrow points inward — something else embeds this, never the reverse.

## Why it exists

Every adapter `runtime/agent` owns is `AdapterClass::Harness`: a vendor keeps the loop and we drive
its surface. To reach an OpenAI-compatible gateway that way, a pinned Codex binary has to be booted
whose only job is to hold a loop. Every tool has to pass through a vendor tool-registration
mechanism. Any budget the vendor does not enforce cannot be enforced at all.

Owning the loop changes three things concretely:

- **Tools are published directly.** Each admitted operation is its own model tool with its real
  input schema — no `search`/`describe`/`invoke` indirection, no vendor tool ceiling to dodge.
- **Budgets bind.** `max_turns`, input and output token totals and a wall-clock deadline are
  counted here, so they are enforced here. A bound that cannot be enforced — a spend ceiling, since
  a gateway relays bytes and reports no price — is [refused by name](crates/harness-loop/src/budget.rs)
  rather than accepted and ignored.
- **Approval is a blocking call**, not a protocol round trip that can land after the effect.

## Two shells, one loop

| Shell | What it is | Status |
| --- | --- | --- |
| Embedded | `harness-loop` as a library; tools bound in-process, no IPC | implemented |
| Command line | `b10x-harness run`, over a read-only workspace | implemented |
| Bridge | a process speaking the Codex app-server JSON-RPC format | implemented |

The seam that makes this possible is [`ToolPort`](crates/harness-wire/src/port.rs): in-process it is
a direct call, under a bridge it is a callback over the wire, and the loop cannot tell.

`runtime/agent` already drives a process speaking the Codex format, and the command it spawns is
arbitrary — `AppServerChild::spawn` takes a `Command`. So bridge mode reuses that whole investment
without either component depending on the other:

```text
b10x-harness app-server \
  --base-url https://llmgw.example/v1 \
  --model <alias> \
  --api-key-env LLMGW_KEY
```

It declares the client's **operation-tools** profile, not the plain `codex-app-server-stdio-v2` —
that one is the *stable* profile and admits no dynamic tools, so declaring it while emitting tool
frames yields a server that looks compatible and fails at the first tool call. The client therefore
has to negotiate `experimentalApi`; registering tools without it is refused by name, and a
text-only thread works either way.

Tools arrive from the client on `thread/start` and are called back over the wire; the workspace
toolset is not published there. `thread/resume` and `turn/steer` are refused by name rather than
answered with a silent success. The real bridge has **not** yet driven this server, and no gate
compares the two method inventories — see [`STATUS.md`](STATUS.md).

## Wires

| Wire | Endpoint | Status |
| --- | --- | --- |
| `openai-responses` | `POST {base}/responses`, streaming | implemented |
| `anthropic-messages` | `POST {base}/messages`, streaming | not started |

Turns are **stateless**: `store: false`, the whole conversation replayed each time. Reasoning items
are carried verbatim as [`Item::Opaque`](crates/harness-wire/src/item.rs) tagged with the wire that
produced them, so the model keeps its own chain of thought across a tool round trip and cannot have
one provider's blob replayed into another's.

## Running it

```text
cargo run -p b10x-harness-cli -- run \
  --base-url https://llmgw.example/v1 \
  --model <alias> \
  --api-key-file <path> \
  --workspace . \
  --input "what does this workspace do?"
```

`--api-key-env <NAME>` is the alternative. There is no ambient fallback: the harness reads no
credential it was not pointed at, so a run can always be explained afterwards.

The published toolset is **read-only** — `workspace.list`, `workspace.read`, `workspace.grep`,
bounded to the workspace root. Every path is checked after canonicalization, including each entry
`grep` walks into, so a symlink inside the workspace cannot be used to read outside it. Nothing it can call
changes a file or runs a command, so a first live run costs inference and nothing else. Writing and
executing are a separate, separately-gated slice.

`--json` emits one event per line on stdout instead of prose. Exit status distinguishes the three
outcomes a caller acts on differently: `0` the model answered, `2` the run stopped for a named
reason, `1` the harness could not run.

## Workspace

- `harness-wire` — neutral values plus `ModelPort`, `ToolPort` and `BearerSource`. No I/O, no clock,
  no vendor field name. It defines the credential types; it reads and sends none.
- `harness-responses` — the Responses projection and its HTTP/SSE client.
- `harness-loop` — the loop: turn assembly, tool round trips, approvals, budgets, cancellation.
- `harness-app-server` — the Codex-format JSON-RPC server, and the wire-backed `ToolPort`.
- `harness-cli` — the `b10x-harness` binary and the read-only workspace tools.

## Evidence

`contracts/provider-wires/openai-responses/` pins the exact request the harness sends and the exact
stream it accepts. `contracts/app-server-profile/` pins the JSON-RPC format it serves. Every pin is
checked from both directions — a Python checker verifies the manifest against its fixtures, and a
Rust test verifies the code produces those bytes or holds those constants. Neither half is
sufficient alone.

Three suites drive real processes over real sockets and pipes:
`crates/harness-responses/tests/provider_emulated.rs` (client and loop),
`crates/harness-cli/tests/end_to_end.rs` (the shipped binary over a real workspace), and
`crates/harness-cli/tests/bridge_mode.rs` (the shipped binary driven as a bridge would drive it).

All of it is **`provider_emulated`** evidence and is never promoted to a claim about how a real
provider behaves. No live-provider run has happened, and the real bridge has not driven this
server.

## Not owned here

No Substrate confinement claim: like the model-only Codex routes under
[ADR 0051](https://github.com/daemonloom/daemonloom/blob/1e0749233b711744b6e50f9106bba2c33dbbf396/architecture/adr/0051-hosted-model-only-harness-routes-do-not-claim-substrate-confinement.md),
this harness's effects are exactly what its toolset admits and nothing constrains it further. No
delegation, structured output, realtime media, provider-side sessions, or durable resume.
