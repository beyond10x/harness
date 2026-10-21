---
format: aep.planning-md/3
id: story:api-key-from-the-secrets-store
kind: story
status: draft
title: An API key is read from the local secrets store by reference
summary: '`--api-key-secret <namespace>/<name>` resolves a provider key through llm-credentials'' secrets resolver, so no key is stored in a file'
relations:
- serves: vision:b10x-owns-its-loop
revision: 1
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
