---
title: Status and limitations
description: What Harness 0.1.0 can do today and which claims are not yet earned.
---

# Status and limitations

Harness is pre-v1. Version `0.1.0` was tagged on 2026-08-24; this documentation also describes the
unreleased work present on `main` as of 2026-08-29.

## Available now

| Area | Current state |
|---|---|
| Provider turns | Streaming OpenAI Responses and Anthropic Messages projections |
| Agent loop | Turn assembly, tool round trips, approvals, cancellation, compaction, and budgets |
| Workspace tools | Bounded reads everywhere; writes and argv execution behind substrate capabilities |
| Command line | `run`, `chat`, `sessions`, `tools`, `app-server`, and `events` |
| Persistence | Local, atomic session files with resume and usage/cost retention |
| Machine output | Vendor-neutral JSONL events and structured object output |
| Advanced loop | Opt-in depth-one delegation and operator hooks |
| Contracts | Pinned provider requests/streams, app-server profile, and generated argv surface |

The full Rust workspace gate tests, formats, lints, and checks all three contract families. The two
provider wires run the same scenario suite against deterministic local HTTP endpoints.

## Evidence labels

Most model-wire evidence is `provider_emulated`: a deterministic local server is contacted over a
real socket. This supports claims about bytes, bounds, retries, and loop behavior. It does not prove
conformance with every live provider deployment.

Limited live-provider runs have happened and already found behavior the emulator did not: the first
live run rejected the original workspace tool names on turn one. Live characterization remains a
work item rather than a completed compatibility claim.

## Important limitations

- **No public binary distribution.** Build from a source checkout; the crates are not published.
- **No hosted service.** Admission, tenancy, scheduling, durable storage, and deployment are outside
  this repository.
- **No MCP client or multimodal input.** Harness currently owns text turns and its built-in or bound
  tool port.
- **No realtime media or provider-side session state.** Local sessions replay a stateless
  conversation.
- **Structured output is not independently schema-validated by the loop.** The schema is offered as
  the `answer` tool input shape.
- **Delegation is depth one and sequential.** Delegates cannot create delegate trees or run in
  parallel.
- **Hooks are host programs, not sandboxed tools.** They must be explicitly named and trusted.
- **Bridge mode lacks live external-client evidence.** The implemented profile is contract-tested,
  but no real external bridge has driven it yet.
- **Bridge compaction does not yet receive the command line's `--context-window`.** It retains the
  older fixed byte rule.
- **Confinement is host-dependent.** A missing kernel, cgroup, or substrate capability withholds the
  related tool rather than emulating it.
- **No production embedder yet.** The library boundary exists, but no other component currently
  embeds it in production.

## Stability

Released contract versions are immutable. A changed provider request, accepted stream, app-server
profile, or argv surface cuts a new versioned contract rather than rewriting the old one.

The Rust library APIs are not yet promised stable. Before upgrading, read the repository
[changelog](https://github.com/beyond10x/harness/blob/main/CHANGELOG.md) and regenerate any
command-line or event consumers against the build you will deploy.

## What belongs elsewhere

| Need | Component |
|---|---|
| Drive and compare vendor coding harnesses | [metaharness](https://beyond10x.github.io/metaharness/) |
| Confine files and processes | [substrate](https://github.com/beyond10x/substrate) |
| Route and terminate model requests | [llmgw](https://github.com/beyond10x/llmgw) |
| Principal identity and audiences | [identity](https://github.com/beyond10x/identity) |
| Durable event-sourced state | [eventlog](https://github.com/beyond10x/eventlog) |
