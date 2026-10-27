---
format: aep.planning-md/3
id: story:substrate-pin-0-12
kind: story
status: active
title: Harness embeds the newest Substrate release, and its specification runs on ESS 0.70.0
summary: substrate-host and substrate-wire move from 0.7.17 to the release after 0.12.0; the wire's additive fields are named; ESS 0.69.2 -> 0.70.0
tags:
- adr-0109
relations:
- serves: vision:b10x-owns-its-loop
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-27T08:58:29Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-27T08:58:29Z", actor: "human:timo", revision: 3}
---
## Context

The operator, 2026-10-27: "yes, we want to upgrade to latest substrate for: loom, takt, harness,
metaharness." Harness embeds Substrate's host driver through `crates/harness-substrate`, pinned at
package `0.7.17` (`00fa634c`). Substrate 0.12.0 ships the confinement contract
(`substrate_host::confinement`, ADR 0109) and the follow-up release after it carries the ADR 0109
follow-up stories. Between `0.7.17` and `0.12.x` the wire grew three additive fields a struct
literal must name (`ConfinementRequest::apertures`, `ExecStartInput::workdir` and
`ExecStartInput::writable_roots`), and `Driver::read_workspace_path` answers `Listed<T>`.

The workspace rule (2026-10-05, "ALWAYS upgrade to most recent ESS") moves the specification from
ESS 0.69.2 to 0.70.0 in the same change.

## Outcome

`b10x-substrate-host` and `b10x-substrate-wire` are pinned to the newest Substrate release, and the
embedded driver keeps every request Harness made before: no network, no aperture, `/workspace` as
the working directory and no writable root outside it. The specification runs on ESS 0.70.0.

## Acceptance

1. `cargo xtask gate` exits 0 with both substrate crates pinned by the release's tag commit and an
   exact `=` version requirement.
2. The conformance suite `crates/harness-substrate/tests/conformance.rs` passes against the new
   driver (`every_workspace_answers_the_same_window_of_the_same_file`,
   `a_path_outside_the_workspace_is_refused_by_every_workspace`, and the rest of that file).
3. `confined_exec_input` sends `apertures` empty, `workdir` absent and `writable_roots` empty, so an
   unchanged start hashes as it did
   (`the_embedded_and_socket_paths_build_one_exec_input`, `crates/harness-substrate/src/tests.rs`).
4. `cargo xtask specification` reports the native ESS suite at 29 passed, 0 failed, with
   `spec/ess-inputs.yaml` requiring `ess 0.70.0` and the `ess-*` crates at rev `077e4ee1`.

## Scope

`crates/harness-substrate/Cargo.toml`, `crates/harness-substrate/src/lib.rs`,
`crates/harness-substrate/src/embedded.rs`, `crates/harness-conformance/Cargo.toml`,
`crates/harness-conformance/src/main.rs`, `spec/ess-inputs.yaml`, `spec/README.md`, `AGENTS.md`,
`Cargo.lock`, `CHANGELOG.md`.

## Out of scope

Routing Harness's confined calls through `substrate_host::confinement` instead of the host driver:
Harness asks the driver for guarded workspace IO, which the confinement contract's consumer surface
does not serve.
