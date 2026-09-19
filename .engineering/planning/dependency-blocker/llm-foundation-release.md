---
format: aep.planning-md/1
id: dependency-blocker:llm-foundation-release
kind: dependency-blocker
status: open
title: A qualified full LLM foundation release is required
relations:
- blocks: epic:llm-adoption
- blocks: story:llm-cost-adoption
- blocks: story:llm-neutral-interface
- blocks: story:llm-provider-routing
revision: 1
---
## Upstream requirement

The upstream artifact is llm/story:foundation-qualified in beyond10x/llm. All first-milestone capabilities must be qualified at one exact released revision before consumer adoption begins. Record release identity, contract versions and verification evidence here before clearing.

## Tool limitation

AEP 0.55.0 new --relate depends_on:llm/story:foundation-qualified refuses with: `the kind contains disallowed character /` while addressing mutation evidence. These local blocks are admitted graph edges; the upstream reference is prose, not a machine-resolved dependency. No cross-store completion is inferred.
