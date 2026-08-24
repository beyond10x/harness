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
2. **No dependency on any sibling component.** Something else embeds this; this embeds nothing. A
   dependency here would quietly re-couple the components the split exists to separate.
3. **`harness-wire` performs no I/O, reads no clock and names no vendor field.** Every vendor-shaped
   byte lives in a wire crate. It defines the credential *types* — `Bearer`, zeroized on drop, and
   the `StaticBearer` a caller may hold for a process lifetime — but reads no credential from
   anywhere and sends none.
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
    format this harness serves. A change cuts a new version directory.
14. **Both halves must hold for each contract**: a Python checker verifies the manifest against its
    fixtures, and a Rust test verifies the code produces exactly those bytes or holds exactly those
    constants. A change to what is sent, accepted or emitted re-pins the fixture *and* enters the
    changelog.
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
- **Approvals are the review gate (invariant 12).** Changing the default, or adding a path that
  reaches a tool without consulting the approver, removes the gate rather than tuning it.
- **The shipped toolset is read-only.** Adding a tool that writes or executes is its own change, with
  its own gate and its own entry in `STATUS.md` — never a flag on an existing one.
- **Three pinned wire manifests carry wire-visible identifiers.** The `b10x_operation_search`
  tool name and the `b10x-emulated` model name are bytes a sender on the other side verifies.
  They were renamed off the former brand together with the agent-side sender and the manifests
  re-pinned to the new bytes, so `contracts/` carries no former-brand token either
  (atlas ADR 0001 § *Wire-visible identifiers*). Renaming either again is a **coordinated
  migration with an ADR in atlas**, done by cutting a new contract version — never by rewriting
  a released one (invariant 13).
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
the contract checkers. Run it before every commit. The former brand is fenced org-wide by `scripts/check-org-brand.sh` in the **atlas** repo, not here.

**`python3` must be available**: the wire fixtures are a standard-library HTTP server, driven as a
real subprocess over a real socket. A missing interpreter is a failed gate, not a skipped check.

**A green local gate does not guarantee a green CI.** The steps mirror each other; the toolchain does
not — CI installs whatever `stable` is that day, and a newer clippy can fail a commit that passed
locally. Run `rustup update` before pushing, and read the gate's own exit status, never a pipeline's
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
`scripts/bot-token.sh` mints the token, and **the bot-org default it applies at
`scripts/bot-token.sh:8` is not the org this repository lives in** — set that variable explicitly to
`beyond10x` rather than relying on the default.
