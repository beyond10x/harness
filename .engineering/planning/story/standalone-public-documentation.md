---
format: aep.planning-md/3
id: story:standalone-public-documentation
kind: story
status: active
title: Harness owns a standalone public documentation site
relations:
- serves: vision:b10x-owns-its-loop
- informed_by: epic:public-site-is-accurate-live-and-governed
- derived_from: initiative:record-matches-the-code
scope:
- confidence: cited
  path: .github/workflows
- confidence: inferred
  path: Cargo.lock
- confidence: inferred
  path: Cargo.toml
- confidence: cited
  path: b10x.docs.yaml
- confidence: inferred
  path: crates/harness-docs
- confidence: cited
  path: crates/harness-xtask/src/website_contract.rs
- confidence: cited
  path: website
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-10-03T19:50:26Z", actor: "human:timo", revision: 4}
- {from: "proposed", to: "active", at: "2026-10-03T19:50:26Z", actor: "human:timo", revision: 5}
---
## Outcome

The operator requests public documentation independent like Mantle's website. Replace the shared-renderer dependency with a repository-owned static site, build and provenance while retaining the complete public reference and truthful current capability limits. Mantle's website/index.html, crates/mantle-docs and read-only documentation artifact workflow provide the implementation reference.

## Acceptance

A clean standalone Rust build produces the Harness landing page and every existing public guide/reference at local working links with an exact source provenance record; the source gate checks the site against the shipped CLI contract, rejects private content and unsafe output paths, and the read-only CI job uploads that same artifact without requiring Atlas or a sibling checkout.

## Scope

Cited: website/, crates/harness-xtask/src/website_contract.rs, .github/workflows/pages.yml and b10x.docs.yaml contain today's public source, validation and shared delivery contract. Inferred: a new Rust documentation builder crate, Cargo.toml/Cargo.lock wiring and documentation build integration. Parent records historical delivery; this story changes that delivery model under the explicit operator request, without deploying a site in this session.

## Boundaries

Preserve CLI reference coverage and generated toolchain docs. No new product behavior, release, central Website edit or main merge. Publishing changes to the integration branch is authorized; production deployment is not part of this request. Retain generated delivery files unless the replacement makes their retirement explicit and safe.
