---
format: aep.planning-md/1
id: dependency-blocker:llm-plan-publication
kind: dependency-blocker
status: open
title: Existing history admission blocks publishing the LLM plan
relations:
- blocks: epic:llm-adoption
revision: 1
---
## Blocking fact

Existing remote main commit `709a2ebadcc14602b82b6f3c240350e4ddc1c88c` has an author outside the bot allowlist. After fetching two missing historical blobs in the partial clone, the unchanged Gates pre-push hook refused `commit 709a2ebadcc14602b82b6f3c240350e4ddc1c88c has inadmissible authorship`. The new planning candidate itself has exact bot author and committer. No hook, baseline, policy or published history was changed.

## Completion boundary

This blocks publication/integration of the planning candidate, not a claim that the LLM runtime exists. The source remains in the managed `llm-plan-harness` tree on `plan/llm-foundation`.

## Clearing evidence

The Gates owner must resolve the existing-history admission conflict through the supported policy/tool process. Clear only after the exact planning candidate passes the unchanged coordinated publication checks and its remote ref is verified. Rewriting published history or bypassing a hook is not an authorized workaround.

## Evidence

Bot-authenticated push attempts on 2026-09-19; candidate Git author/committer inspection; local repository correctness and AEP gates passed. The publication refusal is retained in the session handoff.
