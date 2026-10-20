---
format: aep.planning-md/3
id: story:llm-port-is-a-public-crate
kind: story
status: active
title: The llm adapter is a public crate consumers can import
summary: Move mod llm_port (907 lines) unchanged into b10x-harness-llm; harness-cli depends on it; a short wave after tools-1, released in 0.15.0.
relations:
- decomposes: epic:harness-on-llm-and-tools
- serves: vision:b10x-owns-its-loop
- depends_on: story:harness-consumes-tools-crates
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T09:11:50Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-10-20T09:11:50Z", actor: "human:timo", revision: 6}
---
## Context

harness's consumers (agentide, agent-platform, uilab, platform `runtime/inference`) import
`b10x-harness-credential`, `b10x-harness-messages` and `b10x-harness-responses`, which
`story:harness-builds-on-llm-0-7` deleted (spike `8989d138`). Their replacement, the llm adapter,
is `crates/harness-cli/src/llm_port.rs` (907 lines, private `mod llm_port`, `lib.rs:14`), so a
consumer cannot reach it without copying it. agentide's move off the deleted crates is a security
fix (Substrate 0.5.0), so this must not wait on anything beyond tools-1.

Decided by session `adr-changes`, 2026-10-20, under the owner's exception 5: move `llm_port`
unchanged into a public library crate `b10x-harness-llm` that harness-cli uses, with its tests;
release it in 0.15.0. Feasibility checked: the module's only crate-internal reference is
`use super::*` inside its own `mod tests` (`llm_port.rs:701`), so it has no harness-cli-private
dependency and moves unchanged.

## Outcome

`b10x-harness-llm` is a workspace crate holding the former `mod llm_port` and its tests;
`harness-cli` depends on it rather than embedding it; a consumer can build its own `ModelPort`
over llm's client crates from this crate's public surface alone.

## Acceptance

`cargo xtask gate` exits 0 on a tree where `crates/harness-cli/src/llm_port.rs` is gone,
`b10x-harness-llm` is a workspace member whose public API is exactly the set § Public surface
names and no wider, `harness-cli` names it as a dependency, the CLI argv contract
(`contracts/cli/b10x-harness/2026-09-18/`) is byte-identical (invariant 13), and one test shows a
`SecretResolver` passed to `Source::resolver` is asked exactly once per model request (the
per-request lease agent-platform needs).

## Public surface (checked against three consumers, nothing wider)

Fixed against the call sites each consumer has today, off the deleted crates. Most of this is a
move: `Source::{key,file,environment}` (`llm_port.rs:91,105,121`), `Target` (`:189`) and `client`
(`:202`, `cancel: &Cancel`) are already `pub` in the module and become public with it. One
constructor and one re-export are added, decided in this story.

- **agentide** (`crates/agentide-harness/src/lib.rs:17-20,104-126,174`): `NamedSource`,
  `SubscriptionToken`, `MessagesClient`/`ResponsesClient` + `Endpoint`, `with_cancel`, building
  `Box<dyn ModelPort>` → `Source::{file}`, `client(&Target, Option<Source>, &Cancel) -> Box<dyn ModelPort>`.
- **platform `runtime/inference`** (`provider.rs:9`): `client(&Target, Option<Source>, &Cancel)`,
  `Target`, `Source::key` — all already public, confirmed.
- **agent-platform** (`crates/agent-platform-harness/src/lib.rs:522-529,683-710`, a per-attempt
  OAuth lease redeemed once per request): **add** `Source::resolver(name, Arc<dyn llm_credentials::SecretResolver>, Presentation)`
  (new constructor; `key`/`file`/`environment` do not fit — x-api-key, tenant credential on disk or
  in env), and **`pub use llm_credentials;`** so a caller names the `SecretResolver` trait and its
  value types from the same llm copy harness links.

- **agentide, second need:** re-export the `Protocol` type `Target.protocol` uses (`llm_core::Protocol`)
  as `pub use llm_core::Protocol;` (or re-export `llm_core`), so a consumer names it without pinning
  llm-core itself. Test: `Source::resolver` + an `EnvironmentResolver` + `Presentation::ApiKey`
  re-reads the variable on each request.
- **uilab:** it holds the client across `spawn_blocking` (`crates/uilab-agent/src/lib.rs:506`,
  `uilab-app/src/app.rs:974`), so `client()` must return `Box<dyn ModelPort + Send>`
  (`harness_wire::ModelPort` has no `Send` bound, `harness-wire/src/port.rs:91`); harness-cli's own
  callers still coerce to `Box<dyn ModelPort>`. Add a compile-time assertion that `LlmPort: Send`.

So the crate's public surface is: `Source` with `key`, `file`, `environment` and `resolver`;
`Presentation`; `Target`; `client` (returns `Box<dyn ModelPort + Send>`); `LlmPort` (implements
`harness_wire::ModelPort`, asserted `Send`); `RESPONSES_WIRE`, `MESSAGES_WIRE`; `renew_codex_login`;
`pub use llm_credentials;`; `pub use llm_core::Protocol;`; `pub use llm_core::BoxFuture;`
(agent-platform's `SecretResolver::resolve` returns it, so a consumer names it rather than expanding
the alias). Plus whatever `harness-cli` already names. Nothing else becomes public.
`harness_wire::{ModelPort, Cancel}` stay harness-wire's (consumers already name them there).

## Scope

- New crate `crates/harness-llm` (package `b10x-harness-llm`), holding `llm_port.rs` moved
  unchanged (module → crate root), its `mod tests`, and the two fake-server fixtures if the tests
  need them (`crates/harness-cli/tests/fixtures/fake_{messages,responses}.py` are integration-test
  fixtures, not unit-test; the module's own `mod tests` is unit-level — confirm what moves).
- `Cargo.toml` workspace members + `[workspace.dependencies]` entry (the seven llm crates move to
  the new crate's deps); `Cargo.lock`.
- `crates/harness-cli/Cargo.toml` gains `harness-llm`; `crates/harness-cli/src/lib.rs:14`
  `mod llm_port;` → `use harness_llm as llm_port;` (or re-point the `llm_port::` paths); the call
  sites naming `llm_port::` in `lib.rs`.
- `AGENTS.md` invariant 2 foundations list; `README.md`/`STATUS.md` crate layout; `CHANGELOG.md`.
- **Not touched:** behaviour, argv, `contracts/`, `harness-loop`, `harness-wire` types.

## Out of scope

- A consumer's move onto the new crate (its own wave, at the 0.15.0 tag).
- Widening the surface beyond what a consumer that had the deleted crates needs.

## Sequencing

`depends_on: story:harness-consumes-tools-crates` — both edit `Cargo.toml`,
`crates/harness-cli/Cargo.toml` and `lib.rs`, so this runs after tools-1 merges, on a fresh branch
from that merge. A short wave; released in 0.15.0 with the tools work.
