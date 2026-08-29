---
format: aep.planning-md/1
id: story:codex-live-refresh-measured
kind: story
status: draft
title: A successful refresh against the live authorization server is measured
relations:
- derived_from: epic:subscription-auth-finished
- depends_on: story:codex-provider
revision: 1
---
## What is unmeasured

The `codex` provider renews a stale token by presenting the refresh token beside it to
`https://auth.openai.com/oauth/token`. **No successful exchange with that server has been observed.**
Everything around it has been:

- `crates/harness-credential/src/renewal.rs`,
  `a_stale_document_is_renewed_written_back_byte_for_byte_and_reported` — the whole act against a
  real socket on `127.0.0.1`: what is sent, what comes back, and every byte of the document
  afterwards. It proves this build's half.
- The unpaid control recorded on `story:codex-provider` — the real `client_id` and a deliberately
  invalid refresh token to the real endpoint answered `401 token_expired`. It proves the URL exists,
  recognises the client and speaks this grant.
- `crates/harness-cli/tests/end_to_end.rs` and a smoke run under a synthetic `$HOME`: the CLI
  detected staleness, fired the renewal, surfaced the vendor's own `401` and exited 1 without
  touching the document.

What is left is the one case the control cannot stand in for: **a 200 from that server, carrying an
`access_token` this build then writes back.** A refusal proves the endpoint is discriminating; it
does not prove the success path, and the success path is where the write happens.

## Why it has not been run

It is a paid step with a side effect on the operator's own credential. OpenAI rotates the refresh
token on use, so the run retires the one in `~/.codex/auth.json` and the recovery for a failure is
`codex login`, not restoring a backup — the backup's refresh token is dead too. That makes it the
operator's to run rather than an agent's.

It is also not currently reachable without contrivance: the token in that file expires roughly ten
days after it is issued (`iat` 1788007151 → `exp` 1788871151), and the renewal margin is fifteen
minutes, so the path only opens near the end of that window.

## Acceptance

One renewal against the live authorization server that returns `200`, observed end to end:

1. `credential-renewed` in the run's record, naming `~/.codex/auth.json` and a new `expires_unix`
   later than the one it replaced;
2. `~/.codex/auth.json` still parses, still holds `auth_mode`, `account_id` and every other key it
   had, and `codex` itself still authenticates against it afterwards — the byte-preserving claim
   checked against the program that owns the file rather than against a fixture;
3. `refresh_token_rotated` recorded either way, so whether that vendor rotates is a measured fact
   rather than an assumption this build carries;
4. the observation retained as `vendor_live`, distinct from the `provider_emulated` socket test that
   stands in for it today.

If instead it returns a non-200, that is equally a result: it means the token endpoint or the client
id in `crates/harness-cli/src/provider.rs` is wrong for a real credential, and the entry must lose
its renewal until somebody measures the right one.
