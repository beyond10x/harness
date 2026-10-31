---
format: aep.planning-md/3
id: review-result:async-loop-adversary-blast-radius
kind: review-result
status: active
title: 'Adversarial plan review: blast radius and completeness, async-native loop'
relations:
- reviews: epic:async-native-loop
- reviews: story:async-model-and-tool-ports
- reviews: story:loop-runs-as-a-future
- reviews: story:harness-llm-awaits-the-model
- reviews: story:catalogue-awaits-its-operations
- reviews: story:mcp-and-substrate-await-their-drivers
- reviews: story:binary-and-catalogue-on-one-runtime
- reviews: story:blocking-facade-keeps-sync-callers
prose_only: adversarial plan review; findings are a severity-marked prose list
revision: 1
---
verdict: gaps

The async direction is sound. It does not meet the goal on two counts. A synchronous caller holding a real `harness_llm` model has no working path after 0.23.0. And a tokio embedder gets no async `resume_approval` or `call`, while the shape of the Substrate `Backend` trait is left undecided.

**Affected repositories** (grepped every `Cargo.toml` and `Cargo.lock` outside `target/`)

| repo | crate depending on harness | pin | harness types used | effect of the final state | plan covers |
|---|---|---|---|---|---|
| uilab | uilab-agent | tag 0.15.1 | `harness_llm::client`, `Target`, `Source`, `llm_credentials`, `Box<dyn ModelPort + Send>`, `impl ToolPort for NoTools`, `AgentLoop::new(...).run_in`, `DenyAll`, `RunLedger`, `NullLoopSink`; run from `spawn_blocking` (`uilab-app/src/app.rs:974`) | `client` returns an async-only port. `run_in` is a facade over `Blocking*` ports. Nothing adapts async to blocking. **No sync path** | no |
| agentide | agentide-harness, -cli, -substrate | rev f0870131 (=0.15.1) | `client` via `connect()` returning a non-`Send` `Box<dyn ModelPort>` (`agentide-harness/src/lib.rs:102-114`, `harness_tui.rs:153`), `ApprovalPort`, `ToolPort`, `LoopSink`, `run_in` on a `std::thread`; `Embedded` and the sync `Backend` | Same model break. A non-`Send` port cannot go to `spawn_blocking`. `Backend` changes in a way the plan does not specify | no |
| agent-platform | agent-platform-harness, -http (+ agentide-harness tag 0.3.5) | rev 0f2edfef (0.11.1) | `harness_messages` (crate since removed), three `ToolPort`s calling `handle.block_on`, `LoopSink`, `ApprovalPort`, `run` and `resume_approval` inside `spawn_blocking` under axum | This is exactly the tokio embedder the goal describes. It needs an async `resume_approval`, which the plan does not provide | partly (run only) |
| metaharness | metaharness-tools | rev 330e2cde (0.18.0) | `Catalogue`, `Verbs`, `LocalOperations`, `Operations` re-export (`lib.rs:60`), sync `self.verbs.call` (`server.rs:258`) | The sync `ToolPort` on `Verbs` is deleted and nothing replaces it. The sync MCP server would have to build its own runtime | no; missing from the epic's consumer census |
| metaharness | metaharness-b10x | rev 330e2cde | only `micro_usd_as_decimal` and `entry_names` | unaffected | n/a |
| loom, takt | through metaharness | 330e2cde / f0870131 | none directly | follows metaharness | n/a |
| control-plane | none | — | its `ModelPort`/`ToolPort` come from loom 0.20.0's own `loom::harness`; its `Cargo.lock` has no `b10x-harness` | unaffected; the epic counts it wrongly | — |
| atlas | integration/harness-plugins | `b10x-harness-cli` on branch main | `agents::agents_in`, `skills::skill_at` | compiles against main; parser API only, unaffected | n/a |
| platform | runtime/inference | branch main, `version = "0.1.0"` | `ModelPort`, `StreamSink`, `BearerSource`; also `harness-responses` (crate since removed) | already cannot resolve; stale since 2026-09-15 and not in the repo map | n/a |

**Findings**

1. epic, story:blocking-facade-keeps-sync-callers — A sync caller holding a real `LlmPort` has no path. The sync `LlmPort` impl and `b10x-llm-blocking` are deleted, `Blocking::new` only adapts sync to async, and the facade "wraps the synchronous ports". Worse, the Done-When grep `BlockingModel` also matches `BlockingModelPort`, so it forbids harness-llm from ever offering a sync impl. Evidence: contract acceptance, `uilab-agent/src/lib.rs:527,800-806`. **blocker**, verified text / inferred effect.
2. story:async-model-and-tool-ports vs story:loop-runs-as-a-future — `spawn_blocking` needs `Send + 'static`. The loop holds borrowed ports (`AgentLoop<'a>`, `harness-loop/src/lib.rs:1805-1808`), `ModelPort` has no `Send` bound (`port.rs:91`), and `turn` borrows its request and `&mut dyn StreamSink`. So the facade cannot reuse story 1's adapters, and D4 ("migrate by wrapping") is false for non-`Send` or borrowed ports. **major**, inferred.
3. story:loop-runs-as-a-future — The ports are passed to `AgentLoop::new`, `with_hooks` and `with_environment` (`lib.rs:2048-2098`), not to `run`. The acceptance says `run_async` "takes" the async ports. How one `AgentLoop` holds sync or async ports (a second type, generics, or a builder) is undesigned. **major**, verified.
4. story:loop-runs-as-a-future — The public `AgentLoop::call` (`lib.rs:2120`) and `resume_approval` (`lib.rs:2227`) get no async variant. agent-platform calls `resume_approval` from tokio (`agent-platform-harness/src/lib.rs:581`, `-http/src/lib.rs:885`), and the conformance target drives `.call` (`boundary.rs:162`). **major**, verified.
5. story:loop-runs-as-a-future — Removing `thread::scope` rewrites `parallel.rs`: `ChildSink` (:93), `ChildApprovals` (:111) and `ChildHooks` (:149) relay over mpsc to "the run's own thread". `parallel.rs` and `delegate.rs` are not in scope, and nothing says how concurrent child futures share the single `&mut` approver, hooks and sink. **major**, verified.
6. story:async-model-and-tool-ports — The `Send` design is missing. D2 makes every future `Send`, but the loop's sinks are deliberately not `Send` (`README.md:384`, `harness-llm/src/lib.rs:552-553`), while llm's `StreamSink: Send` (`llm-core/src/port.rs:60`). The plan states no `Send` supertraits. Also, the literal `+ Send + '_` cannot capture the request and sink lifetimes; llm uses an explicit `'a` (`port.rs:75-80`). **major**, inferred.
7. story:mcp-and-substrate-await-their-drivers — The public sync `Backend` trait (`backend.rs:21`) is unaddressed. Its implementors are `Embedded` (`embedded.rs:263`), `Client` over a std `UnixStream` (`client.rs:10,696`) and the test `Recording` (`tests/confined_spelling.rs:29`). `ConfinedOperations` holds `Box<dyn Backend + Send + Sync>` (`tools.rs:42`), and `Embedded::open_with` and `workspace_adopt` are public sync. The Done-When grep forces the runtime out, but nothing says `Backend` becomes async. agentide-substrate uses it. **major**, verified.
8. story:harness-llm-awaits-the-model — "The renewal" is the public `renew_codex_login` (`lib.rs:480`). It is called by `harness-cli/src/lib.rs:1601` and pinned by `tests/adversary_public_surface.rs:487`. Making it async breaks harness-cli in this story, which breaks D5's per-story green; neither file is in scope. **major**, verified.
9. epic, story:harness-llm-awaits-the-model — The "write at :1441" is a test keychain helper inside `#[cfg(test)] mod tests` (from `lib.rs:854`), not a production runtime. `LlmPort::turn`'s own `std::thread::scope` (`lib.rs:555`) is not mentioned. **minor**, verified.
10. story:catalogue-awaits-its-operations — Changing `Operations` in place breaks implementors outside this story's scope: `harness-catalogue/tests/toolchain_against_0_15_1.rs:272,332`, `search_against_0_15_1.rs:319`, `toolchain_adversary.rs:276`, `written_directory_adversary.rs:58`, and `harness-substrate/tests/conformance.rs:876,1493`. The workspace is not green after the story. **major**, verified.
11. story:binary-and-catalogue-on-one-runtime — The app server has no runtime: no tokio in its `Cargo.toml`, and a `thread::spawn` pump (`transport.rs:78`). "Awaits on the runtime it is served on" means rewriting `transport.rs`, which is not in scope. That rewrite risks the interrupt ordering pinned under `contracts/app-server-profile`, but the acceptance only diffs `contracts/cli`. **major**, verified / risk inferred.
12. story:binary-and-catalogue-on-one-runtime — `Renderer` (`harness-cli/src/render.rs:219`) and `BridgeSink` (`app-server/src/session.rs:253`) are not named. The stdin approver and the hook process waits would block a runtime worker, and nothing requires moving them off it. **major**, verified names / inferred stall.
13. epic — ESS is untouched. `spec/domains/loop.yaml:48`, `spec/README.md:23` and `spec/COVERAGE.md:10,11,26` cite `run_in` and line numbers in `lib.rs`. AGENTS.md § Executable specification says the spec is a retrofit, "not permission to invent new runtime semantics". The workspace rule requires named conformance scenarios as each story's acceptance. No story scopes `spec/` or `conformance/`. **major**, verified.
14. epic — Docs that describe threads or a synchronous loop: `AGENTS.md:249,264-265`, `README.md:311,332,384`, `website/docs/concepts/agent-loop.md:94-97`. Only README is in scope, inferred, with no acceptance. **minor**, verified.
15. epic D3 vs scope — D3 says tokio appears only in crates that already use it, but harness-loop has none today (`crates/harness-loop/Cargo.toml`) and receives the adapter and the facade runtime. The workspace tokio features are `rt, time, macros` (`Cargo.toml`); `rt-multi-thread`, needed by a facade that refuses current-thread runtimes and by `#[tokio::main]`, is missing. Story 2 still claims "no dependency changes". **minor**, verified.
16. epic — The consumer census is wrong in both directions. It counts control-plane, which is not a consumer. It omits metaharness-tools and the transitive loom and takt. **minor**, verified.
17. epic D5 — Expand/migrate/contract is over-built for this crate. Nothing is released in between (one 0.23.0), consumers sit 4 to 11 minors behind on rev or tag pins, and the workspace rule is one gate, one PR. The rename to `Blocking*` makes every sync consumer rename every impl, for no gain over adding the async traits under new names. Every implementor is written twice and then about 131 test call sites are renamed. **major**, inferred.
18. story:blocking-facade-keeps-sync-callers — The scope omits `harness-loop/src/tests.rs` (9 sync impls), `harness-catalogue/tests`, `harness-substrate/tests` and `harness-llm/tests`, yet the acceptance requires `cargo test --workspace`. **minor**, verified.
19. story:loop-runs-as-a-future — `BlockingLoop` appears only in the acceptance and is never defined. Its name collides with `Blocking::new`, `BlockingModelPort` and llm's `BlockingModel`. **minor**, verified.
20. story:loop-runs-as-a-future — How long the facade's runtime lives is unspecified. If each run builds one, an async client's connection pool, spawned on a runtime that has since been dropped, fails on the next run. It needs one runtime per facade or per process. **minor**, inferred.
21. epic, stories — Done When never runs the full `cargo xtask gate`: clippy `-D warnings`, strict rustdoc, `harness-docs generate --check` and the app-server profile check (`harness-xtask/src/main.rs:77-136`). **minor**, verified.
22. story:binary-and-catalogue-on-one-runtime, story:mcp-and-substrate-await-their-drivers — Citation slips: `approve.rs:25`, `caller.rs:53` and `hooks.rs:58` are `use` lines, not impls; `embedded.rs:462/480/491` are the `.runtime` lines and the `block_on` calls are at 463/481/492. All other citations checked out. **minor**, verified.

On whether a tokio program can embed the loop with its real ports: with the llm model, the catalogue and MCP, yes, once the stories land. Substrate confinement depends on finding 7, and deferred approvals and single gated calls are missing (finding 4).

I modified no files, ran no builds and did not write to the planning store.
