---
format: aep.planning-md/1
id: epic:gate-stays-trustworthy
kind: epic
status: draft
title: The gate is trusted because nothing in it is unexplained
summary: One test that failed once and was never pinned; one duplicated CI step named as cleanup and left.
relations:
- decomposes: initiative:record-matches-the-code
revision: 2
---
## Evidence

- `AGENTS.md:203-212` — the gate is `bash scripts/gate.sh`: tests, format, clippy and the three contract checkers, run before every commit.
- `AGENTS.md:217-222` — CI runs `scripts/gate.sh` itself "rather than a copied step list, so the two cannot drift".
- `AGENTS.md:225-235` — a green local gate does not guarantee a green CI, and two worktrees must not share a `CARGO_TARGET_DIR` (twice in one day, 2026-08-29).
- `docs/reviews/2026-08-29-code-review-2.md:38-41` — "`bridge_mode::an_unknown_request_mid_turn_is_refused_without_killing_the_server` failed once under a loaded gate (`completed` where `interrupted` was expected) and passed 5/5 alone — a race between the `slow` fixture and the interrupt, **not yet pinned**".
- `docs/reviews/2026-08-29-code-review-2.md:26-28` — "Not done, cleanup only: the duplicated token steps in `gate.yml` (GitHub Actions has no YAML anchors; a composite action is the fix)".
- `.github/workflows/gate.yml:42-59` and `:80-95` — the two identical token-and-git-rewrite step pairs.
- `STATUS.md:43-47` — 792 tests, and three crates that "had already drifted from the gate they claimed before this wave".

## Outcome

Nothing in the gate is known-flaky and unexplained. A red run means a defect, which is the only
thing that makes a gate worth running before every commit.

## Why Now

The one recorded intermittent failure is in bridge mode — the area with no external proof at all
(`epic:bridge-mode-proof`), so an unexplained failure there is the failure mode hardest to dismiss.

## Scope

Pin the interrupt race; remove the duplicated CI credential steps. Both were named in a review on
2026-08-29 and neither was done.

## Done When

The bridge-mode interrupt test either passes deterministically under load or is replaced by one that
does, and `gate.yml` states its token step once.
