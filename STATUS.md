# Status

Observed on 2026-08-21, after the Responses wire, bridge mode, and an independent review.

| Area | State | Next evidence |
| --- | --- | --- |
| Source | canonical repository `beyond10x/harness`; extracted from the daemonloom monorepo at 1e074923; own workspace, own gate, no dependency on any other repository | keep the no-dependency boundary as the component grows |
| Architecture | the split from `runtime/agent`'s bridges is accepted by ADR 0052 | none pending; the component is registered in `architecture/STATUS.md` |
| Neutral values | `harness-wire` implements items, tool specs, turns, usage, stream events, the three ports and every size bound; positive and adversarial tests pass | none pending for the Responses slice; the Messages wire is what tests whether the abstraction holds |
| Responses wire | `POST {base}/responses` streaming, SSE decode, request projection, tool-call decode, reasoning-item preservation, usage, stop reasons, cancellation, and typed status mapping pass against a real socket | characterize one authorized live endpoint and retain the evidence |
| Loop | turn assembly, tool round trips, approvals, cancellation between turns and between calls, turn/token/deadline budgets, and refusal of an unenforceable spend ceiling all pass | add the wall-clock deadline test; it is enforced but proven only by construction |
| Workspace tools | read-only `list`/`read`/`grep`, bounded. Every path is re-checked after canonicalization — including each entry `grep` walks into, which previously followed a symlink out of the workspace and returned outside files under a workspace-relative name | none pending; writing and executing are a separate slice |
| Command line | `run` and `tools`, credential from an explicitly named file or variable, prose and JSONL output, Ctrl-C cancelling the run rather than the process, three distinct exit statuses | none pending |
| Wire contract | `contracts/provider-wires/openai-responses/2026-08-21` pins the exact request and stream; `contracts/app-server-profile/codex-app-server-stdio-v2/2026-08-21` pins the served JSON-RPC subset and a full connection trace. Each is checked from both directions | pin the Messages wire the same way when it lands |
| Bridge mode | `b10x-harness app-server` serves the pinned subset on stdio under profile `codex-app-server-stdio-v2-dynamic-operation-tools-experimental` — the client profile that actually admits dynamic tools. Registering tools requires the client to negotiate `experimentalApi`, and is refused by name otherwise; a text-only thread needs no capability. `thread/resume` and `turn/steer` refuse by name. An interrupt is acted on when its frame is decoded, acknowledged between streamed events, and distinguished from a client that merely vanished | **run `runtime/agent`'s real bridge against this binary.** Everything so far is this component's own client, written from the bridge's source; the two processes have never spoken, and no gate compares the two inventories |
| Cancellation | one token reaches the loop, the tool sequence and the HTTP body being read. A cancelled read is a terminal outcome, not an error, so a person who cancels is not told something broke | none pending |
| Messages wire | **not started** | project `/v1/messages` onto the same loop |
| Subscription auth | **works today without code**, and that is the finding: the ChatGPT/Codex access token out of `~/.codex/auth.json` is accepted as a plain bearer, and `authorization` alone is enough — `chatgpt-account-id`, `originator` and `OpenAI-Beta` were each dropped in turn and the endpoint still answered 200. No `BearerSource` implementation is needed to *use* it | a `BearerSource` that reads and refreshes the token, since a pasted one expires and nothing here renews it |
| Live provider | **first live run: 2026-08-23**, against `https://chatgpt.com/backend-api/codex` under the operator's own ChatGPT subscription credential, model `gpt-5.6-sol`. Two turns, two tool round trips, usage reported, `finished{completed}`. It found a real defect on turn 1 — the whole workspace toolset was named illegally for this wire (see the changelog) — which is exactly what the emulator could not find | pin a `2026-08-23` contract from live bytes rather than emulated ones; the current pin is still emulator-derived |
| Embedding | **not started.** Nothing embeds this component yet | a `runtime/agent` direct-provider adapter binding `ToolPort` to its capability compiler |
| Substrate confinement | **publication proven, execution blocked.** The probe works against a real daemon: `GET /v1/machine` on this machine reports `exec.argv-only`, all three cgroup limits, all six namespaces, `exec.no-egress` and `workspace.guarded-io`, and the toolset grows from three tools to six accordingly. **No confined operation has ever succeeded.** `POST /v1/workspaces` answers `422 request.schema-invalid` at `input` for every body derivable from the committed 0.2.0 and 0.4.0 contracts and from the daemon binary's own strings — bare and `input`-wrapped, `source` as the string `empty` and as `{"empty": {}}`, with and without `lease_ttl_ms`, with empty and non-empty `labels`, and with the capability snapshot at body and input level. The daemon on this box embeds `substrate-wire/0.4.0`, reports `driver_version 0.2.0`, and was installed on 2026-08-16 from a source this repository does not have | the daemon's own accepted `workspace.create` body — from whoever owns that deployment, or from a daemon built out of `beyond10x/substrate`. `tests/live.rs` is the standing probe: point `B10X_SUBSTRATE_SOCKET` at a socket and run it `--ignored` |

## What this component does not claim

- **No Substrate confinement.** Like the model-only Codex routes under ADR 0051, this harness's
  effects are exactly what its published toolset admits, and nothing constrains it further.
- **No live-provider conformance.** The deterministic local endpoint proves the pieces agree with
  each other. It proves nothing about how a real provider behaves.
- No delegation, structured output, realtime media, provider-side sessions, or durable resume.
- No hosted service, no admission transport, no durable store.

## Test counts

189 tests pass across the workspace:

| Crate | Unit | Integration |
| --- | --- | --- |
| `harness-wire` | 28 | — |
| `harness-responses` | 37 | 16 provider-emulated, 4 contract |
| `harness-loop` | 30 | — |
| `harness-app-server` | 19 | 5 contract |
| `harness-cli` | 26 | 7 end-to-end, 17 bridge-mode |

The provider-emulated, end-to-end and bridge-mode suites drive real processes: a local HTTP endpoint
over a socket, and the built binary over pipes.
