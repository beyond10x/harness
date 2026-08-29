---
format: aep.planning-md/1
id: story:oauth-token-renewal
kind: story
status: draft
title: A run outlives the token it started with
relations:
- derived_from: epic:subscription-auth-finished
revision: 2
---
## Evidence

- `STATUS.md:20` — "**a `BearerSource` exists; it does not renew.** … There is still no default path and no vendor directory anything here looks in, and no fallback when the named source is missing"; next evidence: "**renewal** … Nothing here holds a refresh token or calls an authorization server, so a token nobody renews expires and the run fails by name."
- `ROADMAP.md:104-107` — what renewal is today: the token is re-read on **every** call rather than cached at construction, "which is the whole of the renewal story here: an owner outside this process that renews the token is followed on the next turn".
- `ROADMAP.md:117-119` — "**renewal.** Nothing here holds a refresh token or calls an authorization server."
- `AGENTS.md:121-126` — the credential is fetched at call time, `Bearer` has no `Display` and a redacted `Debug`, and "**There is no ambient credential fallback**: the harness reads nothing it was not pointed at. Never add one".

## Context

The harness already follows a token somebody else renews — it re-reads the named file or variable on
every call. What it cannot survive is a token nobody renews: a long run, or any run started with an
expired token, fails by name partway through and the operator's only recovery is to renew by hand and
start again.

The open question is deliberately not answered in the tree: whether renewal belongs here at all.
Holding a refresh token would make this component a client of an authorization server, and the
credential rules above are written to keep exactly that kind of reach out. The alternative — an
external renewer plus a named source — is what exists, and the work may be to say so where a reader
looking for renewal will find it, and to make the expiry failure legible before the turn is paid for.

## Acceptance

A run whose subscription token expires mid-run either continues after the token's owner renews it, or
stops with a message that names the credential and the fact that it expired — and `ROADMAP.md`'s
Phase 4 renewal bullet states which of the two was chosen and why.
