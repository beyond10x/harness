---
format: aep.planning-md/1
id: story:codex-provider
kind: story
status: implemented
title: A codex provider, once its credential location has been measured
relations:
- derived_from: epic:adoption-follow-ups
- serves: vision:b10x-owns-its-loop
revision: 6
---
## What is missing

`b10x-harness` ships two providers, `claude` and `openai`. A `codex` provider is wanted so a
subscription to that vendor is one word of config, as `provider = "claude"` already is.

- `crates/harness-cli/src/provider.rs`, `built_in()` — where it would go.

## Why it was left

**Its credential location has never been read off a working install.** `claude`'s values are the
ones a live run actually used, taken from the eval that drives it; nothing equivalent exists for
codex. A provider naming a path that turns out to be wrong fails at the far side, where this build
can say nothing useful about why — and inventing a vendor path is the single mistake the provider
table exists to avoid, which its own module doc records.

`metaharness`'s codex adapter knows about `CODEX_HOME` and a config document inside it
(`crates/metaharness-codex/src/launch.rs:58-97`), but that is a scratch home the adapter *writes*,
not where a real installation keeps a subscription token.

## Acceptance

Someone reads the credential location off a working codex installation and records what they read —
the path, the document shape, and the pointer to the token inside it. The provider entry follows
from that in one commit, with a test that the file exists on a machine that has codex and skips
where it does not.

Until that measurement exists this story cannot be implemented, only guessed at.

## Outcome — 2026-08-30

**Shipped, and it does more than this story asked for**: the operator decided the provider should
also renew a stale token and write the new one back. That decision is recorded here rather than in a
new story, because it changes what a `codex` entry *is* and cannot be read apart from it.

### The measurement the story was blocked on

| fact | value | read from |
|---|---|---|
| endpoint | `https://chatgpt.com/backend-api/codex` | the completed run of `story:chatgpt-codex-authorized-run` |
| wire | `openai-responses` | the same run |
| model | `gpt-5.6-sol` | the same run |
| credential | `~/.codex/auth.json`, pointer `/tokens/access_token` | the same run |
| `auth_mode` | `"chatgpt"` | the document itself |

Nothing was guessed. `crates/harness-cli/src/provider.rs` `codex_tests` asserts each value and names
the run it came from, so editing one without a run behind it has to be argued there.

### The renewal, and the four measurements under it

The `codex` entry carries a `Renewal`: a token endpoint, a client id, and pointers to the refresh
token, the id token and the store's own `last_refresh` stamp.

| fact | value | how it was measured |
|---|---|---|
| issuer | `https://auth.openai.com` | the `iss` claim of the tokens in that file |
| token endpoint | `https://auth.openai.com/oauth/token` | the string the `codex` binary itself presents this token to (`/usr/bin/codex`, 0.145.0) |
| client id | `app_EMoamEEZ73f0CkXaXp7hrann` | the `client_id` claim of the access token, and the `aud` of the id token beside it |
| token lifetime | 10 days (`iat` 1788007151 → `exp` 1788871151) | the access token in that file |

The issuer's OIDC discovery document advertises `/api/accounts/oauth/token` as a second token
endpoint. The one above was chosen because it is the one the program that *wrote* the file uses;
following the writer is the safer of two measured answers.

**Unpaid control, 2026-08-30.** A POST to that endpoint with the real `client_id` and a deliberately
invalid refresh token answered `401` with
`{"error":{"code":"token_expired","message":"Could not validate your token. Please try signing in
again.","type":"invalid_request_error"}}`. That is the discrimination this project asks for: the URL
exists, recognises the client, speaks this grant, and refuses a bad token rather than answering
anything to anyone. **Not yet measured: that it accepts a live refresh token.** That is one paid
step and it rotates the operator's credential, so it is theirs to run.

### The write, and what bounds it

This is the harness writing to a file another program owns — a larger softening than the defaulted
credential path that preceded it. Four rules bound it, each with a test:

1. **Only a credential a provider defaulted.** `apply_provider` sets the renewal on the same branch
   that defaults the path, so `--oauth-token-file` and a `[providers.codex] oauth-token-file` both
   switch it off (`naming_your_own_credential_turns_the_renewal_off`).
2. **Atomic.** Temporary file beside the original, parsed back to check it carries the new values,
   then renamed. The original's mode is carried across, so a store is never widened by being
   renewed.
3. **Byte-preserving.** Only the token values are spliced; key order, indentation and unknown keys
   survive exactly. Where a splice cannot be proven safe the document is re-serialised and
   `byte_preserving: false` says so.
4. **No part of the credential is recorded** — not a prefix, not a length, not a digest, because a
   digest of a token is an oracle for it.

Staleness is the access token's own `exp`, decoded without verifying the signature: this is not
authenticating anybody, it is asking the credential when it expects to stop working. A token whose
`exp` cannot be read is left alone rather than guessed at.

### What is in the record

A new `credential-renewed` event, emitted **before** `started` because that is when it happened. It
names the file, the provider, the new expiry, whether the refresh token on disk was retired, and
whether the rewrite preserved every other byte. Printed on stderr even under `--quiet`: quiet asks
for less progress noise, not for a side effect on somebody's disk to go unmentioned.

`providers show codex` prints the file, the token endpoint, the client id and the refresh pointer
before anything is spent — the same rule that pays for a defaulted credential path, applied to a
larger claim.

### What this deliberately did not do

`claude` gained no renewal. Its credential file holds a refresh token, but the authorization server
and client that would accept it have not been read off anything here, and a guessed token endpoint
sends a live refresh token to a server nobody verified — the same mistake as a guessed credential
path, with a worse failure.

Nothing renews **mid-run**. The margin is fifteen minutes, checked once before the first request;
`story:oauth-token-renewal` still owns the case of a run that outlives the token it started with.
