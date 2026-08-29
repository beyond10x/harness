# Status

Observed on 2026-08-29, after the second wire. The previous observation was 2026-08-21.

| Area | State | Next evidence |
| --- | --- | --- |
| Source | canonical repository `beyond10x/harness`; own workspace, own gate, no dependency on any other repository | keep the no-dependency boundary as the component grows |
| Architecture | the split from `runtime/agent`'s bridges is accepted by ADR 0052 | none pending; the component is registered in `architecture/STATUS.md` |
| Neutral values | `harness-wire` implements items, tool specs, turns, usage, stream events, the three ports and every size bound; positive and adversarial tests pass. **The second wire held it, at the cost of two widenings**, each with its reason recorded where it lands: `Usage::cache_creation_input_tokens` (an `Option`, because a route that reports no cache-write figure has not said there was none) and `BearerSource::kind` (the same secret travels under different header names on one endpoint's two routes, so which one a credential *is* stopped being derivable from the wire alone). `Usage` also now states the invariant it had only ever implied — `input_tokens` is the whole and the cache figures are parts of it | none pending. What the second wire found wrong is one layer up, not here: see *Messages wire* |
| Responses wire | `POST {base}/responses` streaming, SSE decode, request projection, tool-call decode, reasoning-item preservation, usage, stop reasons, cancellation, and typed status mapping pass against a real socket | characterize one authorized live endpoint and retain the evidence |
| Loop | turn assembly, tool round trips, approvals, cancellation between turns and between calls, turn/token/deadline budgets, and refusal of an unenforceable spend ceiling all pass | add the wall-clock deadline test; it is enforced but proven only by construction |
| Workspace tools | read-only `list`/`read`/`grep`, bounded. Every path is re-checked after canonicalization — including each entry `grep` walks into, which previously followed a symlink out of the workspace and returned outside files under a workspace-relative name | none pending; writing and executing are a separate slice |
| Command line | `run` and `tools`, credential from an explicitly named file or variable, prose and JSONL output, Ctrl-C cancelling the run rather than the process, three distinct exit statuses. **`--wire openai-responses\|anthropic-messages`** on `run` and `app-server`, defaulting to the wire this harness shipped with so every invocation written before the second wire still means what it did; a named OAuth source with `--oauth-token-file`/`--oauth-token-env` and an optional `--oauth-token-pointer`, mutually exclusive with the API-key flags | none pending |
| Wire contract | `contracts/provider-wires/openai-responses/2026-08-21` and `contracts/provider-wires/anthropic-messages/2026-08-29` pin the exact request and stream; `contracts/app-server-profile/codex-app-server-stdio-v2/2026-08-21` pins the served JSON-RPC subset and a full connection trace. Each is checked from both directions. The Messages pin adds two halves the first wire has no equivalent of: the `content_block_delta` sub-types — on that route the interesting variation is *inside* one outer event name — and the **header names each credential kind travels under**, checked against the very function the client calls to build them | re-pin both from live bytes; the Messages pin is emulator-derived and no subscription route has been contacted |
| Bridge mode | `b10x-harness app-server` serves the pinned subset on stdio under profile `codex-app-server-stdio-v2-dynamic-operation-tools-experimental` — the client profile that actually admits dynamic tools. Registering tools requires the client to negotiate `experimentalApi`, and is refused by name otherwise; a text-only thread needs no capability. `thread/resume` and `turn/steer` refuse by name. An interrupt is acted on when its frame is decoded, acknowledged between streamed events, and distinguished from a client that merely vanished | **run `runtime/agent`'s real bridge against this binary.** Everything so far is this component's own client, written from the bridge's source; the two processes have never spoken, and no gate compares the two inventories |
| Cancellation | one token reaches the loop, the tool sequence and the HTTP body being read. A cancelled read is a terminal outcome, not an error, so a person who cancels is not told something broke | none pending |
| Messages wire | **implemented.** `POST {base}/messages` streaming, SSE decode, role-alternating message projection, `tool_use`/`tool_result` blocks, `thinking` and `redacted_thinking` preserved as opaque items and replayed verbatim and in place, disjoint usage figures summed to the neutral total, stop reasons including `refusal` and `pause_turn` carried under their own names rather than flattened into *finished*, cancellation, and typed status mapping. Both wires pass the **same** 20-case loop suite against a real socket, and a test compares the two emulators' scenario declarations so the two halves cannot drift apart | **the transport half is duplicated, and that is the finding.** SSE framing, the retry rule, the witnessed sink that makes the retry rule safe, the back-off and the status mapping are a near-copy of `harness-responses` — none of it vendor-shaped, and the second wire proved it by needing all of it unchanged. A `harness-http` beneath both wires is what that argues for; it was not done here because this change is the one that produces the evidence, not the one that should act on it |
| Subscription auth | **a `BearerSource` exists; it does not renew.** `harness_credential::SubscriptionToken` reads a token from a file or an environment variable the caller **names**, optionally at a caller-named JSON pointer, and re-reads it on **every** call — so a token an owner outside this process renews is followed without restarting the run. It declares `CredentialKind::Oauth`, and the Messages wire presents that as `authorization: Bearer` **plus** `anthropic-beta: oauth-2025-04-20`, while a key issued to a program goes to `x-api-key`. There is still no default path and no vendor directory anything here looks in, and no fallback when the named source is missing. The ChatGPT/Codex finding stands: that route takes its access token as a plain bearer and needs no per-route header | **renewal, and one authorized run on each route.** Nothing here holds a refresh token or calls an authorization server, so a token nobody renews expires and the run fails by name. The Anthropic header shapes are `provider_emulated`: no subscription route has been contacted |
| Live provider | **first live run: 2026-08-23**, against `https://chatgpt.com/backend-api/codex` under the operator's own ChatGPT subscription credential, model `gpt-5.6-sol`. Two turns, two tool round trips, usage reported, `finished{completed}`. It found a real defect on turn 1 — the whole workspace toolset was named illegally for this wire (see the changelog) — which is exactly what the emulator could not find | pin a `2026-08-23` contract from live bytes rather than emulated ones; the current pin is still emulator-derived |
| Embedding | **not started.** Nothing embeds this component yet | a `runtime/agent` direct-provider adapter binding `ToolPort` to its capability compiler |
| Substrate confinement | **working, embedded, including execution.** `Backend` has two implementations and the tools cannot tell them apart: `Embedded` holds substrate's `HostDriver` in this process, `Client` reaches a daemon over a socket. Workspace adoption means the tree a run reads is the tree it writes. With a delegated cgroup the toolset is six tools — `workspace_list/read/grep`, `workspace_write`, `workspace_edit`, `run`; without one it is five; with no backend at all it is the three this component has always shipped | exec has been *published* and not yet *exercised*: no confined process has been started through `run` |
| Substrate over a socket | **blocked, and parked.** `POST /v1/workspaces` on the daemon this machine runs answers `422 request.schema-invalid` at `input` for every body derivable from the committed 0.2.0 and 0.4.0 contracts. That daemon embeds `substrate-wire/0.4.0`, reports `driver_version 0.2.0`, and was installed on 2026-08-16 from a source this repository does not have. Embedding made the question moot for a simple run; the socket path is what an integrated deployment will need | the daemon's own accepted `workspace.create` body, or a daemon built from `beyond10x/substrate`. `tests/live.rs` is the standing probe |

## What this component does not claim

- **No Substrate confinement.** Like the model-only Codex routes under ADR 0051, this harness's
  effects are exactly what its published toolset admits, and nothing constrains it further.
- **No live-provider conformance.** The deterministic local endpoint proves the pieces agree with
  each other. It proves nothing about how a real provider behaves.
- No delegation, structured output, realtime media, provider-side sessions, or durable resume.
- No hosted service, no admission transport, no durable store.

## Test counts

412 tests pass across the workspace and 1 is ignored (`bash scripts/gate.sh`, 2026-08-29):

| Crate | Unit | Integration |
| --- | --- | --- |
| `harness-wire` | 37 | — |
| `harness-credential` | 7 | — |
| `harness-responses` | 39 | 18 provider-emulated, 4 contract |
| `harness-messages` | 45 | 20 provider-emulated, 6 contract |
| `harness-loop` | 57 | — |
| `harness-flow` | 27 | — |
| `harness-substrate` | 29 | 4 embedded-live; `live` ignored, it needs a daemon |
| `harness-tools` | 30 | — |
| `harness-app-server` | 19 | 5 contract |
| `harness-cli` | 39 | 9 end-to-end, 17 bridge-mode |

The provider-emulated, end-to-end and bridge-mode suites drive real processes: a local HTTP endpoint
over a socket, and the built binary over pipes. The two provider-emulated suites are the **same**
suite pointed at two wires — same case names, same scenario names — and
`the_two_wires_serve_the_same_scenarios` fails if either side grows a case the other lacks.
