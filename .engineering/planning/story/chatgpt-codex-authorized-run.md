---
format: aep.planning-md/1
id: story:chatgpt-codex-authorized-run
kind: story
status: implemented
title: The ChatGPT/Codex route has an authorized run whose evidence is retained
relations:
- derived_from: epic:subscription-auth-finished
- serves: vision:b10x-owns-its-loop
revision: 6
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

## Outcome — 2026-08-30

**Done, and the reconciliation in *Context* was the right one.** The 2026-08-23 run reached the
endpoint; what had never happened was a run through `SubscriptionToken`. Both have now happened.

| | |
|---|---|
| endpoint | `https://chatgpt.com/backend-api/codex`, wire `openai-responses`, model `gpt-5.6-sol` |
| credential | `--oauth-token-file ~/.codex/auth.json --oauth-token-pointer /tokens/access_token` |
| result | 2 turns, `file_read` called and answered, `finished{completed}`, exit 0 |
| usage | 2062 input, 28 output, 0 cached |
| session | `18d066fc428e5e98-0003a176` |
| record | `credential_source: "named"` — the operator typed the source, so no provider was consulted |
| control | same endpoint and model, unparseable token → `401 unauthorized_unknown`, *"Could not parse your authentication token"*, exit 1 |

The control is the part that makes the success mean anything: an endpoint that answered 200 to
anything would have answered 200 to this. It did not.

`STATUS.md`'s two rows now agree, and say which sense of *contacted* each means.

**No code changed.** `ROADMAP.md:119-121` predicted that — *"It needs no new code at all: it takes
its access token as a plain bearer, which `StaticBearer` already does."* That prediction is now
measured rather than reasoned, which was the whole of what this story asked for.

### What this unblocks, and what it deliberately does not

`story:codex-provider`'s missing fact is now measured: the credential lives at `~/.codex/auth.json`
at pointer `/tokens/access_token`, `auth_mode: "chatgpt"` — the exact shape the `claude` entry takes.

The operator has held that story anyway, and the reason is in this run's own evidence: that file
also holds a `refresh_token`. A typed flag is the operator pointing at it. A built-in provider
naming it is the binary reaching into a vendor directory for a file carrying more than it reads,
which is the softening `AGENTS.md:121-126` exists to bound. Measuring the path did not answer that
question; it only removed the excuse for not asking it.
