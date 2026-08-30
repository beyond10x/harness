---
format: aep.planning-md/1
id: story:status-page-contradicts-the-code
kind: story
status: active
title: The status page agrees with the tree and with itself
relations:
- derived_from: epic:tracking-documents-current
- serves: vision:b10x-owns-its-loop
revision: 5
---
## Evidence

- `AGENTS.md:251` — `STATUS.md` is where "what is built, phase by phase, with its exit evidence" is tracked.
- `STATUS.md:15` — "`--context-window` does not reach bridge mode: `ServerConfig` carries no window, so a bridged turn still compacts on the fixed 192 KiB byte rule". `crates/harness-app-server/src/lib.rs:46-50` declares `pub context_window: Option<u64>`, `:410` applies it with `.with_context_window(...)`, and `crates/harness-cli/src/lib.rs:1350` passes `Some(options.context_window)`. It has been true since `9f26ad5` (2026-08-29 13:27); `STATUS.md` was last written by `82f4b85` (2026-08-29 18:20).
- `STATUS.md:32-35` — "**No sub-agents, no hooks, no MCP client, no multimodal input, no structured output.**" `STATUS.md:13` — "**`answer`, `delegate` and the hook port exist, opt-in per run** (design 0002, 2026-08-29)."
- `STATUS.md:28-29` — "**No Substrate confinement.**" `STATUS.md:23` — "Substrate confinement | **working, embedded, including execution.**"
- `STATUS.md:20` — "The ChatGPT/Codex route still has not been contacted." `STATUS.md:21` — a live run against `https://chatgpt.com/backend-api/codex` on 2026-08-23.
- `STATUS.md:15` — the command line is listed as `run`, `chat`, `sessions`, `tools`, `app-server` and `events`; `contracts/cli/b10x-harness/2026-08-30/argv.json` also holds `workflow`, `workflow plan`, `workflow run`, `profiles` (four verbs) and `providers` (two).
- `STATUS.md:16` — names the argv pin as `contracts/cli/b10x-harness/2026-08-29`; `crates/harness-cli/src/contract.rs:34` pins `2026-08-30`.
- `STATUS.md:43-45` — "792 tests passed … 2026-08-29, after the workflow-runner wave", which is five feature commits ago.
- Commits after `82f4b85`: `a405f46` (skills, named agents), `719f6e3` (providers, profiles), `0c31438` (workspace adoption), `f701e2e` (default model and aliases) — none of them named on the status page.

## Context

`STATUS.md` is the page `README.md:31` tells a reader to trust over the README itself. It is
currently wrong about one thing the code does, disagrees with itself in three places, and does not
mention the last five features to land. Every one of those drifts happened inside a single day.

Three of the contradictions are the same shape: a *does not claim* section written when it was true
and never revisited when the table above it changed. The claim rows are right and the disclaimer
rows are stale, which is the more dangerous direction — a reader trusting the disclaimers thinks
this component has fewer capabilities, and therefore a smaller blast radius, than it has.

## Acceptance

`STATUS.md` names a commit it was observed at, every row is true of that commit, and its
"What this component does not claim" section names nothing the table above it says exists.

## Scope

Derived 2026-08-30 by `story-scoper`. Every line is **cited** (read from the story or the tree) or
**inferred** (a reading that could be wrong).

- **Primary surface:** `STATUS.md` — cited; the whole acceptance is a property of that one file
- **Files:** `STATUS.md:3` (observation commit), `:16` (Command line row), `:21`, `:22`, `:24`
  (Subscription auth, Live provider, Substrate confinement rows), `:27-43` (§ *What this component
  does not claim*), `:45-47` (§ *Test counts*) — cited
- **Symbols:** none — cited; `ServerConfig::context_window`
  (`crates/harness-app-server/src/lib.rs:41-50`) and `ARGV_CONTRACT_VERSION`
  (`crates/harness-cli/src/contract.rs:34`, now `2026-08-30.1`) are read as evidence, not edited
- **Also likely:** `README.md:43` — inferred; it repeats status claims the same wave would correct,
  but the acceptance does not require it
- **Documents:** `STATUS.md` only — cited; no crate, no contract, no gate script changes.
  `scripts/gate.sh:6-11` reads none of these files, so nothing mechanical changes with it — cited
- **Confidence:** high — the story, its epic and the tree all name the same single file, and every
  cited line was re-read at `HEAD` (`c5bb2ed`)
- **Would collide with:** any unit that edits `STATUS.md` — which, under `AGENTS.md:160` ("its own
  gate and its own entry in `STATUS.md`"), is *every* story shipping a user-visible capability, plus
  any unit that rewrites the § *Test counts* block after a gate run. It collides with no crate.

Stale citations found while scoping: `STATUS.md` moved twice since the story was written (`bfcc352`,
`c5bb2ed`), so `:15`→`:16`, `:32-35`→`:27-43`, `:43-45`→`:45-47`. Two of the five cited drifts are
already fixed in the tree — `:3` names commit `5b8d21a`, and the sub-agents/hooks/structured-output
disclaimer now says they shipped — so the remaining work is smaller than the body claims. `5b8d21a`
is nine commits behind `HEAD`, so "names a commit it was observed at" holds but at a stale commit.
