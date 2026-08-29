# AGENTS.md — harness

The contract for changing **this** repository. Org-wide rules — the naming convention, the
former-brand rule (atlas ADR 0001) and its four exemption categories, and the rule that renaming
anything another repo verifies is a coordinated migration with an ADR — live in `atlas/AGENTS.md`
and are not restated here.

`README.md` orients a reader; `STATUS.md` says what is built and `ROADMAP.md` what is next. This
file says what must not break.

## What this repository owns

The b10x agent loop: turn assembly, tool round trips, approvals, budgets. A harness that talks to
LLM APIs **directly** rather than driving someone else's.

## Invariants

Each is a claim that can be checked. Breaking one is a design change, not a refactor.

### Boundary

1. **No bridges.** This component drives no vendor binary, no subprocess harness and no vendor
   control protocol as a client. Driving a vendor's loop is `metaharness`.
2. **No dependency on any sibling that could embed this.** Something else embeds this; this
   embeds nothing above it — not `metaharness`, `llmgw`, `identity` or `eventlog`. A dependency
   there would quietly re-couple the components the split exists to separate. **The one
   dependency below it is substrate** — `substrate-host` and `substrate-wire`, pinned by git
   revision in `crates/harness-substrate/Cargo.toml`, never by `path`: a path into a sibling
   checkout builds against whatever is checked out there, and `--locked` cannot lock it. The
   boundary that import crosses is argued in `crates/harness-substrate/src/embedded.rs`.
3. **`harness-wire` performs no I/O, reads no clock and names no vendor field.** Every vendor-shaped
   byte lives in a wire crate. It defines the credential *types* — `Bearer`, zeroized on drop, and
   the `StaticBearer` a caller may hold for a process lifetime — but reads no credential from
   anywhere and sends none.

   **`harness-http` is fenced from the other side, and holds no credential either.** It is the
   crate that *does* the I/O and reads the back-off's clock, and it names **no vendor, no field, no
   header and no endpoint path**: a wire hands it a URL, a body, a header list the wire built
   itself and a decoder. A vendor-shaped name appearing in it means the boundary was drawn in the
   wrong place — and the one route difference that could not be neutralised, whether `data: [DONE]`
   ends a stream, is a **parameter both wires set explicitly** rather than a default one of them
   inherits. The credential is fetched by the wire, per attempt, and reaches the transport already
   inside a header value.
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
    `max_cost_microunits` **by name**, because nothing here can convert tokens to money.
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
14. **Both halves must hold for each contract**: a Python checker verifies the manifest against its
    fixtures, and a Rust test verifies the code produces exactly those bytes or holds exactly those
    constants. A change to what is sent, accepted or emitted re-pins the fixture *and* enters the
    changelog. For `contracts/cli/` the two halves are `scripts/check-cli-contract.py` and
    `crates/harness-cli/src/contract.rs`, and the pinned document is **generated from clap's own
    definition** — a hand-written one would be a second description of the command line that
    drifts from the first.
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

- **Credentials.** A credential is fetched from an injected `BearerSource` **at call time** and the
  fetched `Bearer` is dropped when the call ends. A long-lived source such as `StaticBearer` does
  hold the value for as long as the caller keeps it — what must never happen is a **config struct or
  an error carrying it**, so `Bearer` has no `Display` and a redacted `Debug`. **There is no ambient
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

  **Bridge mode is the one place the approver is `ApproveAll`**
  (`crates/harness-app-server/src/lib.rs`): the client registered every tool and executes every
  `item/tool/call` itself, so the gate is the client's, and a second one on this side would decide
  something nobody asked this side to decide. A bridge tool the client does not mediate would be a
  new path, not a tuning.
- **The shipped toolset is read-only.** Adding a tool that writes or executes is its own change, with
  its own gate and its own entry in `STATUS.md` — never a flag on an existing one. `--surface` is
  **not** such a flag: it chooses how the same catalogue is published, and both surfaces publish
  exactly what the machine admits and meet the same approval gate.
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
- **Two tools belong to the loop, not to the catalogue, and they meet the same gate.** `answer`
  (structured output) and `delegate` (sub-agents) are published by `harness-loop` itself when a run
  asks for them, resolved in `AgentLoop::run_calls` before the tool port sees a call, and never
  batched or routed by bare name. A delegate runs a second `AgentLoop` over the **same** tool port,
  approver, hooks and cancellation token, with the remainder of the parent's budget: delegation
  widens nothing, and every call inside a delegate is gated on its own entry's envelope exactly as
  the parent's calls are. Adding a third loop-owned tool is a design change (design 0002 § 0).
- **A hook narrows and never widens, and is never ambient.** `before-call` fires **after** the
  approver said yes; its block is one more refusal and it cannot approve, change or redirect a
  call. `stop` can keep a run working; it cannot end one. Hooks are named on the command line
  (`--hooks <FILE>`) and are **never discovered in the workspace** — a hook found in a repository
  would be a program the repository runs on the operator's machine, which is the ambient fallback
  the credential rule above forbids, for the same reason. The loop spawns nothing: `HookPort` is
  a port like `ApprovalPort`, and the process-running half lives in `harness-cli`. A run with hooks
  attached batches nothing, so a hook fires exactly once per call.
- **This repository is private.** Never commit credentials, tokens, key files or transcripts.

## Out of scope

| Belongs elsewhere | Repo |
|---|---|
| Driving a vendor harness, hermetic runs, per-call tool decisions over a protocol | `metaharness` |
| Sandboxed execution, confinement, the durable operation ledger | `substrate` |
| Terminating LLM requests, model routing, backends | `llmgw` |
| Principal identity and token audiences | `identity` |
| Durable event-sourced state | `eventlog` |

## The gate

```console
bash scripts/gate.sh
```

`cargo test --workspace --locked`, `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets --locked -- -D warnings`,
`python3 scripts/check-provider-wires.py`, `python3 scripts/check-app-server-profile.py`,
`python3 scripts/check-cli-contract.py` — the contract checkers, one per pinned interface. Run it before every commit. The former brand is fenced org-wide by `scripts/check-org-brand.sh` in the **atlas** repo, not here.

**`python3` must be available**: the wire fixtures are a standard-library HTTP server, driven as a
real subprocess over a real socket. A missing interpreter is a failed gate, not a skipped check.

**CI is `.github/workflows/gate.yml`**, and it runs `scripts/gate.sh` itself rather than a copied
step list, so the two cannot drift; a second job builds on the declared `rust-version`. It needs two
repository secrets, `B10X_BOT_APP_ID` and `B10X_BOT_PRIVATE_KEY`, because the substrate dependency
is a private repository and `GITHUB_TOKEN` cannot read it; without them the token step fails by name
and nothing is built. They are provisioned from the atlas checkout —
`bash ../atlas/scripts/bot-ci-secrets.sh beyond10x/harness` — never by hand.

**A green local gate does not guarantee a green CI.** The script is the same; the toolchain is not —
CI installs whatever `stable` is that day, and a newer clippy can fail a commit that passed locally.
Run `rustup update` before pushing, and read the gate's own exit status, never a pipeline's
(`gate.sh 2>&1 | tail` reports `tail`'s status, not the gate's).

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

Automated commits and pushes use the GitHub App via `scripts/as-bot.sh`, never a human credential.
`scripts/bot-token.sh` mints the token; its org default is `beyond10x` (`scripts/bot-token.sh:8`),
which is where the App is installed. **The bot's automation lives in atlas**
(`atlas/scripts/`, `atlas/docs/bot-only-commits.md`); the copies here are byte-identical to it and
are changed there first.

The three commits before this repository became canonical (`fc676ec`, `14f53f4`, `b61a9bb`) carry
the bot's former name, and two of them carry b10x-bot's own app ID (`316511680`) under it. A
`.mailmap` would put the former brand back in the tree and fail the org fence, and history is not
rewritten, so the record stands as it is.
