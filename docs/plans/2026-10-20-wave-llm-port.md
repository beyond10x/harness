# Wave llm-port — 2026-10-20

One unit, `story:llm-port-is-a-public-crate`, on `wave/2026-10-20-llm-port`, branched from the
tools-1 merge on main (`947d0e48`). Approved by `b10x-chef-orchestrator` under the operator's
exception 5 (atlas ADR 0097): "0.15.0 = tools-1 + story:llm-port-is-a-public-crate … merge tools-1
to main; run the short llm-port wave on a branch from that merge; merge it to main; then cut
0.15.0."

## The unit

| | |
|---|---|
| story | `story:llm-port-is-a-public-crate` |
| branch | `impl/llm-port-is-a-public-crate`, tree `harness-llm-port-u1` |
| commit | `d32192a3` — `harness-cli`'s private `llm_port` becomes `crates/harness-llm` (`b10x-harness-llm`) |
| merge | `b00c8425` |

Implementor gate (package-scoped, every cargo command through `b10x-build-slot`): workspace build,
`cargo test -p b10x-harness-llm` (12 passed), `cargo test -p b10x-harness-cli` (lib 256, integration
148), clippy, fmt, `cargo xtask cli-contract` (12 pinned versions verified), all exit 0.

## Review

`review-result:adversary-llm-port-pass-1`: 8 consumer-shaped cases added
(`crates/harness-llm/tests/adversary_public_surface.rs`), 7 green, 1 red; two notes.

| finding | outcome |
|---|---|
| `renew_codex_login`, now public, panicked inside a tokio runtime | fixed in `d32192a3`: a scoped thread when the caller drives a runtime; the red case is green |
| `STATUS.md` Source row cited a command that did not measure its claim | fixed: cites `cargo tree -i b10x-llm-core --workspace` |

## Added on the wave branch before the gate

| commit | what | approved by |
|---|---|---|
| `6cc50452` | tools crates pinned at `0.2.0` (`60d4d0e3`, Apache-2.0) | `b10x-chef-orchestrator`, "Firm … 0.15.0 pins tools 0.2.0" |
| `62f44050` | README/AGENTS.md: llm and tools as foundations, model calls through `llm`, MSRV 1.98, invariant 3 links README § Wires; two website pages' MSRV | `b10x-chef-orchestrator`, release-PR doc fixes (ADR 0096 §1–3, 0097) |

`vision:b10x-owns-its-loop` revised to revision 8 in the closing planning commit (summary, evidence
and line citations re-read at `62f44050`).

## Gate

`~/.cache/harness-llm-port-gate-steps.sh` at `62f44050` (mirrors `gate()` in
`crates/harness-xtask/src/main.rs:90-139`): 17 steps, all exit 0 — test-workspace 106 s,
substrate conformance 19 s, clippy 43 s, provider-contracts 31 s, specification 38 s, rustdoc 4 s.
