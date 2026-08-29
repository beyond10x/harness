---
format: aep.planning-md/1
id: story:a-steps-scope-is-the-scope-it-runs-under
kind: story
status: draft
title: A step's declared write scope is the scope it runs under, so the same map denies the same write on both arms
summary: 'The projection carries each step''s first-match-wins scope and FlowRunner ignores it: the toolset is built once per run, so the eval''s deliberate-denial step wrote revision: 99 to the planning store on the native arm where the driven arm refuses it at the tool layer'
owner: harness
tags:
- safety
- workflow
revision: 1
---
# Story: a step's declared write scope is the scope it runs under

## Outcome

`protocol workflow flow --map` writes each step's `scope` into its node — the map's own
first-match-wins list, `.engineering/**=denied` before a catch-all. `FlowRunner` never reads it.
The toolset, the approver and the write scope are built once in `prepare` from the command line
(design 0003 § 2, § 6: *a published toolset per group is M2*), so **every step of a walk runs under
the run's scope and the document's own is decorative**.

That is a difference between the two arms in the direction that matters. The eighth paid native
walk (metaharness `native-eval.ew4lFi`, 2026-08-30) ran the eval's deliberate-denial step, whose
map entry denies writes to `.engineering/**` precisely so a refusal can be observed. The write was
not refused: `revision: 99` reached
`ws_project/.engineering/planning/specification/passkey-login.md` on disk, and the only thing that
caught it was the store's own after-the-fact validator —

```
specification:passkey-login claims revision 99, and no write produced it: … a revision above the
log's own was written by hand, not by a command
```

A driven run of the same map refuses that edit at the tool layer, before it happens. The native
run recorded it after it happened. *Prevented* and *detected* are not the same guarantee, and the
document said prevented.

After this story a step runs under the scope its node declares, so the same map denies the same
write on both arms.

## Acceptance

- A step whose `run.scope` denies a path cannot write it: the call is refused in the tool layer,
  the model is told, and nothing reaches disk.
- A step whose node declares no scope runs under the run's, exactly as today — a document that
  says nothing does not silently narrow a run.
- The scope is applied **in the order the map wrote it**: first match wins, and re-ordering the
  list changes what it means.
- A walk of the eval's map leaves the store `valid`, because the write that invalidated it is
  refused rather than audited.

## Notes

Design 0003 § 6 files this under *a published toolset per group*, one milestone with per-section
tools. The write scope is the half with a safety consequence and is worth doing first and alone:
a narrower toolset is a smaller surface, but an unenforced `denied` is a stated rule that does not
hold.

`Scope` and `ScopeRule` already exist (`harness_tools`), `--write-scope` already parses the same
`<glob>=<word>` grammar the projection emits, and `crate::write_scope` is the function that turns
the declarations into one. What is missing is rebuilding the published toolset per step from the
node rather than once per run.

## Evidence

A walk under a document whose step denies a path, asserting the refusal in the record and the file
unchanged on disk; and a re-run of metaharness `run-native.sh` whose store is `valid` at the end.
