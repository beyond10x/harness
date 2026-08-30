---
format: aep.planning-md/1
id: story:argv-pin-misdescribes-the-command-line
kind: story
status: draft
title: The argv pin records a placeholder for flags that take no word, and no short flag at all
summary: 23 bare flags carry a value_name the binary refuses; -p is accepted on four commands and pinned nowhere.
relations:
- derived_from: epic:pinned-interfaces-honest
revision: 1
---
## Evidence

Both found by the adversarial pass against `wt/cli-contract` on 2026-08-30, while attacking
`story:cli-contract-dated-ahead`. Neither is that story's defect: both are properties of the pinned
document itself, and fixing either re-pins `argv.json`, which needs its own cut.

- **A flag that eats no word records a placeholder for one.** `contracts/cli/b10x-harness/2026-08-30.1/README.md:97`
  defines `value_name` as "the placeholder in the usage line". 23 flags with `takes_value: false`
  record a `value_name` anyway — `--substrate-embedded` among them, which is the flag whose shape
  change is the reason this contract exists at all (`AGENTS.md:84-86`). `run --help` prints no
  placeholder for any of them, so the document and the binary disagree.
- **A short flag a consumer can type is pinned nowhere.** `crates/harness-cli/src/lib.rs:340` and
  `:480` accept `-p` on four commands. The word "short" occurs in neither
  `contracts/cli/b10x-harness/2026-08-30.1/README.md` nor `scripts/check-cli-contract.py`. A
  consumer building an invocation from the document cannot know `-p` exists, and nothing would
  catch it being removed.

## Context

The CLI contract exists because a flag changed shape and a consumer pinned to `0.1.0` was refused by
clap before any harness code ran. Both defects here are the same kind of gap in a different place:
the document describes a command line that is not quite the one the binary serves, in a direction a
consumer would act on.

The `value_name` one is the more misleading of the two. A driver generating an invocation from the
pin has every reason to read `value_name` as "this flag takes this argument", and for these 23 it
does not — the binary refuses the extra word.

Neither is urgent and neither is a safety question. Both are cheap once a cut is being made for
another reason, and both are invisible until somebody generates an invocation from the document.

## Scope

- `crates/harness-cli/src/contract.rs` — `flags()`, which emits `value_name`, and whatever records
  short flags.
- A new `contracts/cli/b10x-harness/<date>/` version directory: invariant 13 forbids editing a
  released one, and `2026-08-30.1` will be released once wave 2 is pushed.
- `scripts/check-cli-contract.py` — a new per-flag key has to be learned there and made optional for
  the already-pinned versions.
- `CHANGELOG.md` in the same change (invariant 14).

## Acceptance

`takes_value: false` implies `value_name: null` for every flag in the version in force; every short
flag clap accepts is either pinned in the document or named in its *What is not pinned* section; and
both halves of the CLI contract check are green against the new version.
