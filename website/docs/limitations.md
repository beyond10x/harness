---
title: Limitations
sidebar_position: 91
description: Which claims the evidence supports, what Harness does not do, how stable its contracts are, and which component to use instead.
---

# Limitations

Harness is pre-v1. [Status](./status.md) lists what is shipped, capability by capability, with the
test that holds each claim. This page says what that evidence does not establish.

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

## What Harness does not do

- **Linux builds only.** Each release packages Linux x86_64 and aarch64 binaries. Check the
  [release page](https://github.com/beyond10x/harness/releases) for the assets and checksums of the
  version you choose. Source builds need Rust 1.98 and Git; the foundation crates are public Git
  repositories pinned by tag or revision, and no crate is published on crates.io. The embedded host
  does not target macOS.
- **No hosted service.** Admission, tenancy, scheduling, durable storage, and deployment are outside
  this repository.
- **No multimodal input.** Harness owns text turns. Outbound MCP tools are available only through a
  reviewed profile that pins the local registry and exact discovery snapshot; server annotations do
  not grant Harness authority.
- **No realtime media or provider-side session state.** Local sessions replay a stateless
  conversation.
- **Delegation is depth one.** Neighbouring delegates may run concurrently only on a non-mutating,
  approval-free surface with no hooks; delegates cannot create delegate trees.
- **The skill and agent frontmatter reader is deliberately small.** Top-level `key: value` only;
  any other key refuses the run by name rather than being skipped.
- **Memories are read-only, and there is no writer.** `--memory-dir` is the whole of how a memory
  reaches a run: no default vault, no environment variable, no discovery. `recall` returns a body
  the caller loaded before the run. No tool and no flag writes a memory, deliberately, and a writer
  would be its own change with its own gate. A malformed or partly invalid vault refuses the whole
  vault by name rather than yielding a smaller one.
- **Hooks are host programs, not sandboxed tools.** They must be explicitly named and trusted.
- **Bridge mode lacks live external-client evidence.** The implemented profile is contract-tested,
  but no real external bridge has driven it yet.
- **Confinement is host-dependent.** A missing kernel, cgroup, or substrate capability withholds the
  related tool rather than emulating it.
- **No production embedder yet.** The library boundary exists, but no other component embeds it in
  production.

## Stability

Released contract versions are immutable. A changed provider request, accepted stream, app-server
profile, or argv surface cuts a new versioned contract rather than rewriting the old one. Released
means reachable on `main`, not tagged and not out of the changelog's `[Unreleased]`, and a second
cut on one day takes a `.N` suffix. [The CLI reference](./reference/cli.md) names the contract the
current command line is pinned as.

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
