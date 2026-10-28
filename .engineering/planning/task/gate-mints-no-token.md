---
format: aep.planning-md/3
id: task:gate-mints-no-token
kind: task
status: implemented
title: The CI gate mints no token, because every dependency is public
tags:
- d142
relations:
- decomposes: epic:gate-stays-trustworthy
- serves: vision:b10x-owns-its-loop
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-28T14:51:24Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-28T14:51:24Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-10-28T17:43:41Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"verification":1}}}
---
## Evidence

- `.github/workflows/gate.yml` (`origin/main` `c715947`): both jobs mint a b10x-bot installation
  token ("Token for the private substrate dependency") and rewrite git's `beyond10x` URLs to carry
  it; the header says "The substrate dependency is a private repository".
- `AGENTS.md` § The gate repeats the premise: "It needs two repository secrets, … because the
  substrate dependency is a private repository and `GITHUB_TOKEN` cannot read it".
- The premise no longer holds. `gh api repos/beyond10x/<repo> --jq .private` answers `false` for
  every git source in `Cargo.lock` (`ess`, `llm`, `mcp`, `secrets`, `substrate`, `tools`),
  read 2026-10-28. No other workflow mints a token (`git grep app-token -- .github`).

## What to do

Remove the token and URL-rewrite steps from both jobs and the premise from the workflow header
and `AGENTS.md`. Nothing else in CI changes.

## Done When

`gate.yml` mints no token, and the `Gate` and `MSRV` jobs are green on the pull request that
removes it.

Supersedes `task:gate-token-steps-in-one-action`: there is nothing left to write once.
