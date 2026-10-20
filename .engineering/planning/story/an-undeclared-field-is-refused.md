---
format: aep.planning-md/3
id: story:an-undeclared-field-is-refused
kind: story
status: implemented
title: A tool call with a field its schema does not declare is refused
summary: Catalogue entries with additionalProperties false refuse an undeclared field, naming it (file_write ignored a stale expected_sha256 at 0.15.1). Release after 0.16.0.
relations:
- serves: vision:b10x-owns-its-loop
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T22:55:45Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-20T22:55:45Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-10-20T23:46:10Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1,"review_outcome":1}}}
---
## Context

metaharness (wave 3, unit 3), measured on harness 0.15.1 through its served surface and relayed by
`b10x-chef-orchestrator` (2026-10-20): `file_write` declares `additionalProperties: false`
(`crates/harness-catalogue/src/catalogue.rs:855` at 0.15.1; `file_edit` the same at :879), yet a call
carrying an undeclared `expected_sha256` with a stale digest is accepted — the field is ignored, the
file is replaced, `isError` is false. A caller that sends a digest guard gets neither the guard nor a
refusal: a possible lost update. Not a write-scope bypass; no advisory.

## Outcome

A tool call carrying a field its entry's input schema does not declare is refused, naming the field,
for every catalogue entry whose schema says `additionalProperties: false`.

## Acceptance

- `file_write` and `file_edit` with an extra `expected_sha256` are refused with a reply naming
  `expected_sha256`; the file is unchanged.
- The same holds for every catalogue entry (flat and verbs surfaces, toolchain entries, loop-owned
  tools) whose published `input_schema` has `additionalProperties: false`; a test walks the published
  catalogue and sends each one an undeclared field.
- An entry whose schema does not forbid extra properties behaves as before.
- `CHANGELOG.md` lists the refusal; `cargo xtask gate` exits 0; argv byte-identical.

## Scope

- `crates/harness-catalogue/src/catalogue.rs` (argument checking in `invoke` before dispatch), the
  verbs/flat publication if their schemas are checked elsewhere, loop-owned tools in
  `crates/harness-loop` if they declare `additionalProperties: false`; tests; `CHANGELOG.md`.

## Out of scope

- Implementing `expected_sha256` as a guard (`code-editor` has `expected_sha256` preconditions; harness
  does not publish them yet) — a separate decision.

## Sequencing

The release after 0.16.0 (`b10x-chef-orchestrator`, 2026-10-20).
