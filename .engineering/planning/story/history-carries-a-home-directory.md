---
format: aep.planning-md/1
id: story:history-carries-a-home-directory
kind: story
status: draft
title: Two published commits carry the operator's home directory
relations:
- derived_from: epic:adoption-follow-ups
revision: 2
---
## What is true

This repository was made public on 2026-08-30. Two commits in its history contain the operator's
home directory:

- `a405f46` — `crates/harness-cli/src/{skills,agents}.rs` tested against
  `/home/timo/beyond10x/engineering-protocols/...`, absolute.
- `719f6e3` — the same lines, changed to `env!("CARGO_MANIFEST_DIR")` and a relative path.

`HEAD` is clean: `git grep` finds nothing, and no commit in the 77 contains a credential pattern
(`sk-ant-`, `ghp_`, `github_pat_`, `xoxb-`, `AKIA`, PEM headers) — checked with `git log -S` across
all refs before the repository was published.

## Why it was published anyway

A decision, taken with the exposure stated: a username and a directory layout, already public in
four sibling repositories, against a force-push that would invalidate every clone and every pinned
revision — and `metaharness` pins harness commits. The cost of the rewrite was judged higher than
the exposure.

The same leak was fixed at HEAD in all five repositories the same evening
(`metaharness 7a71847`, `engineering-protocols 67f00f3`, `entity-runtime aa757b5`, and the
`substrate` journal in that repository's own commit).

## What this story is for

Not to reverse the decision — it is recorded so that nobody re-derives it, and so the answer exists
if the question is asked from outside.

Also worth noting: the leak's second defect was worse than the first. Those tests passed on exactly
one machine, so the absolute path was a portability bug wearing a privacy bug's clothes. That half
is fixed and cannot recur — `env!("CARGO_MANIFEST_DIR")` resolves anywhere.

## Acceptance

One of:

- the decision stands and this is moved to `rejected`, which records that it was considered; or
- somebody wants the history clean, in which case the rewrite is planned with its consumers —
  `metaharness`'s pins first — and this becomes a task with a coordination step, not a `git filter`
  somebody runs on a Friday.

## Prevention, which is the part that generalises

An audit of all five repositories found twenty tracked files publishing the same thing, including a
committed `.pyc` in `entity-runtime` whose `co_filename` embedded the path and which **no text grep
would ever have found**. A pre-commit or CI check for absolute home directories in tracked files
would have caught nineteen of the twenty; the twentieth is why `git ls-files` deserves a look of its
own. That check does not exist and is the thing most likely to stop this recurring.
