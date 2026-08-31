# Design 0004: attributable context and toolchain providers

## Decision

A run's standing instruction is a `ContextPackage`: an ordered set of layers carrying `id`,
`kind`, `trust`, cache class, source, capture time and body. The model receives a deterministic
tagged rendering. The event record receives only a manifest: metadata, byte count and SHA-256,
never the body.

The trust vocabulary answers who may speak with authority: `harness`, `operator`, `workspace`, or
`machine`. It is distinct from provenance (the `source`) and freshness (cache class and capture
time). Workspace instructions are therefore visible as workspace-trust text rather than silently
acquiring the authority of the harness that read them. Only kind, trust and a useful document
source reach the model; cache class, capture time, byte count and digest stay in the manifest, where
they support audit and caching without spending prompt tokens.

`--instructions-file` is `operator.instructions`, not a replacement for immutable harness and tool
guidance. `context show` exposes the body-free manifest by default; `--body` is the explicit path
to instruction contents.

Toolchains use versioned declarative providers. Built-in Rust, Go, Taskfile, npm and Yarn documents
and explicitly named operator extensions contribute read-only discovery, typed tools, fixed argv
plans and selected context facts through the same registry. The catalogue is frozen before turn
one. There is no sticky widening when a later turn notices a marker.

## Admission and execution

`--toolchain auto` checks only declared root-relative project markers. Explicit provider names
remain declarations, and `--toolchain-spec FILE` is the only way custom policy enters the registry.
A missing installation or unreadable static probe refuses startup; discovery never executes a
compiler, Taskfile expression or package manager.

Toolchain tools are published only when embedded substrate admits confined execution. A
socket-backed run refuses a process-local toolchain declaration because the daemon did not receive
those roots. Every dependency operation is offline and the sandbox has no network.

Providers publish language tools (`rust_test`, `go_test`, and lifecycle peers), project-derived
`taskfile_run`, `npm_run` and `yarn_run` enum calls, plus generic
verification routers (`toolchain_check`, `toolchain_build`, `toolchain_test`,
`toolchain_fmt_check`). A generic call runs every active provider in deterministic order. Schemas
carry domain arguments, never raw argv. Formatter writes require explicit paths, and every path is
checked against the run's write scope before any formatter starts.

All entries map to the existing neutral `shell` operation. This adds no operation to the protocol
inventory and requires no coordinated cross-repository vocabulary migration.

## Consequences

- A transcript can prove which context shaped a run without storing instruction bodies.
- Prompt authority, origin and freshness are separately inspectable.
- Only facts marked for model exposure, such as installed language versions, are available from
  turn one; discovery evidence and source hashes remain record metadata.
- Adding another toolchain is another built-in YAML document or an explicit operator file, not
  another branch in the loop, substrate adapter or command-line parser.
- Builds may write caches and artifacts inside the workspace and are high-risk process calls.
