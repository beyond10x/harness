---
format: aep.planning-md/1
id: story:chatgpt-codex-authorized-run
kind: story
status: draft
title: The ChatGPT/Codex route has an authorized run whose evidence is retained
relations:
- derived_from: epic:subscription-auth-finished
revision: 2
---
## Evidence

- `ROADMAP.md:142-143` — "**Exit:** one authorized run on each … Anthropic: met. ChatGPT/Codex: not met."
- `ROADMAP.md:119-121` — "**the ChatGPT/Codex authorized run.** That route has not been contacted. It needs no new code at all: it takes its access token as a plain bearer, which `StaticBearer` already does — what is missing is the run that says so."
- `STATUS.md:20` — "The ChatGPT/Codex route still has not been contacted."
- `STATUS.md:21` — and in the row directly below it: "**first live run: 2026-08-23**, against `https://chatgpt.com/backend-api/codex` under the operator's own ChatGPT subscription credential, model `gpt-5.6-sol`. Two turns, two tool round trips, usage reported, `finished{completed}`."
- `AGENTS.md:121-126` — the credential never leaves the source that owns it; no ambient fallback.

## Context

Two rows of one table disagree about whether this route has ever been contacted. The likely
reconciliation is that the 2026-08-23 run presented a hand-extracted token as a plain bearer, and
what Phase 4 means by *authorized* is a run through `SubscriptionToken` with the credential read from
a named source — but that reading is not written anywhere, and a plan cannot be built on it.

Either way the same run closes it: one run on that route, credential read from a source the operator
named, with the discrimination control the Anthropic run used — a deliberately invalid token to the
same endpoint, so a 200 is the credential's and not the endpoint's indifference (`STATUS.md:20`).

**Overlaps** `story:codex-provider`, which is about a `codex` entry in the provider table and is
blocked on reading a credential location off a working install. This story is the run; that story is
the config. The measurement that unblocks that story is a by-product of this one.

## Acceptance

One completed run against the ChatGPT/Codex route with the credential read from a named source, its
evidence retained as `vendor_live`, a failed control against the same endpoint recorded beside it,
and `STATUS.md`'s two rows agreeing about what has been contacted.
