---
format: aep.planning-md/1
id: story:public-reference-covers-the-shipped-surface
kind: story
status: implemented
title: The public reference covers the shipped surface
summary: The public CLI, status, and wire reference match the released command and evidence.
tags:
- contract
- docs
relations:
- derived_from: epic:public-site-is-accurate-live-and-governed
- serves: vision:b10x-owns-its-loop
revision: 5
---
# Story: The public reference covers the shipped surface

## Outcome

A builder can find every 0.6.0 command option and read evidence claims at their actual strength.

## Context

`--driver` and `--instructions-file` are absent, while live-provider and provider-emulated evidence are conflated or stale.

## Acceptance

- Every generated subcommand and long option appears in the CLI reference, including the two missing options.
- Status says both subscription routes have authorised live runs while current contract pins remain provider-emulated.
- Existing public routes continue to build and resolve.
- Public references link public release/source material only, never internal designs, plans, reviews, STATUS, ROADMAP, or Atlas.

## Out of Scope

No duplication of clap's full help prose and no contract cut.

## Open Questions

None.
