---
format: aep.planning-md/1
id: story:website-typecheck-is-side-effect-free
kind: story
status: implemented
title: Website type checking cannot create duplicate routes
summary: Type checking emits no JavaScript and a subsequent build has one route per page.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Defect

`npm run typecheck` emits JavaScript beside every TypeScript source because the website tsconfig does not set `noEmit`. Docusaurus then discovers both `index.tsx` and the emitted `index.js`, reports a duplicate `/harness/` route, and can choose either copy non-deterministically.

## Acceptance

Type checking is side-effect free, leaves no generated source copies, and a clean typecheck followed by the production build reports no duplicate routes.
