---
format: aep.planning-md/3
id: review-result:security-secrets-0171-pass-1
kind: review-result
status: active
title: Security review on story:api-key-from-the-secrets-store (wave secrets-0171, before 9389c693)
relations:
- reviews: story:api-key-from-the-secrets-store
revision: 1
---
Security review over the unit at the uncommitted tree before 9389c693 (2026-10-22).

| id | severity | file:line | outcome |
|---|---|---|---|
| S1 | note | zbus-secret-service-keyring-store 1.0.1 `src/service.rs:41` | documented in README: every store read runs `unlock_all` on locked items, so a locked collection may prompt on each request with no deadline but cancel. Not observed: real D-Bus was out of bounds |

Held, with evidence:
- No ambient fallback: `Source::secret` has two callers, both behind `api_key_secret.is_some()` (`crates/harness-cli/src/lib.rs` `resolve_credential`, `app_server_command`); `apply_provider`'s `named_already` includes the flag; no `env =` binding; 0 session-bus connections without the flag (`crates/harness-cli/tests/secrets_security_cli.rs`).
- Per-request read, zeroized copies through the keychain backend (`Zeroizing`, `SecretValue`, `Secret`); the one unzeroized copy is llm's `HeaderValue`, the same for every source.
- No error carries a value: `SecretError` messages are fixed; keyring and D-Bus errors are dropped unformatted.
- Dependencies: only `harness-llm` depends on `b10x-llm-*`; secrets crates locked at `tag=v0.5.0#8c4eabb1`; 52 packages added, none changed; `keyring-core` dev-dependency is the same 1.0.1 llm-credentials links.
- Released `contracts/cli/` versions byte-identical to origin/main.

Tests: 6 in `crates/harness-llm/tests/secrets_security.rs`, 1 in `crates/harness-cli/tests/secrets_security_cli.rs`, all green.
