---
format: aep.planning-md/3
id: story:harness-consumes-tools-crates
kind: story
status: implemented
title: Harness consumes the tools repository's crates instead of harness-tools and harness-toolchain
summary: 'ADR 0095 phase T2 for harness: the tool contracts and default implementations come from tools crates pinned by tag; waits for tools'' first release.'
relations:
- decomposes: epic:harness-on-llm-and-tools
- serves: vision:b10x-owns-its-loop
- depends_on: story:harness-builds-on-llm-0-7
scope:
- confidence: inferred
  path: AGENTS.md
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: Cargo.lock
- confidence: cited
  path: Cargo.toml
- confidence: inferred
  path: README.md
- confidence: inferred
  path: ROADMAP.md
- confidence: inferred
  path: STATUS.md
- confidence: cited
  path: crates/harness-cli/Cargo.toml
- confidence: inferred
  path: crates/harness-cli/src/approve.rs
- confidence: inferred
  path: crates/harness-cli/src/lib.rs
- confidence: inferred
  path: crates/harness-cli/src/metaharness.rs
- confidence: inferred
  path: crates/harness-cli/src/workflow.rs
- confidence: cited
  path: crates/harness-substrate/Cargo.toml
- confidence: inferred
  path: crates/harness-substrate/src/embedded.rs
- confidence: inferred
  path: crates/harness-substrate/src/tests.rs
- confidence: inferred
  path: crates/harness-substrate/src/toolchain.rs
- confidence: inferred
  path: crates/harness-substrate/src/tools.rs
- confidence: inferred
  path: crates/harness-substrate/tests/conformance.rs
- confidence: inferred
  path: crates/harness-substrate/tests/embedded_live.rs
- confidence: inferred
  path: crates/harness-substrate/tests/live.rs
- confidence: cited
  path: crates/harness-toolchain
- confidence: cited
  path: crates/harness-tools
- confidence: inferred
  path: crates/harness-xtask/Cargo.toml
- confidence: inferred
  path: crates/harness-xtask/src/main.rs
- confidence: inferred
  path: spec/COVERAGE.md
revision: 42
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T07:26:34Z", actor: "human:timo", revision: 40, decided_on: {"recorded":{"review_outcome":7}}}
- {from: "proposed", to: "active", at: "2026-10-20T07:26:34Z", actor: "human:timo", revision: 41, decided_on: {"recorded":{"review_outcome":7}}}
- {from: "active", to: "implemented", at: "2026-10-20T08:43:46Z", actor: "human:timo", revision: 42, decided_on: {"recorded":{"test_result":1,"review_outcome":8}}}
---
## Context

Atlas ADR 0097 decision 3: "harness's tools move into `tools`. `harness-tools` and
`harness-toolchain` become `tools` crates under the `tool-contract/1` profile (ADR 0095 phases T1
and T2), and harness consumes them." ADR 0095's phase table (`tools/docs/design/tool-contracts.md:98-102`):
T1 is "the `tool-contract/1` profile and its validator; `fs` and `code-editor` contracts with
suites; `crates/tool-fs`, `crates/tool-code-editor` passing them"; T2 is "harness, metaharness and
takt adopt T1". The design names harness as the first adopter (`tool-contracts.md:92`: "First
adopters: harness (fs and code-editor from this repository)").

Where the tool layer lives today, in this repository:

| crate | lines (ADR 0097 § Context) | consumed by |
|---|---|---|
| `crates/harness-tools` | 6,049 | `crates/harness-cli/Cargo.toml:28`, `crates/harness-substrate/Cargo.toml:16,42`; calls at `crates/harness-cli/src/lib.rs:115,122,207` |
| `crates/harness-toolchain` | 1,952 | `crates/harness-cli/Cargo.toml:27`, `crates/harness-substrate/Cargo.toml:15`, `crates/harness-xtask/Cargo.toml:13`; calls at `crates/harness-xtask/src/main.rs:195-220` |

Outside this repository, metaharness pins `b10x-harness-tools` at rev `798325f0`
(`metaharness/crates/metaharness-b10x/Cargo.toml:19`, `metaharness/crates/metaharness-tools/Cargo.toml:24`).

The `tools` repository (main `dabd034`, 2026-10-20) holds no crate and no tag: its `Cargo.toml`
lists no members and `git tag` prints nothing. Its store has three draft stories
(`story:tool-contract-profile`, `story:toolset-fs`, `story:toolset-code-editor`). Session `tools`
is moving `harness-tools` and `harness-toolchain` there now. Until a tag exists this story cannot
name what it pins; `dependency-blocker:tools-has-no-release` holds it.

## The seam

`harness-tools` is written against `harness-wire`'s tool vocabulary: `ToolPort`, `ToolSpec`,
`Envelope`, `Risk`, `ToolOutcome` (`crates/harness-tools/Cargo.toml:17`,
`crates/harness-tools/src/catalogue.rs:19`, `crates/harness-wire/src/port.rs:135`). That vocabulary
stays here: `harness-wire` is the loop's port layer (AGENTS.md invariant 3) and
`story:harness-builds-on-llm-0-7` does not touch it. A `tools` crate must not depend on
`harness-wire`, because `tools` serves every harness (ADR 0095) and a dependency on one harness's
port types would couple it to that harness.

`harness-tools` also depends on `harness-toolchain` (`crates/harness-tools/Cargo.toml:16`) and
re-exports its `ResolvedProvider`, `Registry` and `CommandPlan` (`crates/harness-tools/src/lib.rs:58`,
`crates/harness-tools/src/catalogue.rs:75`). So `harness-toolchain` moves with `harness-tools`, as
ADR 0097 decision 3 says ("`harness-tools` and `harness-toolchain` become `tools` crates"): a
`tools` crate that still took the toolchain types from here would depend on a harness crate, the
coupling the previous paragraph forbids. Nothing in this repository keeps a copy of those types.

So the adapter is this story's and lives in this repository: a mapping from a `tools` crate's
contract (its operations and the `tool-contract/1` properties: effects, risk, idempotency, bounds,
required binaries, the paths a call touches) onto `harness_wire::ToolSpec` and `Envelope`, and
from its operation calls onto `ToolPort::invoke`. It goes into `crates/harness-cli` unless
`crates/harness-substrate` needs the same mapping, in which case it is a crate of its own
(§ Open). `harness-wire` gains no dependency and no new type for it.

## Outcome

`harness-cli`, `harness-substrate` and `harness-xtask` take their tool contracts and default
implementations from `tools` crates pinned by tag, through an adapter owned here, and neither
`crates/harness-tools` nor `crates/harness-toolchain` exists in this tree any more.

## Acceptance

`cargo xtask gate` exits 0, its `cargo xtask cli-contract` step included, on a tree whose
workspace members no longer include `crates/harness-tools` or `crates/harness-toolchain`, whose
`[workspace.dependencies]` pin `b10x-tool-fs`, `b10x-tool-code-editor`, `b10x-tool-toolchain` and
`b10x-tool-contract` at `tools` tag `0.1.0` (or a later tag § Open names), whose CLI
contract version in force (`contracts/cli/b10x-harness/2026-09-18/manifest.json`) still lists
every tool name the binary publishes, unchanged, with the adapter's name-to-operation mapping
table tested in both directions, and which carries one new test,
beside the adapter, that builds the adapter's `ToolSpec` for `run` and asserts that `AgentLoop`
under `DenyAll` returns a failed `ToolOutcome` for it (the gate `AGENTS.md` § Safety envelope
describes, exercised through the new adapter rather than through `harness-tools`).

## Open until `tools` releases — answered 2026-10-20

`tools` released `0.1.0` on 2026-10-20 05:13 UTC: annotated tag on `046dfb14c36773b557d76de0de92e04ce23a388d`
(`git rev-parse '0.1.0^{}'` in a fetched checkout; GitHub release page
`https://github.com/beyond10x/tools/releases/tag/0.1.0`, no assets). Its `CHANGELOG.md` `[0.1.0]`:
"The first release: ADR 0095 phase T1, seeded from harness 0.13.3 under ADR 0097. harness and
metaharness pin this tag." Workspace `rust-version = "1.98"`. Four crates (`crates/*/Cargo.toml` at
the tag):

| crate | what | licence |
|---|---|---|
| `b10x-tool-contract` | the `tool-contract/1` profile: parser, validator, `tool-conformance/1` runner, schema shape check, `tool-contract validate` binary | Apache-2.0 |
| `b10x-tool-fs` | `fs` v1alpha1: `fs.read`, `fs.list`, `fs.glob`, `fs.stat` over one workspace root; "harness 0.13.3's local reader moved here with a whole-file digest on reads" | LicenseRef-B10x-Proprietary |
| `b10x-tool-code-editor` | `code-editor` v1alpha1: `code.write`, `code.edit`, `code.patch`, `code.move`, `code.delete`; "harness 0.13.3's local writer moved here with `expected_sha256` preconditions" | LicenseRef-B10x-Proprietary |
| `b10x-tool-toolchain` | "harness 0.13.3's declarative toolchain registry moved here, deriving `build-test` operations under the profile" | LicenseRef-B10x-Proprietary |

- **The tag and the crates.** `tag = "0.1.0"`. `crates/harness-tools` is replaced by
  `b10x-tool-fs` and `b10x-tool-code-editor` (plus `b10x-tool-contract` for the profile types);
  `crates/harness-toolchain` by `b10x-tool-toolchain`. Both move, as § The seam requires.
- **The toolchain gate steps.** `cargo xtask toolchain-specs` and `cargo xtask toolchain-docs`
  (`crates/harness-xtask/src/main.rs:195-220`) check `crates/harness-toolchain`'s specs today; after
  the move they check `b10x-tool-toolchain`'s through its API or leave the gate with the crate.
  Decided at implementation by what `b10x-tool-toolchain` 0.1.0 exposes; `AGENTS.md` § The gate
  says which.
- **Tool names.** They change: `tools` names its operations `fs.read`, `fs.list`, `fs.glob`,
  `fs.stat`, `code.write`, `code.edit`, `code.patch`, `code.move`, `code.delete`; the CLI contract
  in force (`contracts/cli/b10x-harness/2026-09-18/manifest.json`) pins harness's own names
  (`run` among them, `manifest.json:26`). **Decided** (adr-changes, 2026-10-20): this story keeps
  harness's CLI contract names toward the model and toward metaharness, with a mapping table from
  each harness name to its `tools` contract operation id (`file_read` → `fs.read`, `file_edit` →
  `code.edit`, …) in the adapter, tested in both directions. A rename changes what the model sees
  and metaharness's surface, so it is a later wave with its own CLI contract version (invariant
  13), measured by a takt trial first (Atlas ADR 0101). The CLI contract `2026-09-18` therefore
  stays in force for this story. The `tools` `fs` and `code-editor` toolsets carry no `run`: the
  shell tool (`run`, `Risk::High`) stays harness's until `tools` publishes a process toolset, and
  the story says so in `STATUS.md`.
- **Read-only toolset.** `b10x-tool-code-editor` writes. `AGENTS.md` § Safety envelope: "the
  shipped toolset is read-only. Adding a tool that writes or executes is its own change, with its
  own gate and its own entry in `STATUS.md`". Linking the crate is not publishing its tools: this
  story links `b10x-tool-fs` (and `b10x-tool-contract`) for the shipped read-only surface and
  `b10x-tool-toolchain`; `b10x-tool-code-editor` is linked only where `harness-tools`'s writer was
  already published behind the approval gate (`file_edit`, `file_write` at `Risk::Medium`), under
  the same gate, and nothing new is published.
- **Adapter location.** `crates/harness-cli` or a crate of its own: decided by whether
  `harness-substrate` needs the mapping (`crates/harness-substrate/Cargo.toml:16,42` consumes
  `harness-tools` today).
- **metaharness's pin.** metaharness's `b10x-harness-tools` pin is metaharness's plan (ADR 0097
  decision 3, "metaharness's own tool surface … follows them"); not this story's.

## Out of scope

- Writing the `tools` crates: the `tools` repository.
- Adding a tool that writes or executes (`AGENTS.md` § Safety envelope: its own change, its own
  gate, its own `STATUS.md` row).
- `harness-wire`'s types.

## Scope

Derived 2026-10-20 by the coordinator from `git grep` at `origin/main` `3169042f`. Every line is
**cited** (read from a command) or **inferred**; most are inferred because the replacement crates
do not exist yet.

- **Primary surface:** `crates/harness-tools` and `crates/harness-toolchain` (deleted), the adapter
  (new, location per § Open) — cited
- **Manifests:** `Cargo.toml`, `crates/harness-cli/Cargo.toml`, `crates/harness-substrate/Cargo.toml`,
  `crates/harness-xtask/Cargo.toml` (the `harness-tools` and `harness-toolchain` lines) — cited;
  `Cargo.lock` — inferred
- **Sources that call the crates** (`git grep -l 'harness_tools\|harness_toolchain' -- crates` at
  `3169042f`): `crates/harness-cli/src/{lib,approve,metaharness,workflow}.rs`,
  `crates/harness-xtask/src/main.rs:195-220`, `crates/harness-substrate/src/{embedded,tools,toolchain,tests}.rs`,
  `crates/harness-substrate/tests/{conformance,embedded_live,live}.rs`,
  `crates/harness-loop/src/tests.rs` — cited files; which of them change depends on whether the
  replacement keeps the names, so each is inferred as a change
- **Documents:** `README.md:229,259,474-475`, `ROADMAP.md:206-214` (`harness_tools::Operations`,
  `harness_tools::Verbs`), `spec/COVERAGE.md:44`, `AGENTS.md` (§ Safety envelope, "the shipped
  toolset"; § The gate, the toolchain steps), `STATUS.md`, `CHANGELOG.md` — cited lines, inferred
  as changes
- **Maybe:** `contracts/cli/b10x-harness/<new date>/` — inferred (§ Open, tool names)
- **Confidence:** low — the `tools` crates do not exist — cited
- **Would collide with:** `story:harness-builds-on-llm-0-7` on `Cargo.toml`, `crates/harness-cli/Cargo.toml`,
  `Cargo.lock`, `crates/harness-cli/src/lib.rs`, `crates/harness-xtask/src/main.rs`, `AGENTS.md`,
  `README.md`, `ROADMAP.md`, `STATUS.md`, `CHANGELOG.md`, `spec/COVERAGE.md`. This story carries
  `depends_on: story:harness-builds-on-llm-0-7` (recorded 2026-10-20) for that reason: it runs after
  that story has merged, never beside it — cited

## Scope refined by story-scoper (2026-10-20, medium confidence)

Settled, cited from the tree at harness `c50f4fd5` and tools tag `0.1.0` (`046dfb14`):

- **Adapter is a crate of its own, below `harness-substrate` and `harness-cli`.** `ConfinedOperations`
  implements `harness_tools::Operations` (`crates/harness-substrate/src/tools.rs:28,152`) and
  `harness-substrate` depends on nothing above it, so the `Operations` trait and the publication
  layer (`Catalogue`, `Flat`, `Verbs`, `Scope`) that move out of `crates/harness-tools` land in a
  new crate both depend on. Invariant 6 forbids `common`/`shared`/`utils`/`misc`/`helpers`; the
  name is the implementor's.
- **`crates/harness-toolchain` deletes outright**: `b10x-tool-toolchain` 0.1.0 is the same code plus
  a doc paragraph, `pub mod contract;` and a `plan` fallback; the five builtin YAMLs are
  byte-identical.
- **`crates/harness-tools` does not delete outright.** `b10x-tool-fs` and `b10x-tool-code-editor`
  replace the reader and writer halves of `local.rs`; `catalogue.rs`, `flat.rs`, `verbs.rs`,
  `scope.rs`, `toolchain.rs`, `operations.rs`, `tests.rs` and `local.rs`'s `grep`/`exec`/`drain`
  stay (tools has no `process` or `search` toolset in 0.1.0; `tool_fs` does not export `walk`).
- **Name mapping (the recorded operation, `Entry.operation`, is a third column and stays):**
  `file_read → fs.read`, `dir_list → fs.list` (the harness name is `dir_list`, not `directory_list`),
  `find → fs.glob`, `file_write → code.write`, `file_edit → code.edit`; `search` and `run` have no
  tools operation in 0.1.0 and stay local (`run` at `Risk::High`).
- **The CLI contract names no tool.** `contracts/cli/b10x-harness/2026-09-18/argv.json` matches none
  of the tool names (grep → 0); `manifest.json:26` `run` is the `run` subcommand. The acceptance's
  "contract lists every tool name" is met by argv staying byte-identical; the tool-name list lives
  in `catalogue.rs:931-940` and `README.md:238-246`. No new CLI contract version is cut.

Callers to re-point (`git grep`, cited): `crates/harness-cli/src/{lib,approve,metaharness,workflow}.rs`,
`crates/harness-xtask/src/main.rs:194-219` (`harness_toolchain::`),
`crates/harness-substrate/src/{embedded,tools,toolchain,tests}.rs`,
`crates/harness-substrate/tests/{conformance,embedded_live,live}.rs`.
`crates/harness-loop/src/tests.rs` is doc comments only, not a caller.

Not established by the scoper (the implementor decides and reports):

- The new crate's name and whether `verbs.rs`/`flat.rs` move verbatim or are rewritten.
- `tool_fs` adds `sha256` to every `fs.read` reply; `ConfinedOperations::file_read` answers none, so
  the substrate conformance suite's `agreed`/`all_agree` equality check breaks unless the adapter
  strips the field or the confined reply gains one. **Default: strip in the adapter** (the digest is
  a tools contract property harness does not publish yet); the implementor confirms against
  `crates/harness-substrate/tests/conformance.rs`.
- Behavioural drift between `local.rs` and the tools crates (overlapping-match counting, trailing
  `/`, `limit: 0`, bound in bytes): which of `harness-tools/src/tests.rs` (1,240 lines) and the 34
  conformance cases those changes break was not checked.
