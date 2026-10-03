---
format: aep.planning-md/3
id: story:standalone-public-documentation
kind: story
status: implemented
title: Harness owns a standalone public documentation site
relations:
- serves: vision:b10x-owns-its-loop
- informed_by: epic:public-site-is-accurate-live-and-governed
- derived_from: initiative:record-matches-the-code
scope:
- confidence: cited
  path: .github/workflows
- confidence: cited
  path: AGENTS.md
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: Taskfile.yml
- confidence: cited
  path: crates/harness-xtask/src/main.rs
- confidence: cited
  path: crates/harness-xtask/src/website_contract.rs
- confidence: cited
  path: website
revision: 9
transitions:
- {from: "draft", to: "proposed", at: "2026-10-03T19:50:26Z", actor: "human:timo", revision: 4}
- {from: "proposed", to: "active", at: "2026-10-03T19:50:26Z", actor: "human:timo", revision: 5}
- {from: "active", to: "implemented", at: "2026-10-03T20:19:37Z", actor: "human:timo", revision: 9, decided_on: {"recorded":{"test_result":1}}}
---
## Outcome

The operator requests public documentation independent like Mantle's website. Replace the shared-renderer dependency with a repository-owned static site, build and provenance while retaining the complete public reference and truthful current capability limits. Mantle's website/index.html, crates/mantle-docs and read-only documentation artifact workflow provide the implementation reference.

## Acceptance

A clean standalone Rust build produces the Harness landing page and every existing public guide/reference at local working links with an exact source provenance record; the source gate checks the site against the shipped CLI contract, rejects private content and unsafe output paths, and the read-only CI job uploads that same artifact without requiring Atlas or a sibling checkout.

## Scope

Cited from the completed implementation: website/ owns a separate Rust workspace and lockfile, static source and authored Markdown; .github/workflows/pages.yml validates and packages its exact artifact. crates/harness-xtask/src/main.rs and Taskfile.yml integrate its checks; the existing website_contract.rs continues checking shipped CLI coverage. AGENTS.md and CHANGELOG.md describe the new source boundary. b10x.docs.yaml and generated b10x-docs-* delivery workflows remain unchanged legacy routing, not dependencies of the independent builder. No root Cargo dependency wiring is needed for the standalone site.

## Boundaries

Preserve CLI reference coverage and generated toolchain docs. No new product behavior, release, central Website edit or main merge. Publishing changes to the integration branch is authorized; production deployment is not part of this request. Retain generated delivery files unless the replacement makes their retirement explicit and safe.

## Implementation and verification

The independent website workspace resolves only crates.io dependencies and builds 18 pages: a new landing page and all 17 existing documentation pages. Rust 1.97 formatting, six builder tests, strict Clippy, rendered navigation/link/anchor validation and preview build pass. Tests reject broken anchors and routes, private or active content, unsafe output paths, source/output symlinks and incorrect source provenance; an actual dirty production build refuses. Desktop (1440 pixels) and mobile (390 pixels) Chromium previews were visually inspected; mobile navigation now defaults closed. Product CLI and generated toolchain reference checks remain green.

The integrated `task check` completed with `gate: green`, including website checks, 42 passing native ESS scenarios, a no-op target with all 42 failing, and strict rustdoc. `cargo +1.97 check --workspace --all-targets --locked` passed. These runs use uncommitted integration source; they are not published-commit or live-site evidence.

The read-only workflow validates pull requests and uploads the exact main artifact. Legacy delivery integration remains unchanged; this implementation neither deploys the new site nor changes the live route. The integration commit is separately blocked by decision-blocker:migrated-evidence-policy-exception.
