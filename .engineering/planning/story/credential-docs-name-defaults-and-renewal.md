---
format: aep.planning-md/1
id: story:credential-docs-name-defaults-and-renewal
kind: story
status: implemented
title: Credential docs name defaults and renewal
summary: Every public credential claim distinguishes named sources, provider defaults, and bounded renewal.
tags:
- docs
- security
relations:
- derived_from: epic:public-site-is-accurate-live-and-governed
- serves: vision:b10x-owns-its-loop
revision: 5
---
# Story: Credential docs name defaults and renewal

## Outcome

An operator can tell before a run which credential source will be read and whether Harness may rewrite it.

## Context

The public security and quickstart prose deny provider defaults and renewal that the provider guide and code ship.

## Acceptance

- Quickstart, security, wire, provider, CLI, status, and landing claims distinguish named sources from provider-declared sources.
- `providers show` is the inspection step before a built-in provider is selected.
- Only provider-defaulted `codex` renewal is described as a pre-run atomic rewrite; explicitly named sources and `claude` are never described as renewable.
- No credential value, prefix, length, digest, or internal Atlas material reaches the site.

## Out of Scope

No credential runtime change or mid-run renewal.

## Open Questions

None.
