---
format: aep.planning-md/1
id: story:missing-model-refuses-by-name
kind: story
status: draft
title: A run with no model refuses by name on every machine, never panics
relations:
- derived_from: epic:pinned-interfaces-honest
revision: 2
---
## Evidence

- `crates/harness-cli/src/lib.rs:1109-1112` — `apply_profiles` returns `Ok(Vec::new())` early when `profile::config_path()` is `None`, before the endpoint/model check at `:1170-1174`.
- `crates/harness-cli/src/profile.rs:85-90` — `config_path()` is `None` when **both** `XDG_CONFIG_HOME` and `HOME` are unset.
- `crates/harness-cli/src/lib.rs:977-986` — `RunOptions::model()`, documented "Never after [`apply_profiles`], which fills it or refuses the run", `expect`s the value.
- Runtime, `target/release/b10x-harness` built from `d1ab5dd`: `env -u HOME -u XDG_CONFIG_HOME b10x-harness run --base-url http://127.0.0.1:9/v1 --input hi --json` → `thread 'main' panicked at crates/harness-cli/src/lib.rs:985: 'apply_profiles' fills the model or refuses the run`, **exit 101**, nothing on stdout.
- The same command with `XDG_CONFIG_HOME=/nonexistent-dir` → `error: no endpoint or model: type '--base-url' and '--model', or name a provider …`, **exit 1**. The refusal path is correct; only the no-home path is not.
- `README.md:92-97` — the three exit statuses a caller acts on: `0` answered, `2` stopped for a named reason, `1` could not run; a run refused before the loop "writes `{"kind":"refused","reason":…}`" under `--json`.
- `contracts/cli/b10x-harness/2026-08-30/README.md:84-85` — the pinned document defers to those three statuses.

## Context

`--model` stopped being clap-required in the profiles wave (`719f6e3`, 2026-08-29 23:08) because a
profile may supply it. The check that replaced clap's lives inside `apply_profiles`, after an early
return that fires when there is no config path at all — so on a machine with no `HOME` and no
`XDG_CONFIG_HOME` the run reaches `model()` unset and panics.

A panic is a fourth exit status (101) on a command line that documents three, with no `refused` line
under `--json`. The environments where it happens are exactly the unattended ones: a systemd unit
with a cleared environment, a container built without a home, a CI runner with `env -i`.

## Acceptance

`b10x-harness run` with no `--model` and no reachable config exits 1 with the same named refusal it
gives when the config file is merely missing, on a machine with neither `HOME` nor
`XDG_CONFIG_HOME` set, and a test pins it.
