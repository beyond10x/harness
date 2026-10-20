---
format: aep.planning-md/3
id: story:harness-builds-on-llm-0-7
kind: story
status: implemented
title: Harness builds on llm 0.7.0 and its four wire and credential crates are gone
summary: The spike bridge (8989d138) is carried from llm 0.1.7 to 0.7.0; harness-messages, harness-responses, harness-http and harness-credential are deleted; compaction, retry, the ledger and approvals stay.
relations:
- decomposes: epic:harness-on-llm-and-tools
- serves: vision:b10x-owns-its-loop
- supersedes: story:oauth-token-renewal
- supersedes: story:responses-pin-from-live-bytes
- supersedes: story:messages-cache-breakpoint-live-pin
- supersedes: story:vllm-reasoning-events-unpinned
scope:
- confidence: cited
  path: .github/workflows/gate.yml
- confidence: cited
  path: AGENTS.md
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: Cargo.lock
- confidence: cited
  path: Cargo.toml
- confidence: cited
  path: README.md
- confidence: cited
  path: ROADMAP.md
- confidence: cited
  path: STATUS.md
- confidence: cited
  path: crates/harness-app-server/Cargo.toml
- confidence: cited
  path: crates/harness-cli/Cargo.toml
- confidence: cited
  path: crates/harness-cli/src/lib.rs
- confidence: cited
  path: crates/harness-cli/src/llm_port.rs
- confidence: cited
  path: crates/harness-cli/src/provider.rs
- confidence: cited
  path: crates/harness-cli/tests
- confidence: cited
  path: crates/harness-cli/tests/workflow.rs
- confidence: cited
  path: crates/harness-credential
- confidence: cited
  path: crates/harness-http
- confidence: cited
  path: crates/harness-messages
- confidence: cited
  path: crates/harness-responses
- confidence: inferred
  path: crates/harness-wire/src/id.rs
- confidence: cited
  path: crates/harness-xtask/src/main.rs
- confidence: cited
  path: measurements
- confidence: cited
  path: spec/COVERAGE.md
- confidence: inferred
  path: website
revision: 37
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T01:57:56Z", actor: "human:timo", revision: 27, decided_on: {"recorded":{"review_outcome":5}}}
- {from: "proposed", to: "active", at: "2026-10-20T01:57:56Z", actor: "human:timo", revision: 28, decided_on: {"recorded":{"review_outcome":5}}}
- {from: "active", to: "implemented", at: "2026-10-20T05:39:37Z", actor: "human:timo", revision: 37, decided_on: {"recorded":{"test_result":1,"review_outcome":12,"verification":1}}}
---
## Context

Atlas ADR 0097 decision 2: "harness builds on llm. The four wire and credential crates go; the
bridge from the spike branch is rebased onto llm 0.7.0 (Rust 1.98, as llm requires). Compaction,
mid-stream retry, the run ledger and approvals stay in harness: they are loop features, and llm is
'not an agent loop'."

The spike is `spike/builds-on-llm-0.1.7` at `8989d138`, one commit whose parent is `origin/main`
`3169042f`, so the git rebase is a no-op and the work is the pin and the bridge. What the spike did
(`git show --stat 8989d138`: 42 files, +1,001 / -10,066):

- deleted `crates/harness-credential`, `crates/harness-http`, `crates/harness-messages`,
  `crates/harness-responses` and their workspace entries (`Cargo.toml`);
- added `crates/harness-cli/src/llm_port.rs` (701 lines): flags, credential sources, the binding,
  Codex renewal, the blocking model port, request, event, outcome, usage and error translation;
- `crates/harness-cli` depends on seven llm crates by tag `0.1.7`: `b10x-llm-blocking`, `-core`,
  `-credentials` (features `codex-renewal`, `environment`, `file`, `json-pointer`), `-http`,
  `-messages`, `-providers`, `-responses`;
- moved `tests/fixtures/fake_messages.py` and `fake_responses.py` into `crates/harness-cli/tests/`;
- removed `check_http_boundary` from `crates/harness-xtask/src/main.rs` (the fenced crate is gone);
- `rust-version` `1.97` to `1.98` (`Cargo.toml`), MSRV job to `1.98.0` (`.github/workflows/gate.yml`);
- three tests `#[ignore]`d for behaviours llm's parity table did not list:
  `crates/harness-cli/tests/argv_pin_consumer.rs:572`, `crates/harness-cli/tests/end_to_end.rs:638`
  and `:1845`;
- result: `cargo test --workspace` 928 passed, 4 ignored (1,134 on the base; 206 left with the
  deleted crates); fmt, clippy and the xtask gates exit 0 (commit message).

What changed in llm between 0.1.7 and 0.7.0 that the bridge uses (read at tag `0.7.0`; llm's
`story:harness-cutover-on-llm-0-7`, wave 2026-10-20-w09, is re-citing the full parity table and
is not waited for):

| llm item | 0.7.0 | bridge site on the spike |
|---|---|---|
| `llm_core::TurnRequest` | gains `pub controls: Controls` (`crates/llm-core/src/turn.rs:88-90`); `Controls::default()` "is the 0.6.0 turn exactly" | struct literal `llm_port.rs:506-524`: missing field, does not compile |
| `llm_core::AuthKind` | gains `SubscriptionOauth` (`crates/llm-core/src/vocabulary.rs:17-20`) | the bridge's refusal of subscription auth (`llm_port.rs:266-273`, test `:685-690`) is obsolete |
| `llm_core::controls::wire_effort` | maps effort `max` to `xhigh` on Responses (`crates/llm-core/src/controls.rs:245-252`); 0.1.7 refused it | `REASONING_EFFORTS` at `llm_port.rs:44` lists no `max` |
| `llm_core::Capabilities` | 7 fields (`vocabulary.rs:50-58`) and `Capabilities::text(context_window, max_output_tokens)` (`:62`) | full literal at `llm_port.rs:286-296` |
| `rust-version` | `1.98` (`Cargo.toml:24`) | `1.98` already on the spike |

llm 0.8.0 is not tagged (`git tag -l '0.8*'` empty on llm at `5657d2f5`, 2026-10-20). Session
`adr-changes` relayed llm's plan on 2026-10-20: 0.8.0 "changes Capabilities (two new fields; your
spike's full literal at harness-cli/src/llm_port.rs:286-296 breaks; use ..Capabilities::text(…))
and guarantees that streamed text equals the outcome's text. Item::ToolResult stays as in 0.7.0."

## Outcome

`origin/main` builds and passes its gate with llm's client crates doing every model call, and the
four wire and credential crates no longer exist in the tree.

## Acceptance

`cargo xtask gate` exits 0 on a tree whose workspace members no longer include
`crates/harness-credential`, `crates/harness-http`, `crates/harness-messages` or
`crates/harness-responses`, whose `[workspace.dependencies]` pin the seven llm crates by the one
tag § Pin names, and where `grep -rn '#\[ignore' crates/harness-cli/tests` prints nothing (the
base `3169042f` has none there; the spike added three).

## Pin

`tag = "0.8.0"` (llm tag on `7873aa63`, "release: LLM 0.8.0 (#40)", 2026-10-20 05:15 UTC). The
build and the two adversary passes were made on `0.7.0`; the merge to `main` is on `0.8.0`. This
section is what the acceptance reads. adr-changes, 2026-10-20 (second decision, after llm's wave
w09 adversary): llm 0.7.0 makes Responses `server_error` and `rate_limit_exceeded` and Messages
`overloaded_error` and `api_error` final where harness retried them (llm
`story:retry-classes-harness-retried`, parity rows R28/M27); through the bridge's `may_retry`
(`llm_port.rs:622`) those retries stop. llm 0.8.0 marks all four retriable before output. So:
"build and review on 0.7.0 as planned, but merge to main only on llm 0.8.0 with a test that each
of the four classes before output is retried by Harness's loop. Until 0.8.0 is tagged, the unit
waits on the wave branch after its gate." When llm tags `0.8.0`, the coordinator rewrites this
line, the unit moves the pin and adds that test in one correction round, and the gate runs again.

Third decision (adr-changes, 2026-10-20, after the adversary's pass 1): the merge condition is
"0.8.0, retry cases green, absent-counter case green". Settled after two corrections the same
night: llm keeps "an absent usage counter is unknown, not 0" and takes no Messages usage change
into 0.8.0 (its story archived; llm puts `cache_control` on nearly every request, `codec.rs:163`,
`:88-104`). One case with `cache_creation_input_tokens` absent from the Messages usage object
expects usage unknown and the run saying so (llm parity M25); green on 0.7.0 and after. The
emulator's default matches the real API (the counter present, 0) and the three usage assertions
the spike had rewritten are restored: that is the M25 fix.
Unit commits allowed on `impl/harness-builds-on-llm-0-7`: up to six (spike, unit, corrections,
the 0.8.0 move).

## What the unit decides, and how

1. **Capabilities.** Write the `Capabilities` literal as
   `Capabilities { tools: true, …, ..Capabilities::text(context_window, max_output_tokens) }`, so the
   two fields 0.8.0 adds do not break the build; on 0.7.0 that spelling compiles as well.
2. **`TurnRequest.controls`.** `Controls::default()`; harness has no thinking, parallel-tool-call
   or hint control to map (`harness_wire::TurnRequest` carries none).
3. **`SubscriptionOauth`.** Remove the bridge's refusal and its test; a `--oauth-token-*`
   credential is presented as llm's `AuthKind::SubscriptionOauth` on the Messages protocol. The
   Responses protocol keeps whatever llm 0.7.0 does for a subscription token; the unit reads
   `llm-providers` and states which in `CHANGELOG.md`.
4. **`max` effort.** Add `max` to `REASONING_EFFORTS` only if `harness-cli` accepts it on the
   command line today (`--reasoning-effort`); otherwise leave the list and say so in the report.
5. **The three ignored tests.** Each is re-run on 0.7.0 with the attribute removed. One that passes
   loses the attribute. One that still fails is rewritten to assert llm's behaviour with a message
   naming the llm parity row and this story, per the wave rule that no case is left `#[ignore]`.
   The parity rows are R35 and R45 (partial, "llm is stricter": ADR 0097 § Context).
6. **Contracts (invariants 13 and 14).** `contracts/provider-wires/` stays byte for byte: released
   versions are immutable. `cargo xtask provider-contracts` (`crates/harness-xtask/src/provider_contract.rs:34`)
   keeps checking the manifests against their fixtures. The Rust half (the deleted
   `tests/contract.rs` of each wire crate) is gone and is now llm's: AGENTS.md invariant 14 says
   so for provider wires, and no new provider-wire version is cut here.
7. **Documents.** Every line that names a deleted crate as present is rewritten or removed:
   `AGENTS.md:50` (invariant 3, the `harness-http` paragraph) and invariants 13-14's provider-wire
   sentences; `README.md:76,435,457-458,467-470,500-501`; `ROADMAP.md:91,99`; `STATUS.md:17`;
   `spec/COVERAGE.md:35-38`; the two comments in `crates/harness-wire/src/id.rs:41,121`.
   `CHANGELOG.md` `[Unreleased]` gets a `Removed` entry for the four crates and a `Changed` entry
   for the llm dependency and `rust-version`. `ROADMAP.md:182` and `STATUS.md:30` describe a
   consumer's pin and stay.
8. **Gate.** `cargo xtask gate` on the unit's tree; the spike's commit message reports the
   per-step results and this unit reports them again at the pinned tag.
9. **Retry classes (on 0.8.0).** One test per class, through the shipped binary or
   `llm_port::client` against the fake servers in `crates/harness-cli/tests/fixtures/`: a
   Responses `server_error` and a `rate_limit_exceeded`, and a Messages `overloaded_error` and an
   `api_error`, each arriving before any output, are retried by the loop and the turn completes on
   the retry. Red on 0.7.0 (the class is final there), green on 0.8.0. The test names llm's story
   `retry-classes-harness-retried` and parity rows R28/M27.

## Stories whose surface this removes

Four `proposed` stories name the deleted crates as their surface. llm owns that surface after this
story, and llm's parity table (`llm/docs/harness-parity.md`, 148 rows: 139 covered, 2 partial, 0
gap, 7 not needed) is where their outcomes are tracked. This story carries a `supersedes` edge to
each (recorded 2026-10-20 after the design critic's round-1 finding). The wave proposal asks for
them to move `proposed -> rejected` with this story named as the reason; this story does not move
them.

| story | acceptance names |
|---|---|
| `story:responses-pin-from-live-bytes` | `contracts/provider-wires/openai-responses/<date>/`, `cargo test -p b10x-harness-responses --test contract` |
| `story:messages-cache-breakpoint-live-pin` | `contracts/provider-wires/anthropic-messages/<date>/`, "both halves of the contract check" |
| `story:oauth-token-renewal` | a run outlives its subscription token; renewal is `llm-credentials` `codex-renewal` now (`llm_port::renew_codex_login`) |
| `story:vllm-reasoning-events-unpinned` | the Responses stream decoder's admitted event names, a new provider-wire contract version |

## Consumers

ADR 0097: "harness's six consumers keep their pins; a consumer moves to the new tags in its own
wave." Read on 2026-10-20 from sibling checkouts, not verified from here:

| consumer | pins | crates among the four deleted |
|---|---|---|
| agentide (`Cargo.toml`) | git | `b10x-harness-credential`, `-messages`, `-responses` |
| agent-platform (`Cargo.toml`) | tag `0.10.0` | `b10x-harness-messages` |
| uilab (`crates/uilab-agent/Cargo.toml:18-21`) | rev `798325f0` | `b10x-harness-messages`, `-credential` |
| metaharness (`crates/metaharness-b10x/Cargo.toml:19,23`, `crates/metaharness-tools/Cargo.toml:24-25`) | rev `798325f0` | none |
| atlas `integration/harness-plugins/Cargo.toml` | git | `b10x-harness-cli` only |
| platform `runtime/inference/Cargo.toml:29-30` | **`branch = "main"`** | `b10x-harness-responses` |

The last row is not a pin: the merge of this story to `main` breaks that crate's next
`cargo update`. ADR 0100 moves inference onto llm; the wave proposal names this and the session
`inference` is told.

## Out of scope

- Any change to `crates/harness-loop`, `crates/harness-wire`'s types, compaction, mid-stream retry,
  the run ledger or approvals (ADR 0097 decision 2, second sentence).
- A new provider-wire contract version.
- Moving a consumer's pin.
- A release tag: `AGENTS.md` § Releases, after the wave closes.

## Scope

Derived 2026-10-20 by the coordinator from `git show --stat 8989d138` and `git grep` at `origin/main`
`3169042f`; no scoper agent was dispatched because the spike commit lists every file. Every line is
**cited** (read from a command) or **inferred**. **Confirmed after implementation** (wave llm-1,
six unit commits, `git diff --stat 22ddc1df..abbfc96c`): every cited line held; the two inferred
lines resolved as: `crates/harness-wire/src/id.rs` — changed (two comments reworded to name
`llm-responses`); `website/` — untouched (`git grep` for the four crate names finds nothing there).
Touched beyond the derived list, now cited: `measurements/dry-run-compaction.sh`,
`measurements/dry-run-surfaces.sh`, `measurements/prove-spend-ceiling.sh`,
`measurements/rehearse-paid-command.sh` (the moved fixture path; the token file mode),
`crates/harness-cli/tests/workflow.rs` (a usage assertion restored), and the two emulators
`crates/harness-cli/tests/fixtures/fake_messages.py` and `fake_responses.py` (scenarios added for
the adversary's cases). Not touched, as listed: `contracts/provider-wires/`, `crates/harness-loop`,
`harness-wire`'s types.

- **Primary surface:** `crates/harness-cli`: `Cargo.toml`, `src/lib.rs` (158 lines changed),
  `src/llm_port.rs` (new, 701 lines), `src/provider.rs` (doc comment), and six test files under
  `tests/` plus `tests/fixtures/fake_messages.py` and `fake_responses.py` moved in from the wire
  crates — cited
- **Deleted:** `crates/harness-credential`, `crates/harness-http`, `crates/harness-messages`,
  `crates/harness-responses` — cited
- **Workspace:** `Cargo.toml` (members, `rust-version`, `[workspace.dependencies]`), `Cargo.lock`,
  `.github/workflows/gate.yml` (MSRV job) — cited
- **Also:** `crates/harness-app-server/Cargo.toml` (one dev-dependency), `crates/harness-xtask/src/main.rs`
  (`check_http_boundary` and `files_under` removed) — cited
- **Documents:** `AGENTS.md:50` and invariants 13-14, `README.md:76,435,457-458,467-470,500-501`,
  `ROADMAP.md:91,99`, `STATUS.md:17`, `spec/COVERAGE.md:35-38`, `CHANGELOG.md` `[Unreleased]` — cited
  (`git grep -n` for the four crate names)
- **Also likely:** `crates/harness-wire/src/id.rs:41,121` (two comments naming `harness-responses`) — cited
  lines, whether to reword them is inferred
- **Also likely:** `website/` — inferred; the spike touched nothing there, and `cargo xtask website-contract`
  decides at gate time
- **Size:** 42 files on the spike (+1,001 / -10,066) plus 6 documents — cited
- **Confidence:** high — the spike is the change, compiled and gated once already on 0.1.7 — cited
- **Would collide with:** `story:harness-consumes-tools-crates` on the paths
  `aep plan artifact waves --kind story --status draft` reports (ten after that story's round-2
  scope, plus `ROADMAP.md` once its entry is typed): `Cargo.toml` and `crates/harness-cli/Cargo.toml`
  (cited on both sides); `AGENTS.md`, `CHANGELOG.md`, `Cargo.lock`, `README.md`, `ROADMAP.md`,
  `STATUS.md`, `crates/harness-cli/src/lib.rs`, `crates/harness-xtask/src/main.rs`,
  `spec/COVERAGE.md` (cited here, inferred there, because that story's entries are inferred until
  `tools` releases). That story carries `depends_on: story:harness-builds-on-llm-0-7` (recorded
  2026-10-20), so it runs after this one and never beside it — cited
- **Would collide with:** `story:oauth-token-renewal` on `crates/harness-credential` (the verb at
  `--status proposed`); superseded by this story — cited
- **Safety fact:** `contracts/provider-wires/` is not touched (invariant 13); the approval gate, the
  shipped read-only toolset and `harness-loop` are not touched (ADR 0097 decision 2) — cited
