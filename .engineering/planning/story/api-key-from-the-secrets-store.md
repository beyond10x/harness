---
format: aep.planning-md/3
id: story:api-key-from-the-secrets-store
kind: story
status: implemented
title: An API key is read from the local secrets store by reference
summary: '`--api-key-secret <namespace>/<name>` resolves a provider key through llm-credentials'' secrets resolver, so no key is stored in a file'
relations:
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: Cargo.lock
- confidence: cited
  path: contracts/cli/b10x-harness/2026-10-22
- confidence: cited
  path: crates/harness-cli/src/contract.rs
- confidence: cited
  path: crates/harness-cli/src/lib.rs
- confidence: cited
  path: crates/harness-cli/tests/secrets_adversary_cli.rs
- confidence: cited
  path: crates/harness-cli/tests/secrets_security_cli.rs
- confidence: cited
  path: crates/harness-llm/Cargo.toml
- confidence: cited
  path: crates/harness-llm/src/lib.rs
- confidence: cited
  path: crates/harness-llm/tests/secrets_adversary.rs
- confidence: cited
  path: crates/harness-llm/tests/secrets_security.rs
- confidence: cited
  path: website/docs/concepts/security-boundary.md
- confidence: cited
  path: website/docs/reference/cli.md
revision: 9
transitions:
- {from: "draft", to: "proposed", at: "2026-10-22T00:02:57Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-10-22T00:02:57Z", actor: "human:timo", revision: 4}
- {from: "active", to: "implemented", at: "2026-10-22T02:02:59Z", actor: "human:timo", revision: 9, decided_on: {"recorded":{"test_result":1,"review_outcome":2}}}
---
## Context

An API key is today named on the command line as a file (`--api-key-file`,
`crates/harness-cli/src/lib.rs:506`) or an environment variable (`--api-key-env`). The operator
does not want a provider key stored in a file (operator, 2026-10-22: "i dont want to store it in
file - it should go to os keychain"). The local secrets store already holds such keys under a
namespace and a name, and `llm-credentials` has a resolver for it behind its `secrets` feature
(`crates/llm-credentials/src/secrets.rs` in the `llm` repository, over `secrets-core` 0.5.0).
`crates/harness-llm/Cargo.toml:21` enables `codex-renewal`, `environment`, `file` and
`json-pointer` and not `secrets`.

The first caller is a trial arm that reads the key `llm/opencode-go-api-key`.

## Acceptance

- `b10x-harness run --api-key-secret <namespace>/<name>` takes the API key from the local secrets
  store by reference, resolved per request through `llm-credentials`' `secrets` resolver, with
  the same lifetime rules as every other `Source` (AGENTS.md § Safety envelope, *Credentials*):
  fetched at call time, zeroized when the call ends, never in a config struct or an error.
- The flag conflicts with `--api-key-file`, `--api-key-env` and the OAuth sources, as they
  conflict with each other today.
- A reference that is not `<namespace>/<name>`, or a secret the store does not hold, is refused
  by name: the refusal names the reference and never a value.
- There is no ambient fallback: without the flag the store is never read.
- The command-line surface change cuts a new `contracts/cli/b10x-harness/<date>` version
  (invariant 13); the released versions stay byte for byte.
- A test against a store fixture the test creates (no real keychain) proves the value reaches the
  request header and is absent from every refusal and from `Debug`.

## Out of scope

- Writing a secret: `secretsctl put` stays the only writer.
- Reading Secret Service attributes directly.

## Release target

On 2026-10-22 the orchestrator asked for this story to be pulled into 0.17.0, as the first link of the
trial arm chain (harness release, then a metaharness story, then a trial story), and allowed at most
one extra gate round. It needs more than one gate round: a new git dependency (`beyond10x/secrets`
v0.5.0 through `llm-credentials`' `secrets` feature, `llm` 0.8.1), a new `contracts/cli` version and
an adversary pass over a credential path. It ships as 0.17.1, the release after 0.17.0.

## Delivery

Shipped in 0.17.1 (tag object 243dae18 on c5cf4de5, 2026-10-22): unit 9389c693, merge da4dbd58,
release commit 6e10881e, clippy fix c5cf4de5.

- The first 0.17.1 gate (on 6e10881e) failed clippy `too_many_lines` (103/100) in
  `crates/harness-cli/tests/secrets_security_cli.rs`; c5cf4de5 moved the case's sandbox into a
  struct, and the second gate passed (1181 passed, 0 failed, 6 ignored).
- Disk: during the gate the orchestrator held all cargo at 14G free on `/`, then sent a go at 46G
  free for the 0.17.1 gate in harness-w0171-secret, one cargo run at a time (2026-10-22, ~04:0x).
- The unit's first bot commit was refused by the common checks: gitleaks `generic-api-key` matched
  a synthetic environment-variable name in `secrets_adversary_cli.rs`; it moved into a named
  constant before 9389c693.
- Not covered and not run: a read of a real keychain entry, a namespace `secretsctl` mounted on
  another backend (refused as not found), a locked collection (may prompt per request).
