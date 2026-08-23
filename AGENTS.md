# Working on beyond10x/harness

**github.com/beyond10x/harness is the canonical home of the b10x harness.** It was extracted from
the daemonloom monorepo at
[`1e074923`](https://github.com/daemonloom/daemonloom/blob/1e0749233b711744b6e50f9106bba2c33dbbf396/runtime/harness)
on 2026-08-23; the monorepo keeps a pinned git-submodule checkout at `runtime/harness` for its
`inference` consumer. The gate is `bash scripts/gate.sh`.

This repository owns the b10x agent loop: the harness that talks to LLM APIs directly rather
than driving someone else's. The root [`AGENTS.md`](https://github.com/daemonloom/daemonloom/blob/1e0749233b711744b6e50f9106bba2c33dbbf396/AGENTS.md) applies throughout; this file
adds component rules. Read `README.md` and `STATUS.md` first, then `ROADMAP.md`.

The cross-component decision is
[ADR 0052](https://github.com/daemonloom/daemonloom/blob/1e0749233b711744b6e50f9106bba2c33dbbf396/architecture/adr/0052-daemonloom-owns-an-inner-harness-separate-from-its-bridges.md).

## Boundary

- **No bridges.** This component drives no vendor binary, no subprocess harness, and no vendor
  control protocol as a client. Bridges live in [`runtime/agent`](https://github.com/daemonloom/daemonloom/blob/1e0749233b711744b6e50f9106bba2c33dbbf396/runtime/agent).
- **No monorepo dependencies.** Not on `agent-model`, not on `agent-runtime`, not on anything else
  in this repository. Something else embeds this; this embeds nothing. A dependency here would
  quietly re-couple the component the split exists to separate.
- `harness-wire` performs no I/O, reads no clock, and names no vendor field. Every vendor-shaped
  byte lives in a wire crate. It defines the credential *types* — `Bearer`, zeroized on drop, and
  the `StaticBearer` a caller may hold for a process lifetime — but reads no credential from
  anywhere and sends none.
- A credential is fetched from an injected `BearerSource` at call time, and the fetched `Bearer` is
  dropped when the call ends. A long-lived source such as `StaticBearer` does hold the value for as
  long as the caller keeps it — what must never happen is a config struct or an error carrying it,
  so `Bearer` has no `Display` and a redacted `Debug`. There is no ambient credential fallback: the
  harness reads nothing it was not pointed at.
- Turns are stateless. Nothing is retained provider-side, and no wire may use provider-side
  threading, because it would not survive the second wire.
- An opaque provider item is replayed verbatim and never reinterpreted. Replaying one into a wire
  that did not produce it is a typed refusal, not a silent drop.
- Never add a catch-all crate or module named `common`, `shared`, `utils`, `misc`, or `helpers`.

## Discipline

- **Preserve absence as absence.** Unreported usage stays `None` and never becomes zero. An
  unmodelled stream event or output item is preserved and warned about, never dropped: a dropped
  item is a hole in the conversation the next turn cannot see.
- **Never truncate silently.** Every bound in `harness-wire::bound` refuses by name. A truncated
  tool result reads to the model exactly like a complete one.
- **A refusal the model must learn about is an outcome, not an error.** An unpublished tool, a
  denied approval, an oversized result — all come back as a failed `ToolOutcome` so the model knows
  the effect did not happen. Ending the run would leave it believing the call succeeded.
- **A bound that cannot be enforced is refused, never ignored.** `Budget::validate` refuses
  `max_cost_microunits` by name because nothing here can convert tokens to money.
- **A budget that binds is an outcome, not a failure.** `LoopStop` carries the reason; `LoopError`
  is only for a run that could not proceed at all.
- The default approver is `DenyAll`. A harness that approves by default turns a review gate into
  decoration.

## Contracts

- `contracts/provider-wires/<wire>/<version>/` pins one model API subset;
  `contracts/app-server-profile/<profile>/<version>/` pins the JSON-RPC format this harness serves.
  A version is immutable after release.
- Both halves must hold for each: a Python checker verifies the manifest against its fixtures, and a
  Rust test verifies the code produces exactly those bytes or holds exactly those constants. A
  change to what is sent, accepted or emitted re-pins the fixture and enters the changelog.
- Bridge-mode method inventories are a **copy** of the client's, never an import. Copying is what
  keeps the components independent. **Nothing in this repository checks the copy against the
  original** — the no-dependency rule forbids reading it, and the profile contract only pins this
  side against itself. Re-reading the client's inventory is a review obligation whenever the pinned
  Codex version moves, and the only thing that catches a mismatch is running the real bridge.
- The declared profile must be one the client actually offers. `codex-app-server-stdio-v2` is its
  *stable* profile and admits no dynamic tools; declaring it while emitting `item/tool/call` yields
  a server that looks compatible and fails at the first tool call.
- Fixtures are synthetic and reviewable. A fixture that writes a credential to disk is a fixture
  that leaks one.
- Evidence from the deterministic local endpoint is `provider_emulated`. It is never promoted to
  `vendor_live`, and no prose may imply a real provider was contacted.

## Gate

Run before every commit:

```text
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all --check
python3 scripts/check-provider-wires.py
python3 scripts/check-app-server-profile.py
```

`python3` must be available: the wire fixtures are a standard-library HTTP server, driven as a real
subprocess over a real socket.

## Releases

- Maintain `CHANGELOG.md` in Keep a Changelog form. Every user-visible behavior, contract, wire, or
  boundary change enters `Unreleased` in the same change that implements it.
- Release tags are `harness-vMAJOR.MINOR.PATCH` and point at a fully gated `main` commit.

## Safety

- The monorepo is private. Never commit credentials, tokens, key files, or transcripts.
- Automated commits and pushes use the org bot via `scripts/as-bot.sh`; never a human credential.
- The shipped toolset is read-only. Adding a tool that writes or executes is its own change, with
  its own gate and its own entry in `STATUS.md` — not a flag on an existing one.
