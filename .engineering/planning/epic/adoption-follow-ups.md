---
format: aep.planning-md/1
id: epic:adoption-follow-ups
kind: epic
status: proposed
title: Deferred work from the profiles and workspace-adoption wave
summary: Five things named rather than left to be found, while shipping providers, profiles, model aliases and real-directory workspaces on 2026-08-29/30.
revision: 4
---
Body written 2026-09-15 during triage of the draft backlog (ORG-0201). Until then this file held the
unedited `epic` template — the frontmatter said `epic:adoption-follow-ups` and the prose said
`# Epic: <name>`, so nothing under it could be read as agreed work. What follows is reconstructed
from the artifacts that already point at this epic and from the tree; where a claim comes from one
of those children rather than from a file read here, it says so.

## Outcome

The work that was named-and-deferred during the providers / profiles / model-alias /
real-directory-workspace wave of 2026-08-29 and 2026-08-30 is either done or explicitly retired, so
that "we knew about this" and "we are doing something about this" stop being the same sentence.

## Why Now

The wave shipped four user-visible changes in one evening —
`719f6e3` *providers and profiles, so the flags that never vary live in a file* (2026-08-29),
`0c31438` *a run can be pointed at a real project directory* (2026-08-29),
`f701e2e` *a default model, and an alias resolves wherever one is named* (2026-08-29) — and each
left something behind that a later reader would otherwise have to rediscover. The epic exists so the
leavings have one home instead of being scattered across commit messages.

## Scope

The seven items filed against this epic, and nothing else:

| child | status |
|---|---|
| `story:codex-provider` | implemented |
| `story:go-toolchain-in-confined-runs` | implemented |
| `story:no-home-path-reaches-a-commit` | implemented |
| `story:verbs-surface-narrowing` | implemented |
| `story:history-carries-a-home-directory` | rejected |
| `story:daemon-exec-start-adoption` | proposed |
| `story:repo-local-profiles` | proposed |

## Out of Scope

Anything the wave did **not** defer. New provider entries, new profile keys and new workspace
backends are their own work; this epic is a closing list, not a feature area. Widening what a
guarded workspace admits belongs to the repository that owns confinement, which is why
`story:daemon-exec-start-adoption` is written as a request against a wire contract rather than as a
change to be made here.

## Risks

Both open children are blocked on a decision rather than on effort, and a decision nobody takes
looks exactly like work nobody started:

- `story:repo-local-profiles` cannot be implemented before somebody rules on whether a
  repository-supplied profile may carry permission keys at all. `crates/harness-cli/src/profile.rs`
  still resolves one location, `config_path()` at `:89-94`, and that is the whole of the surface the
  decision would change.
- `story:daemon-exec-start-adoption` needs the wire's own identity vocabulary decided with it. At
  the revision this tree pins, the daemon still requires the `ws_` prefix
  (`crates/substrate-daemon/src/app/operations.rs:416` in the pinned checkout), so an adopted
  directory can be read and written and then refused an exec.

## Done When

Every child above is `implemented`, `rejected` or `archived`, and no item from that wave is carried
only in a commit message.
