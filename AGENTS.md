# AGENTS.md — harness

The contract for changing **this** repository. Organization naming, Rust implementation and
public privacy rules apply. Changing bytes another repository verifies requires a coordinated
migration and an ADR; historical brand exemptions do not authorize new public associations.

`README.md` orients a reader; `STATUS.md` says what is built and `ROADMAP.md` what is next. This
file says what must not break.

## Serves

The objectives of the collection this repository moves, by id from `atlas/ROADMAP.md` — the only
cross-repository roadmap, and the page that says what each id means and which evidence closes it:

- **O1 — governed reach.** A tool the machine will not admit is refused by name, an agent's `tools:` is intersected at publish and at call, and a run declares where it may write.
- **O3 — any harness, observed and compared.** This loop is the native arm metaharness observes and the eval compares against a vendor's.
- **O6 — self-improvement, built into all of it.** Every session is filed with what it cost, so a run can be read back and improved on.

A change here that moves none of these is a question for the operator, not a task.
`atlas/scripts/check-map.sh` fails a repository whose `AGENTS.md` names no objective.

## What this repository owns

The b10x agent loop: turn assembly, tool round trips, approvals, budgets. A harness that makes its
own model calls, through `llm`'s client crates, rather than driving someone else's loop.

## Invariants

Each is a claim that can be checked. Breaking one is a design change, not a refactor.

### Boundary

1. **No bridges.** This component drives no vendor binary, no subprocess harness and no vendor
   control protocol as a client. Driving a vendor's loop is `metaharness`.
2. **No dependency on any sibling that could embed this.** Something else embeds this; this
   embeds nothing above it — not `metaharness`, `llmgw`, `identity` or `eventlog`. A dependency
   there would quietly re-couple the components the split exists to separate. **The dependencies
   below it are foundations, pinned and never sibling paths**: `substrate-host` and
   `substrate-wire` for confinement, the `b10x-mcp-*` client crates for outbound MCP protocol,
   transport and local credential mechanics, and the `b10x-tool-*` crates — `b10x-tool-contract`,
   `b10x-tool-fs`, `b10x-tool-search`, `b10x-tool-code-editor`, `b10x-tool-process`,
   `b10x-tool-toolchain` — for the tool contracts and default implementations the catalogue adapts (atlas ADR 0097 decision 3, ADR 0095 phase T2), and
   the `llm-*` client crates for every model call, which only this repository's own
   `b10x-harness-llm` crate depends on directly (invariant 3).
   All are pinned by git tag or revision, never by a path into the sibling `tools` checkout. A path
   into a sibling checkout builds against whatever is checked out there, and `--locked` cannot lock
   it. Harness retains the authority in every case: the substrate crossing is argued in
   `crates/harness-substrate/src/embedded.rs`; MCP discovery grants nothing until
   `crates/harness-mcp` intersects it with a locally reviewed, digest-pinned Harness profile; and
   the tool crates are adapted in `crates/harness-catalogue`, which keeps every envelope, risk,
   approval and publication decision harness's own.
3. **`harness-wire` performs no I/O, reads no clock and names no vendor field.** Every vendor-shaped
   byte lives in `llm`'s wire crates, and `crates/harness-llm` (`b10x-harness-llm`) is the one
   place Harness's values meet `llm`'s; which crates those are and what that crate translates is
   [README § Wires](README.md#wires). `harness-wire` keeps the credential *types* —
   `BearerSource`, `Bearer` (zeroized on drop) and the `StaticBearer` a caller may hold for a
   process lifetime — for an embedder that brings its own model port; the shipped binary's model
   port uses none of them, and the crate reads no credential from anywhere and sends none.

   **`crates/harness-llm` holds no credential and writes nothing vendor-shaped.** A request body,
   a stream decoder, a header name or an endpoint path appearing in it means the boundary was
   drawn in the wrong place. A credential is a `Source` that `llm` asks per request
   (§ Safety envelope, *Credentials*), and the fetched value reaches the transport already inside
   a header value `llm` built.
4. **Turns are stateless.** Nothing is retained provider-side and no wire may use provider-side
   threading, because it would not survive the second wire.
5. **An opaque provider item is replayed verbatim and never reinterpreted.** Replaying one into a
   wire that did not produce it is a **typed refusal**, not a silent drop.
6. **No crate or module named `common`, `shared`, `utils`, `misc` or `helpers`.**

### Discipline

7. **Preserve absence as absence.** Unreported usage stays `None` and never becomes zero. An
   unmodelled stream event or output item is preserved and warned about, never dropped — a dropped
   item is a hole in the conversation the next turn cannot see.
8. **Never truncate silently.** Every bound in `harness-wire::bound` refuses by name. A truncated
   tool result reads to the model exactly like a complete one.
9. **A refusal the model must learn about is an outcome, not an error.** An unpublished tool, a
   denied approval, an oversized result: all come back as a failed `ToolOutcome` so the model knows
   the effect did not happen. Ending the run would leave it believing the call succeeded.
10. **A bound that cannot be enforced is refused, never ignored.** `Budget::validate` refuses
    `max_cost_microunits` **by name** when the run cannot price its model. A loaded rate card that
    covers the model makes that ceiling admissible; an unpriced run must never ignore it.
11. **A budget that binds is an outcome, not a failure.** `LoopStop` carries the reason; `LoopError`
    is only for a run that could not proceed at all.
12. **The default approver is `DenyAll`.** A harness that approves by default turns a review gate
    into decoration.

### Contracts

13. **A contract version is immutable after release.** `contracts/provider-wires/<wire>/<version>/`
    pins one model API subset; `contracts/app-server-profile/<profile>/<version>/` pins the JSON-RPC
    format this harness serves; `contracts/cli/<product>/<version>/` pins the argv surface the
    binary accepts. A change cuts a new version directory. Three interfaces, and the third was
    learned the hard way: `--substrate-embedded` changed from taking a value to bare, and a
    consumer pinned to `0.1.0` was refused by clap before any harness code ran.

    **Released means reachable on `origin/main`**, not "in a tagged release" and not "out of
    `CHANGELOG.md`'s `[Unreleased]`". From the moment a version directory is pushed, somebody can
    pin to it, and what they pinned to must not move under them. Two sessions read this differently
    on 2026-08-29 — one took an `[Unreleased]` changelog entry as licence to regenerate
    `cli/b10x-harness/2026-08-29.1` in place, hours after it was pushed — so the definition is
    written down rather than left to be inferred.

    **A second cut on the same day takes a `.N` suffix**: `2026-08-29`, then `2026-08-29.1`, then
    `2026-08-29.2`. The scheme is date-based and cannot otherwise express two cuts in one day, and
    the alternative — dating a directory tomorrow — puts a false date on a pinned artefact.
14. **Both halves must hold for each contract**: an independent Rust checker verifies the manifest against its
    fixtures, and a Rust test verifies the code produces exactly those bytes or holds exactly those
    constants. A change to what is sent, accepted or emitted re-pins the fixture *and* enters the
    changelog. For `contracts/cli/` the two halves are `cargo xtask cli-contract` and
    `crates/harness-cli/src/contract.rs`, and the pinned document is **generated from clap's own
    definition** — a hand-written one would be a second description of the command line that
    drifts from the first. For `contracts/provider-wires/` the checker half is
    `cargo xtask provider-contracts`, which still verifies every released manifest against its
    fixtures here; the Rust half — the test that the client produces exactly those bytes — is
    `llm`'s since the wire crates were removed (atlas ADR 0097). The released versions stay byte
    for byte and no new one is cut here: a change to what is sent or accepted is an `llm` change,
    and this repository re-pins `llm`.
15. **Bridge-mode method inventories are a copy of the client's, never an import** — copying is what
    keeps the components independent. **Nothing in this repository checks the copy against the
    original**: invariant 2 forbids reading it, and the profile contract only pins this side against
    itself. Re-reading the client's inventory is a **review obligation** whenever the pinned Codex
    version moves, and the only thing that catches a mismatch is running the real bridge.
16. **The declared profile must be one the client actually offers.**
    `codex-app-server-stdio-v2` is its *stable* profile and admits no dynamic tools; declaring it
    while emitting `item/tool/call` yields a server that looks compatible and fails at the first tool
    call.
17. **Fixtures are synthetic and reviewable.** A fixture that writes a credential to disk is a
    fixture that leaks one.
18. **Evidence from the deterministic local endpoint is `provider_emulated`.** It is never promoted
    to `vendor_live`, and no prose may imply a real provider was contacted.

## Safety envelope

- **Credentials.** In the shipped binary a credential is a `harness_llm::Source` — an
  `llm-credentials` resolver bound to one reference — that `llm` asks **at call time**; the fetched
  `Secret` is zeroized when the call ends, and a source re-read on every request
  (`--oauth-token-file`, `--oauth-token-env`) follows a renewed token without a restart.
  `harness-wire`'s `BearerSource`, `Bearer` and `StaticBearer` are the same contract for an
  embedder with its own model port, and a `StaticBearer` does hold the value for as long as the
  caller keeps it. What must never happen on either path is a **config struct or an error carrying
  it**: `Bearer` and `Secret` have no `Display` and a redacted `Debug`, and a refusal never names
  the value. The per-request refusal `llm` builds names only the reference (`oauth-token-file`); a
  refusal at construction — a path `llm` will not read, a variable name it will not accept — may
  name the path or variable the operator passed, which is the operator's own input. **There is no ambient
  credential fallback**: the harness reads nothing it was not pointed at. Never add one, and never
  add a `Display`.
- **Approvals are the review gate (invariant 12).** Adding a path that reaches a tool without
  consulting the approver removes the gate rather than tuning it. What asks is **derived, never
  declared**: the loop reads `ToolPort::invoked` — for a verb over a catalogue, the entry's own
  spec, not the verb's; for a flat surface, the published spec, which is the same document — and
  asks when `Envelope::needs_approval` says so against `LoopConfig::unattended_ceiling` (default
  `Risk::Low`). The same spec is what the approver is handed, what the `ApprovalRequired` event
  names and what the refusal says, so a person decides on `file_write` and never on `tool_invoke`.
  `ToolSpec::approval` can only add asking and is being retired. Until 2026-08-29 no shipped tool
  set it, so the gate never fired; a test now pins that a `run` entry under `DenyAll` is refused.

  **`needs_approval` is `risk > ceiling` and nothing else.** A second clause used to ask about
  every non-idempotent mutation whatever the ceiling, which meant `--approve-up-to high` let a
  `run` and a whole-file `file_write` through unasked and refused every `file_edit` — the inverse
  of the risk ordering, pushing an unattended run toward rewriting files whole. `file_edit` and
  `file_write` are both `Risk::Medium`. `Idempotency` is still declared, for a scheduler that
  re-runs a scope; it is not an approval question.

  **The library default is `DenyAll` and stays so. What the command line chooses is its own
  approver**, which is a different question: `--approve auto` (the default) asks a person over
  `/dev/tty` about one call at a time when there is a terminal to ask on and stdin and stderr are
  one, and otherwise denies — saying so in one stderr line before the run, never falling back
  silently. `--approve prompt` refuses the run when there is no terminal, because a run that named
  a person and then refused everything looks like a harness whose tools do not work. `--yes`
  (`--approve all`) is the **declared** unattended run: a run that says out loud, in its own
  invocation, that nobody is watching. Changing `harness_loop`'s own default would be the
  invariant; changing the shell's is a product decision, and this is the record of it.

  **Under metaharness the decision is the caller's** (atlas ADR 0104, points 2-3: L2 decides each
  call, L1 enforces). `--approve caller` hands every approval to the process that spawned the run:
  the `approval-required` event on the `--json` stdout is the request, and one JSON line on stdin
  (`{"call_id":…,"decision":"allow"}` or `"deny"` with a `reason`) is the answer. Harness still asks
  before the effect and still names the entry, not the verb; what changes is only who answers.
  Everything that is not a well-formed `allow` for the call asked about is a denial the model is
  told about, and a closed stdin denies the rest of the run. `replace` is refused until ADR 0104's
  open question (how a rewrite reaches L1) is answered. The mode needs `--json`, is refused on
  `chat` (stdin carries the questions there) and beside `--yes`. No tool, hook or MCP server of
  a run may read this process's stdin: one that read a decision line would approve its own call.
  Inside a delegate the request arrives wrapped in a `delegated` event, and the answer names the
  inner `call_id`.

  **Bridge mode is the one place the approver is `ApproveAll`**
  (`crates/harness-app-server/src/lib.rs`): the client registered every tool and executes every
  `item/tool/call` itself, so the gate is the client's, and a second one on this side would decide
  something nobody asked this side to decide. A bridge tool the client does not mediate would be a
  new path, not a tuning.
- **The shipped toolset is read-only.** Adding a tool that writes or executes is its own change, with
  its own gate and its own entry in `STATUS.md` — never a flag on an existing one. `--surface` is
  **not** such a flag: it chooses how the same catalogue is published, and both surfaces publish
  exactly what the machine admits and meet the same approval gate. The default implementations now
  come from the `tools` crates: `b10x-tool-fs` reads, `b10x-tool-search` searches,
  `b10x-tool-code-editor` writes, `b10x-tool-process` runs the local `run`, and
  `b10x-tool-toolchain`'s executor runs every toolchain call: it makes the checks of the
  `build-test` contract (arguments, where the named path arguments land, the write scope as
  `Catalogue::refusal` answers it on the written paths the plan declares, the declared programs,
  one deadline per call) and hands each argv to the active
  `Operations::run_within` (`ProviderRunner`), so a Substrate-confined call keeps the providers'
  sandbox the embedded driver applies; never route a toolchain argv around that runner. The write
  scope judges the paths a tool declares it writes: a written directory by everything it can hold
  (`crates/harness-catalogue/src/written.rs`, a bounded on-disk walk that fails closed), and a
  `rustfmt` call by every module file its named files reach (`src/modules.rs`). A toolchain tool
  that writes the workspace without declaring paths (build, test, clippy) is a `Risk::High`
  process call held by approval and confinement, not by the scope; do not describe the scope as
  covering it. Linking a crate is not publishing its tools — `b10x-tool-code-editor` and `b10x-tool-process` are reached only through `Operations` behind
  the `Catalogue` gate, which publishes the writer entries only when `operations.writes()`, at
  `Risk::Medium`, and the `run` entry only when programs are declared; the `run` entry stays harness's own at `Risk::High` behind the approver. A test pins that
  a `run` entry built from the adapter is refused under `DenyAll`
  (`crates/harness-catalogue/tests/run_under_deny_all.rs`).
- **A session is written outside the workspace and carries no credential.** `transcript::Session`
  writes to `$XDG_STATE_HOME/b10x-harness/sessions`, `0700`, never into the tree being worked on:
  a transcript holds whatever the model read, and one filed beside the code is one `git add -A`
  from being committed. It stores no credential and no instruction text — the instruction is a
  function of the run's catalogue, scope and project files, and replaying under a stale one gives
  a run nobody can reproduce from its flags.
- **Three pinned wire manifests carry wire-visible identifiers.** The `b10x_operation_search`
  tool name and the `b10x-emulated` model name are bytes a sender on the other side verifies.
  They were renamed off the former brand together with the agent-side sender and the manifests
  re-pinned to the new bytes, so `contracts/` carries no former-brand token either
  (atlas ADR 0001 § *Wire-visible identifiers*). Renaming either again is a **coordinated
  migration with an ADR in atlas**, done by cutting a new contract version — never by rewriting
  a released one (invariant 13).
- **Four tools belong to the loop, not to the catalogue, and they meet the same gate.** `answer`
  (structured output), `delegate` (sub-agents), `skill` (operator-supplied instructions) and
  `recall` (caller-owned memories) are published by `harness-loop` itself only when a run asks for
  them, resolved in `AgentLoop::run_calls` before the tool port sees a call, and never batched or
  routed by bare name. A skill reads only the immutable documents the caller loaded before the run;
  it performs no ambient discovery, and its result is bounded like every other tool result.
  `recall` is the exact sibling of `skill` and is **read-only**: it returns one record out of a
  `Memories` value the caller constructed before the run, its `id` argument is a schema `enum` over
  the active records, and **no tool and no flag in this binary writes a memory**. That absence is
  deliberate; a writer is its own change with its own gate and its own `STATUS.md` row, and never a
  flag on the reader. A delegate runs a
  second `AgentLoop` over the **same** tool port, approver, hooks and cancellation token, with the
  remainder of the parent's budget: delegation widens nothing, and every call inside a delegate is
  gated on its own entry's envelope exactly as the parent's calls are. Adding another loop-owned
  tool is a design change (design 0002 § 0).

  **Neighbouring `delegate` calls of one turn may run side by side**, and that is an optimisation
  and never a difference in what a run can do. Each child gets a *fork* of the model port and of
  the tool port — the same endpoint, the same catalogue, entry for entry — and the approver, the
  operator's hooks and the event sink are **not** forked: there is one of each by nature, and a
  child on a worker thread reaches all three by asking the run's own thread, one question at a
  time. They run side by side only when the whole reachable surface is non-mutating and needs no
  approval, and no hook is attached: otherwise scheduling would change what one child can observe,
  the order a person is asked, or the order a hook guards. Where that condition fails, a port will
  not fork, or the remaining token budget will not divide, the same delegates run **in order** and
  nothing else about the run changes. Adding a path where a child gets a port the parent did not
  have would be widening by delegation.
- **A hook narrows and never widens, and is never ambient.** `before-call` fires **after** the
  approver said yes; its block is one more refusal and it cannot approve, change or redirect a
  call. `stop` can keep a run working; it cannot end one. Hooks are named on the command line
  (`--hooks <FILE>`) and are **never discovered in the workspace** — a hook found in a repository
  would be a program the repository runs on the operator's machine, which is the ambient fallback
  the credential rule above forbids, for the same reason. The loop spawns no **process**:
  `HookPort` is a port like `ApprovalPort`, and the process-running half lives in `harness-cli`. A
  run with hooks attached batches nothing, so a hook fires exactly once per call — and the one
  place the loop starts **threads**, a group of delegates, keeps the operator's programs off them:
  a child asks the run's own thread, which holds the single `HookPort` the shell attached. *How
  many copies of my guard are running* must not depend on how many sub-tasks a model asked for.
- **This repository is public by explicit decision (atlas ADR 0008).** Public visibility is not an
  open-source licence and does not make a development contract stable; the workspace remains
  `LicenseRef-B10x-Proprietary`. Never commit credentials, tokens, key files or transcripts, and
  never rely on repository privacy to contain one.

## Out of scope

| Belongs elsewhere | Repo |
|---|---|
| Driving a vendor harness, hermetic runs, per-call tool decisions over a protocol | `metaharness` |
| Sandboxed execution, confinement, the durable operation ledger | `substrate` |
| Terminating LLM requests, model routing, backends | `llmgw` |
| Principal identity and token audiences | `identity` |
| Durable event-sourced state | `eventlog` |

## Executable specification

`spec/ess-inputs.yaml` pins ESS 0.76.0. The authored specification describes the native loop and
its library boundary; `conformance/` records the compiled model, selected suite, required scenario
inventory and coverage limits. This is a retrofit of shipped behavior, not permission to invent
new runtime semantics. Preserve source citations and report every unresolved mapping.

`cargo xtask specification` compiles with the exact pinned ESS Rust libraries, checks generated
artifacts for drift, and runs the suite against the real Harness libraries. It also runs an inert
target to prove the scenarios require observable responses. The full gate and `task check` run this
command; they require no ambient ESS executable. `task spec` additionally validates through the
pinned ESS CLI. Native fixture-port evidence is not a live-provider or confinement claim.

## The gate

```console
cargo xtask gate
```

`gate()` in `crates/harness-xtask/src/main.rs` is the definition of what runs; this paragraph
transcribes it and nothing else decides. In order: `cargo test --workspace --locked`,
`cargo test -p b10x-harness-substrate --locked --test conformance`, `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo xtask provider-contracts`,
`cargo xtask cli-contract`, `cargo xtask toolchain-specs`,
`cargo run --locked -p harness-docs -- generate --check`, `cargo xtask specification`,
`python3 scripts/check-app-server-profile.py`, `cargo xtask home-paths` and strict
rustdoc. Three of them — the CLI contract checker, the toolchain specs checker and
`home-paths` — run **twice**: `--self-test` first, on planted fixtures, then the tree.
Run it before every commit. Organization audits in Atlas are separate from this source gate;
ordinary publication does not run Atlas scripts.

**The home-path check judges the index and the worktree, and only a literal POSIX path.** It reads
what `git cat-file` returns for every entry in `git ls-files -s`, because a commit records the index
and not the working copy, and it searches the worktree copy too because `git commit -a` stages that.
It matches the *shape* `/home/<name>` and `/Users/<name>`, with or without a trailing separator,
searched as **bytes** — a path inside a committed `.pyc` or UTF-16 text is the case it exists for —
and its account class admits non-ASCII, so a contributor is protected like the author.

What a green run does **not** say: nothing about `~`, `$HOME` or any path assembled at run time;
nothing about a Windows home directory (`C:\Users\<name>`); nothing about history, which
`story:history-carries-a-home-directory` decided not to rewrite. One account name, `you`, is treated
as a documentation placeholder in every file type — `user` and `username` are not, because they are
account names real machines have. Two historical planning-store paths are exempt with the reason in
`crates/harness-xtask/src/home_paths.rs`:
the journal is append-only and committed, and editing it to satisfy a check would forge the record.
The one evidence record extracted from that journal by the verified revision-5 AEP migration
inherits the exemption only at its exact path and SHA-256; a changed record or sibling does not.
`--self-test` is a gate step of its own, because a check that passed everything would look green.

**`python3` must be available** until the untouched app-server profile check moves on its next
material change. A missing interpreter is a failed gate,
not a skipped check.

**CI is `.github/workflows/gate.yml`**, and it runs `cargo xtask gate` itself rather than a copied
step list, so the two cannot drift; a second job builds on the declared `rust-version`. It needs no
credential: every git dependency in `Cargo.lock` (`ess`, `llm`, `mcp`, `secrets`, `substrate`,
`tools`) is a public repository, so the default checkout reads them all. Until 2026-10-28 both jobs
minted a b10x-bot installation token because substrate was private; a token step that reaches
nothing is a credential handed to a job for no reason, so do not add one back unless a private
dependency arrives, and then name it here.

**A green local gate does not guarantee a green CI.** The script is the same; the toolchain is not —
CI installs whatever `stable` is that day, and a newer clippy can fail a commit that passed locally.
Run `rustup update` before pushing, and read the gate's own exit status, never a pipeline's
**Two worktrees must not share a `CARGO_TARGET_DIR`, and a worktree's build must not land in
`main`'s.** Test binaries bake `env!("CARGO_MANIFEST_DIR")` at build time, so a binary built from a
worktree reads that worktree's fixtures — and when the worktree is gone, a fixture
read fails with `NotFound` in a tree where the file plainly exists (2026-08-29, in the since-removed `harness-flow`, twice in one day:
first as four `run_in` compile errors served across trees, then as two red tests after a merge).
Each worktree builds into its own `target/`; when a shared build cannot be avoided, `cargo clean -p`
every crate the other tree touched before trusting the result.

(`gate.sh 2>&1 | tail` reports `tail`'s status, not the gate's).

Atlas owns the scheduled consumer compatibility suite for current Agentplugins documents.
Its `integration/harness-plugins` tests call Harness's public skill and agent loaders against an
explicit Agentplugins checkout, including `aep-plan`. Missing documents still fail; the parser's
generic fixture tests remain in this repository's gate. Harness neither checks out nor reads its
consumers during its own build, test or release.

## Releases

- Maintain `CHANGELOG.md` in Keep a Changelog form. Every user-visible behaviour, contract, wire or
  boundary change enters `Unreleased` **in the same change that implements it**.
- The tag is the bare version — `0.2.0`, the version and nothing else (atlas § *Naming*) — annotated,
  pointing at a fully gated `main` commit. The `harness-v` prefix was the monorepo's namespace and
  retired with it; a slug is a second copy of what the tag message and the changelog heading already
  carry.
- The full gate comes first. Component steps alone are not enough.

## Where work is tracked

| What | Where |
|---|---|
| What is built, phase by phase, with its exit evidence | `STATUS.md` |
| What is next, as outcomes | `ROADMAP.md` |
| Component design | `docs/design/` |
| Pinned wire and profile contracts | `contracts/` |
| What shipped | `CHANGELOG.md`, and `git tag -n99` |

## Bot identity

Automated commits and pushes use standalone `b10x-gates bot` and the GitHub App identity
`b10x-bot[bot]`. Credentials and token minting remain outside this public source tree.

**There is no bot wrapper script here, and adding one would be a second mechanism.**
`scripts/as-bot.sh` and `scripts/bot-token.sh` — and `scripts/check-bot-files.py`, which only
`bot-token.sh` called — were copies of atlas's, left in the tree when this section moved to
`b10x-gates bot` (`e506af2`, 2026-09-10), and are deleted. Nothing here ran them: not
`cargo xtask gate` (`crates/harness-xtask/src/main.rs` runs one Python check and no other
script), not `.github/workflows/gate.yml`, which mints no token at all. A copy is also a
divergence — this tree's `as-bot.sh` had fallen behind atlas's, missing its `GIT_CONFIG_GLOBAL`
isolation and its refusal to push with `--no-verify`. Atlas and substrate keep their wrappers as
their admitted delivery path by decision (atlas ADR 0057); harness is enrolled on standalone
`b10x-gates` by ADR 0049 and does not.

The three commits before this repository became canonical (`fc676ec`, `14f53f4`, `b61a9bb`) carry
the bot's former name, and two of them carry b10x-bot's own app ID (`316511680`) under it. A
`.mailmap` would put the former brand back in the tree and fail the org fence, and history is not
rewritten, so the record stands as it is.

## Source publication

This repository owns its correctness checks, required reviews and release artifacts. Ordinary
commits, pushes and releases require no Atlas checkout, current Atlas main or organization-wide
dependency admission. Use standalone `b10x-gates bot --repo . -- <git-command>` with protected local
credentials and the existing `b10x-bot[bot]` identity. Preserve repository and worktree hooks.
Delete a pull-request branch only after reading the merge back (`gh pr view <n> --json state` says `MERGED`), never chained to the push: on 2026-10-22 a refused fast-forward was followed by the deletion and GitHub closed the open pull request.
A scratch clone or checkout that fails a clean-and-pushed check (`git status --porcelain` not empty, or commits not on its remote) is kept and reported to whoever directs the session, never removed on the session's own verification: on 2026-10-23 six such clones were removed after a local check that they were held elsewhere, and the list of their unpushed commits had been cut at three per clone.

Atlas documentation validation belongs to documentation operations; it is not a prerequisite for
source publication. Documentation failures affect documentation delivery. Organization privacy
rules still apply; historical brand exemptions do not authorize new public associations.

## Independent documentation source

`website/` is the Docusaurus site served at `https://beyond10x.github.io/harness/`, on the shared
Docs System pinned by commit in `website/package.json`. The site build is the only Node here.
`crates/harness-docs` (`harness-docs`) generates `website/docs/reference/cli.md`, `toolchains.md`,
`crates.md`, `website/docs/status.md` and `website/data/status.json`; never edit those by hand.
`task docs-generate` rewrites them, and `task docs-check` (`harness-docs generate --check`, a gate
step) fails on a stale or edited generated file, on a page without its front matter, on a page
that is not passive Markdown or names internal material, and on a landing page that does not name
the workspace version. A release therefore updates `website/docs/index.md`. `website/README.md`
lists what is generated, the page rules, and the preview and build commands.

`.github/workflows/pages.yml` builds the site with `contents: read` and no credential, runs the
`harness-docs` tests, binds the build to its commit with `harness-docs provenance`, and on a push to `main`
uploads `b10x-project-site`. `.github/workflows/b10x-docs-site.yml` hands that exact artifact to
the shared `project-site.yml` publisher, pinned by commit, which deploys it to Pages. Do not add
a deploy step to `pages.yml` or a credential to either workflow.

Do not claim a source change has deployed the site until the live site serves its commit's
provenance. Keep internal plans, stories, ADRs, decisions, worklogs, security material and research
out of the site unless a repository authority explicitly declares them public.

<!-- b10x-release-operations:start -->
## Release completion

An ordinary release completes after this repository's exact tag, required source checks,
published release and required artifacts are verified. A pushed tag with unfinished checks or
uploads is queued; report it as released only after those requirements succeed.

Atlas reconciliation and public documentation publication run asynchronously. Do not wait for
Atlas or Website, update Website source locks or bootstrap snapshots, promote consumer pins,
release shared docs tooling, or redeploy documentation façades as part of an ordinary source
release. Report documentation as pending unless its publication was actually verified. A background
documentation failure does not invalidate a successful source release.

Keep this repository's provenance, correctness, security, compatibility and artifact verification
requirements. Shared rendering, routing or delivery-control changes still require their relevant
integration gates. A release request does not authorize deployment or downstream releases.
Repositories without a release unit retain their existing publication policy. This completion
boundary supersedes older instructions that attach synchronous documentation ceremony to each
source release.
<!-- b10x-release-operations:end -->
