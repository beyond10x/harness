---
format: aep.planning-md/1
id: epic:subscription-auth-finished
kind: epic
status: draft
title: Subscription authentication is finished on both routes
summary: 'Phase 4''s two open halves: token renewal, and an authorized run on the ChatGPT/Codex route.'
relations:
- decomposes: initiative:live-evidence
revision: 2
---
## Evidence

- `ROADMAP.md:92-98` — Phase 4: "**Status: the source exists and the Anthropic route is authorized; renewal and the ChatGPT/Codex run do not.**"
- `ROADMAP.md:116-121` — what is not done, "stated so nobody reads the absence as working": renewal, and the ChatGPT/Codex authorized run.
- `ROADMAP.md:142-143` — "**Exit:** one authorized run on each, with the credential never leaving the source that owns it. Anthropic: met. ChatGPT/Codex: not met."
- `STATUS.md:20` — "**a `BearerSource` exists; it does not renew.** … Nothing here holds a refresh token or calls an authorization server, so a token nobody renews expires and the run fails by name."
- `AGENTS.md:121-126` — the safety envelope: a credential is fetched from an injected `BearerSource` at call time, there is no ambient fallback, and none may ever be added.

## Outcome

A subscription-credentialled run on either vendor route survives longer than one token lifetime, and
both routes have been contacted with the credential presented the way this harness presents it.

## Scope

Renewal against whatever holds the refresh token, and one authorized ChatGPT/Codex run whose
evidence is retained. Both stories carry the same constraint: no default path, no vendor directory,
no fallback when a named source is missing (`ROADMAP.md:103-107`).

## Risks

`STATUS.md:20` and `STATUS.md:21` disagree about whether the ChatGPT/Codex route has ever been
contacted. Whichever is right, one of the two pages is telling a reader something false about a
credential path — see `story:chatgpt-codex-authorized-run`.

## Done When

Phase 4's exit line reads "met" for both routes, against retained evidence, and a run whose token is
renewed by its owner outside this process keeps working across the renewal.
