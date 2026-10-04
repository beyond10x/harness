---
format: aep.planning-md/3
id: story:website-build-dependencies-have-bounded-risk
kind: story
status: implemented
title: Website build dependencies have bounded and reviewed risk
summary: Exact compatible overrides eliminate every website audit finding and retain a green build.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-08-31T03:22:28Z", actor: "human:timo", revision: 2, imported: true}
- {from: "proposed", to: "active", at: "2026-08-31T03:22:28Z", actor: "human:timo", revision: 3, imported: true}
- {from: "active", to: "implemented", at: "2026-08-31T03:29:37Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":2}}, imported: true}
---
## Defect

The website lock resolved vulnerable `serialize-javascript` and `uuid` releases through Docusaurus build tooling. It also resolved archived `image-size@2.0.2`, whose ICNS, JXL, HEIF, and JP2 parsers have unpatched infinite-loop advisories.

## Resolution

Exact overrides select `serialize-javascript@7.1.1`, CommonJS-compatible `uuid@11.1.1`, and API-compatible `image-size-next@2.1.1`. The npm lock pins every registry tarball by integrity. The replacement parser contains forward-progress checks and regression tests for the affected formats; this repository currently supplies only SVG assets.

## Acceptance

A clean install has zero npm audit findings. Typecheck, production build, dependency-tree inspection, and a development-server smoke test pass with the overrides.
