---
format: aep.planning-md/1
id: story:oauth-token-renewal
kind: story
status: draft
title: A run outlives the token it started with
relations:
- derived_from: epic:subscription-auth-finished
revision: 3
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

## The open question is answered — 2026-08-30

**Yes, renewal belongs here, and only for a provider whose renewal facts have been measured.** The
operator decided it while `story:codex-provider` was being implemented, and that story records what
shipped: `harness_credential::renew_if_stale`, a `Renewal` on the `codex` provider entry, an atomic
byte-preserving write back into `~/.codex/auth.json`, and a `credential-renewed` event.

What that answer costs is stated there rather than assumed: the harness now reads a field of a
vendor's credential file it does not send, and writes to a file another program owns. The bound is
that it happens **only** for a credential the provider itself defaulted — a source the operator
typed is read and never written — and that both the intent and the act are readable, before
(`providers show codex`) and after (`credential-renewed`).

## What is left, and it is this story's original sentence

**Nothing renews mid-run.** The check is once, before the first request, against a fifteen-minute
margin. A run that starts with a fresh token and lasts longer than that token still fails partway
through, which is exactly the case this story is titled after.

Two candidate answers, neither built:

- **Renew on the far side's refusal.** A `401` from the model endpoint is the only authority on
  whether a token is actually dead; renewing there and retrying the turn once is the smallest thing
  that survives a long run. It needs a renewal handle reachable from inside a turn, which is
  precisely the coupling `SubscriptionToken` refuses today — a bearer source that could quietly
  rewrite somebody's disk mid-turn, with nothing in the record saying it had, is worse than the
  failure it prevents. Any version of this has to emit the event from where the loop can see it.
- **Refuse at the start of a turn that cannot finish**, rather than mid-stream: check the expiry
  before each turn and stop with a message naming the credential and the expiry, so the failure is
  legible and unpaid instead of arriving as a vendor `401` halfway through a stream.

## Acceptance

A run whose subscription token expires mid-run either continues after the token is renewed — by its
owner or by the run — or stops with a message that names the credential and the fact that it
expired, before the turn is paid for. `ROADMAP.md`'s Phase 4 renewal bullet states which was chosen
and why, and is corrected where it now says nothing here calls an authorization server: as of
2026-08-30 something does.
