---
format: aep.planning-md/3
id: approval-record:docs-system-0-12-0-rollout
kind: approval-record
status: draft
title: docs-system 0.12.0 pin rollout approved by session b10x-chef-orchestrator
summary: Pin batches, getting-started, docs-system migration note, ess-ui-templates and its gates baseline, 2026-10-22
revision: 2
---
## The stop

The docs-system 0.12.0 pin rollout across the organization's documentation sites, run from this
session on 2026-10-22, and the follow-ups it turned up. docs-system 0.12.0 is tag `0.12.0` at
`5fd348a`; it adds the a2a family signature, a generated favicon and `.nojekyll` in the build.

## Who decided

Session `b10x-chef-orchestrator`, 2026-10-22, under the operator's standing grant for class-C items
in organization repositories. A relayed approval, not a bypass. The decisions, verbatim where they
set scope:

- Pin batches: batch 1 stays on 0.11.1; batches 2 and 3 pin 0.12.0. "One build at a time while disk
  is under 60G."
- commission: dropped, the repository is archived; its local commit stays unpushed.
- agentplugins#92: merged through the bot route; its red "Skills match the newest CLI releases"
  check fails on `main` as well.
- getting-started: "Supply DocumentationSurface.shell in src/pages/changes.tsx the way docs-system
  0.12.0's own source and template use it. In the same PR run npm audit fix without --force. […] A
  red audit that was already red on main does not block the merge."
- docs-system: "one small bot PR that adds a Migration line under 0.12.0. No release."
- ess-ui-templates: "the sed fix is fine. Then, as a separate PR, move the ESS 0.52.0 pin to the
  newest ESS release […], regenerating anything ESS generates."
- gates-policy: the ess-ui-templates adoption baseline moves from `c834491` to `5527832`, the root
  of its `main`, after a clean local scan. Publishing the CI secret is the operator's.

## What it authorises and what happened

| Item | State |
|---|---|
| Batch 1 at 0.11.1 | merged: aep#139, aep-service#30, agentic-principles#20, canon#19, conductor#79, connectors#200, agentplugins#92 |
| Batch 2 at 0.12.0 | merged: cortex#82, engineering-protocols#23, entity-runtime#79, ess#632, eventlog#55, inference#5 |
| Batch 3 at 0.12.0 | pending: llm, llm-gateway, loom, modelspec, secrets, swarm, takt, thalamus, uilab |
| getting-started#8 | open. The registry map is typed `RegistrySurface` rather than inventing a `shell`; `ChangeTimelineEntry` takes any documentation surface. `npm audit fix`: 62 vulnerabilities (19 critical) to 45 (17 critical) |
| docs-system#36 | open, CHANGELOG migration note only |
| gates-policy `d9c0c44` | pushed. `b10x-gates check` from `5527832`: 8 commits on `main`, 9 with the pending commit, 0 findings; `audit` 0 |
| ess-ui-templates pin and `FONT_RANGE` sed | committed locally, held until the CI policy secret carries the new baseline |
| ess-ui-templates ESS bump | waits for the item above |

## Closed after the rollout

swarm is closed without a pin bump. The operator decided on 2026-10-23, as relayed by session
`b10x-chef-orchestrator`, quoted: "swarm: no work. Timo closed it on 2026-10-23 as option B,
"swarm stays unpinned" (decisions.md 13:39Z, queue row 29)."

- The batch-3 bump commit `7b7b07f` was never pushed. The bot push was refused with "branch
  authority missing or ambiguous", because swarm has no `b10x-bot-branch-authority` ruleset. The
  commit is kept only in the managed tree `swarm-docs-system-0-12-0`.
- No further swarm work follows from this rollout.

docs-system 0.12.1 (tag on `48d96c3`) and 0.12.2 (tag on `b2729c0`, released 2026-10-22T23:56Z)
carry the cortex template defects s2-01, s2-02, s5-03, s4-01, s2-03, s1-04 and s4-02. The
orchestrator accepted the s1-04 decision on 2026-10-23: a kind word in prose inline code no longer
gets a kind chip, while table cells and `<Kind>` keep theirs.
