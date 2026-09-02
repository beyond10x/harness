---
title: Security boundary
description: Credentials, transcripts, tools, hooks, and the boundary Harness does and does not provide.
---

# Security boundary

Harness controls the agent loop. It is not by itself a sandbox, a credential broker, or a
multi-tenant service. This page separates the protections it provides from the responsibilities of
the system around it.

## Credential sources are inspectable

An explicit command-line source is always honored as written:

- `--api-key-file` or `--api-key-env` for a bearer API key;
- `--oauth-token-file` or `--oauth-token-env` for a subscription OAuth token;
- `--oauth-token-pointer` when that OAuth source is a JSON document.

Harness does not search arbitrary environment names or vendor directories. A selected built-in
provider can, however, name a documented credential default. `providers show <name>` prints that
source before a request, and the run records `credential_source: "provider:<name>"` rather than
hiding the default behind `named`.

Explicit OAuth files are re-read on every model call so a separate owner can rotate them; Harness
never writes one. The `codex` provider is the narrow exception: when it supplied its own default
source and that token is near expiry, Harness can renew it and atomically rewrite that provider
store before the first request. The write is reported on stderr and as `credential-renewed`, even
under `--quiet`. See [Configuration reference](../reference/configuration.md).

Credentials are not written to sessions or added to errors. A credential source is asked at call
time, and the short-lived bearer value has redacted debug output.

## Model visibility

The selected model endpoint receives the standing instruction, conversation, published tool
definitions, and the contents returned by tools. Treat any workspace content read during a run as
data that may leave the machine for that endpoint.

`--context FILE` gives a file to the model before it starts. It may save a discovery turn, but its
contents are replayed on every stateless provider turn. Use it deliberately.

## Sessions contain workspace material

Sessions are plain JSON files containing the conversation, opaque provider items, usage, and cost.
They contain no credential and no stored instruction text, but they can contain every source file or
tool result the model saw.

By default they live outside the workspace under the user's state directory and a directory Harness
creates is mode `0700` on Unix. An existing directory keeps the mode its operator assigned.

Use `--no-session` for evaluation arms, sensitive work, or ephemeral environments that must retain
nothing. If you set `--session-dir`, do not point it inside a repository that might be committed.

## Read containment and write confinement

The built-in read tools stay beneath the canonical workspace root. They re-check resolved paths and
refuse a symlink escape.

Write and execution tools are not available until a named
[substrate](https://github.com/beyond10x/substrate) boundary admits them. Substrate owns guarded IO,
process namespaces, cgroups, and the capability probe. Harness owns which of those admitted
operations reaches the model and which calls require approval.

This division matters: a prompt saying “do not write” is not confinement, and an approval prompt is
not a filesystem boundary.

## Hooks are trusted operator programs

`--hooks FILE` names programs supplied by the operator. Hook commands run directly as an argv,
never through a shell, but they run outside the workspace confinement with the environment Harness
inherited.

Harness removes the environment variable named as this run's credential source before starting a
hook. It does not otherwise sandbox hooks. A hook can narrow the loop by blocking a call or asking it
to continue; it cannot add a tool or approve a call the regular gate rejected.

Never discover hook files from the workspace. A repository-controlled hook would be a program the
repository causes to run on the operator's machine.

## Skills and agents are files, not remote authority

`--skills-dir`, `--agents-dir` and `--plugin-dir` read a directory the operator named, once, before
the first request. Nothing opens a socket and nothing gives a third party a say in what the run may
do. A document this build cannot read refuses the run by name rather than being half-applied, and
an agent's `tools:` can only narrow the parent's catalogue. A skill body is an instruction the
model follows, so a directory under a repository's control puts that repository's words in the
run: name what you trust, as with hooks.

Outbound MCP is different: it opens a remote protocol connection. Harness therefore publishes
only entries in an operator-authored profile that pins both the shared local registry and the exact
`tools/list` snapshot. The profile supplies the local risk, effect, access and subject claims;
server annotations are preserved as remote data and never become Harness authority.

## Multi-tenant use

Embedded substrate is intended for an operator's own process. It has no peer identity. A
multi-tenant service needs the substrate daemon boundary, authenticated callers, admission policy,
and durable audit/storage components around Harness. Those are not provided by this repository.

## Deployment checklist

Before using Harness for consequential work, answer these questions explicitly:

- Which endpoint receives workspace content, and under which data-retention policy?
- Which credential source is explicit or provider-defaulted, who can inspect it, and who rotates it?
- Are sessions permitted, where are they written, and how are they removed?
- Which capabilities does `b10x-harness tools` say this machine actually admitted?
- Which risk ceiling and approver apply to unattended calls?
- Which paths and executable programs are allowed?
- Are hooks present, and are those programs trusted at host privilege?
- Which turn, token, duration, context, and cost ceilings bind the run?
