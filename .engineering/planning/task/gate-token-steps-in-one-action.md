---
format: aep.planning-md/1
id: task:gate-token-steps-in-one-action
kind: task
status: draft
title: The CI token step is written once
relations:
- decomposes: epic:gate-stays-trustworthy
revision: 2
---
## Evidence

- `docs/reviews/2026-08-29-code-review-2.md:26-28` — "Not done, cleanup only: the duplicated token steps in `gate.yml` (GitHub Actions has no YAML anchors; a composite action is the fix)".
- `.github/workflows/gate.yml:42-59` — the `gate` job: mint an app token, then rewrite the git URL for `beyond10x`.
- `.github/workflows/gate.yml:80-95` — the `msrv` job: the same two steps, byte for byte.
- `.github/workflows/gate.yml:9-13` — why the steps exist: substrate is a private repository and `GITHUB_TOKEN` cannot read across repositories; "Without them the token step fails by name and nothing is built".
- `AGENTS.md:217-219` — CI runs `scripts/gate.sh` itself rather than a copied step list, "so the two cannot drift" — the same argument this duplication is the exception to.

## What to do

Move the two steps into a local composite action (`.github/actions/…`) and call it from both jobs.
No change to what runs.

## Done When

The token mint and the git URL rewrite are written once in `.github/workflows/`, and both jobs still
build against the private dependency.
