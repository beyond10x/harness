---
format: aep.planning-md/3
id: story:harness-builds-on-llm-0-15
kind: story
status: implemented
title: Harness builds on llm 0.15.0
summary: Seven llm crates move from 0.8.1 to 0.15.0 without the b10x- package prefix; bedrock-converse is refused by name
relations:
- decomposes: epic:harness-on-llm-and-tools
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: Cargo.lock
- confidence: cited
  path: Cargo.toml
- confidence: cited
  path: crates/harness-llm/src/lib.rs
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-10-26T10:45:03Z", actor: "human:timo", revision: 6}
- {from: "proposed", to: "active", at: "2026-10-26T10:45:03Z", actor: "human:timo", revision: 7}
- {from: "active", to: "implemented", at: "2026-10-26T11:43:38Z", actor: "human:timo", revision: 8, decided_on: {"recorded":{"test_result":1,"review_outcome":1}}}
---
## Context

llm released `0.15.0` on 2026-10-26 (tag on `84ded0f2`). Its Messages client mints one
`x-opencode-session` header per client when the base URL is OpenCode Go (llm `CHANGELOG.md`
§ 0.15.0). Harness pins `0.8.1` (`Cargo.toml:74-80`). Between the two, llm 0.10.0 dropped the
`b10x-` prefix from every package, and 0.11.0 added `Protocol::BedrockConverse`, which makes the
exhaustive match in `crates/harness-llm/src/lib.rs` (`client`) fail to compile.

Approved as dispatch-queue item 7 by session `b10x-chef-orchestrator`, 2026-10-26.

## Outcome

Harness builds on llm 0.15.0: every model call goes through the 0.15.0 client crates, and a
`bedrock-converse` target is refused by name like `chat-completions`.

## Acceptance

`cargo xtask gate` exits 0 on a tree whose `[workspace.dependencies]` pin the seven llm crates as
`llm-* = { git = "https://github.com/beyond10x/llm", tag = "0.15.0" }` with no `package =` key, and
whose `Cargo.lock` holds no `b10x-llm-*` package.

## Out of scope

- A new provider-wire contract version (invariant 14: wire bytes are llm's).
- Consumers' pins (metaharness admits the new tag in its own step).
