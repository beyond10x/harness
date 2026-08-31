---
format: aep.planning-md/1
id: story:go-toolchain-in-confined-runs
kind: story
status: implemented
title: Confined runs can build and test an offline Go project
summary: A declared Go toolchain supplies only GOROOT and workspace-owned caches so a networkless confined run can execute go test.
relations:
- derived_from: epic:adoption-follow-ups
- serves: vision:b10x-owns-its-loop
revision: 5
---
## Defect

Substrate clears the exec environment. The system Go binary can print its version, but `go test` refuses because no build cache, home, or XDG cache exists. The CLI accepts only `--toolchain rust`, so a confined agent cannot verify even a dependency-free Go change.

## Acceptance

`--toolchain go` discovers an explicit or PATH-reachable GOROOT without executing a discovery subprocess, mounts that root read-only, and directs HOME, GOPATH, module cache, build cache, and toolchain configuration into the workspace. Network and checksum lookup remain disabled, CGO is disabled, and no host Go cache, config, credential, or private module state is mounted. Unit tests use synthetic roots; an actual confined scratch project passes `go test` and builds offline.

## Boundary

This is harness-owned toolchain knowledge. It adds no substrate route or contract and does not change the argv shape already pinned as `--toolchain NAME`.
