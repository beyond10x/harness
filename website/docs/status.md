---
title: Status and limitations
description: What Harness 0.11.1 can do today and which claims are not yet earned.
---

# Status and limitations

Harness is pre-v1. Version `0.11.1` was tagged on 2026-09-03; this documentation describes that
release.

## Available now

| Area | Current state |
|---|---|
| Provider turns | Streaming OpenAI Responses and Anthropic Messages projections |
| Agent loop | Turn assembly, tool round trips, approvals, cancellation, compaction, and budgets |
| Workspace tools | Bounded reads everywhere; file writes and argv execution behind substrate capabilities; child processes are workspace-read-only by default and receive only explicitly named writable subtrees |
| Command line | `run`, `chat`, `workflow plan`, `workflow run`, `sessions`, `tools`, `context show`, `app-server`, and `events` |
| Persistence | Local, atomic session files with resume and usage/cost retention |
| Machine output | Vendor-neutral JSONL events and structured object output |
| Advanced loop | Opt-in depth-one delegation, named agents and skills read from a Claude Code plugin layout, and operator hooks |
| Workflows | A YAML or JSON flow walked one model turn per step, one session per section, with a `transition` hook asked at every section boundary |
| Contracts | Pinned provider requests/streams, app-server profile, and generated argv surface |

The full Rust workspace gate tests, formats, lints, and checks all three contract families. The two
provider wires run the same scenario suite against deterministic local HTTP endpoints.

## Evidence labels

The current pinned request and stream fixtures are `provider_emulated`: a deterministic local
server is contacted over a real socket. This supports claims about exact bytes, bounds, retries,
and loop behavior. Those pins have not been promoted to `vendor_live`.

Both subscription routes do have narrower, dated authorized observations. On 2026-08-29 an
Anthropic Messages run completed a three-turn tool-using conversation against
`api.anthropic.com`. On 2026-08-30 an OpenAI Responses run completed a two-turn tool-using
conversation against the ChatGPT/Codex endpoint. Invalid-token controls on each route returned
authorization failures. The first live route observation also exposed invalid workspace tool names
that the emulator had accepted. These runs establish that the named routes authenticated and
completed on those dates; they do not turn emulator-derived fixture bytes into live captures or
promise conformance with every deployment.

On 2026-08-31 an authorized Anthropic Messages run also exercised the embedded confined path in a
delegated cgroup. It used the admitted Go toolchain to build, format, test and vet a scratch todo
server and web frontend; independent host checks then passed its frontend, health route and CRUD
API. This is evidence for that live route and confinement path, not a promotion of the pinned wire
fixtures.

## Important limitations

- **No public binary distribution.** Build from a source checkout; the crates are not published.
- **No hosted service.** Admission, tenancy, scheduling, durable storage, and deployment are outside
  this repository.
- **No multimodal input.** Harness currently owns text turns. Outbound MCP tools are available only
  through a reviewed profile that pins the local registry and exact discovery snapshot; server
  annotations do not grant Harness authority.
- **No realtime media or provider-side session state.** Local sessions replay a stateless
  conversation.
- **Delegation is depth one.** Neighbouring delegates may run concurrently only on a non-mutating,
  approval-free surface with no hooks; delegates cannot create delegate trees.
- **The skill and agent frontmatter reader is deliberately small.** Top-level `key: value` only;
  any other key refuses the run by name rather than being skipped.
- **Hooks are host programs, not sandboxed tools.** They must be explicitly named and trusted.
- **Bridge mode lacks live external-client evidence.** The implemented profile is contract-tested,
  but no real external bridge has driven it yet.
- **Confinement is host-dependent.** A missing kernel, cgroup, or substrate capability withholds the
  related tool rather than emulating it.
- **No production embedder yet.** The library boundary exists, but no other component currently
  embeds it in production.

## Stability

Released contract versions are immutable. A changed provider request, accepted stream, app-server
profile, or argv surface cuts a new versioned contract rather than rewriting the old one. Released
means reachable on `origin/main` — not tagged, and not out of the changelog's `[Unreleased]` — and
a second cut on one day takes a `.N` suffix. The current CLI contract is `2026-09-02`.

The Rust library APIs are not yet promised stable. Before upgrading, read the repository
[changelog](https://github.com/beyond10x/harness/blob/main/CHANGELOG.md) and regenerate any
command-line or event consumers against the build you will deploy.

## What belongs elsewhere

| Need | Component |
|---|---|
| Drive and compare vendor coding harnesses | [metaharness](https://beyond10x.github.io/metaharness/) |
| Confine files and processes | [substrate](https://github.com/beyond10x/substrate) |
| Route and terminate model requests | llmgw (not a public surface) |
| Principal identity and audiences | identity (not a public surface) |
| Durable event-sourced state | eventlog (not a public surface) |
