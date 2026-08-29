# Status

Observed on 2026-08-29, at `0.1.0` plus the substrate pin. The previous observation was 2026-08-21.

| Area | State | Next evidence |
| --- | --- | --- |
| Source | canonical repository `beyond10x/harness`; own workspace, own gate. One dependency on a sibling — `substrate-host` and `substrate-wire`, pinned by git revision in `crates/harness-substrate/Cargo.toml` (AGENTS.md invariant 2) — and none on anything that could embed this | swap the revision for a `tag` when substrate tags past `0.2.0` |
| Architecture | the split from `runtime/agent`'s bridges is accepted by ADR 0052 | none pending; the component is registered in `architecture/STATUS.md` |
| Neutral values | `harness-wire` implements items, tool specs, turns, usage, stream events, the three ports and every size bound; positive and adversarial tests pass | none pending for the Responses slice; the Messages wire is what tests whether the abstraction holds |
| Responses wire | `POST {base}/responses` streaming, SSE decode, request projection, tool-call decode, reasoning-item preservation, usage, stop reasons, cancellation, and typed status mapping pass against a real socket | characterize one authorized live endpoint and retain the evidence |
| Loop | turn assembly, tool round trips, approvals derived from each call's envelope against an unattended ceiling (default low; the approval gate fired for no shipped tool until 2026-08-29), cancellation and the wall-clock deadline between turns and between calls, turn/token/deadline budgets, and refusal of an unenforceable spend ceiling all pass | a ceiling flag on the command line, so a run can say `--approve-up-to medium` instead of all-or-nothing `--yes` |
| Workspace tools | read-only `workspace_list`/`workspace_read`/`workspace_grep`, published to the model under `tool_search`/`tool_describe`/`tool_invoke` over one catalogue, bounded. Every path is re-checked after canonicalization — including each entry `grep` walks into, which previously followed a symlink out of the workspace and returned outside files under a workspace-relative name | none pending. A dangling symlink was a door out of the workspace for `file_write` until 2026-08-29; presence is now `symlink_metadata` and a link that leads nowhere is refused. Writing and executing are the substrate rows below |
| Command line | `run`, `tools`, `app-server` and `events`; credential from an explicitly named file or variable, prose and JSONL output, Ctrl-C cancelling the run rather than the process, three distinct exit statuses. Confinement that was named and cannot be provided refuses the run by name (exit 1) instead of silently running read-only; `--substrate-embedded` is driven end to end | none pending |
| Wire contract | `contracts/provider-wires/openai-responses/2026-08-21` and `2026-08-22` (adds the optional sampling fields) pin the exact request and stream; `contracts/app-server-profile/codex-app-server-stdio-v2-dynamic-operation-tools-experimental/2026-08-21` pins the served JSON-RPC subset and a full connection trace. Each is checked from both directions | pin the Messages wire the same way when it lands; re-pin the Responses wire from live bytes (see *Live provider*) |
| Bridge mode | `b10x-harness app-server` serves the pinned subset on stdio under profile `codex-app-server-stdio-v2-dynamic-operation-tools-experimental` — the client profile that actually admits dynamic tools. Registering tools requires the client to negotiate `experimentalApi`, and is refused by name otherwise; a text-only thread needs no capability. `thread/resume` and `turn/steer` refuse by name. An interrupt is acted on when its frame is decoded, acknowledged between streamed events, and distinguished from a client that merely vanished | **run `runtime/agent`'s real bridge against this binary.** Everything so far is this component's own client, written from the bridge's source; the two processes have never spoken, and no gate compares the two inventories |
| Cancellation | one token reaches the loop, the tool sequence and the HTTP body being read. A cancelled read is a terminal outcome, not an error, so a person who cancels is not told something broke | none pending |
| Messages wire | **not started** | project `/v1/messages` onto the same loop |
| Subscription auth | **works today without code**, and that is the finding: the ChatGPT/Codex access token out of `~/.codex/auth.json` is accepted as a plain bearer, and `authorization` alone is enough — `chatgpt-account-id`, `originator` and `OpenAI-Beta` were each dropped in turn and the endpoint still answered 200. No `BearerSource` implementation is needed to *use* it | a `BearerSource` that reads and refreshes the token, since a pasted one expires and nothing here renews it |
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

353 tests pass across the workspace and 1 is ignored (`bash scripts/gate.sh`, 2026-08-29, after the review's fixes):

| Crate | Unit | Integration |
| --- | --- | --- |
| `harness-wire` | 35 | — |
| `harness-responses` | 38 | 18 provider-emulated, 4 contract |
| `harness-loop` | 65 | — |
| `harness-flow` | 27 | — |
| `harness-substrate` | 33 | 4 embedded-live; `live` ignored, it needs a daemon |
| `harness-tools` | 44 | — |
| `harness-app-server` | 19 | 5 contract |
| `harness-cli` | 34 | 10 end-to-end, 17 bridge-mode |

The provider-emulated, end-to-end and bridge-mode suites drive real processes: a local HTTP endpoint
over a socket, and the built binary over pipes.
