---
format: aep.planning-md/1
id: story:help-text-names-a-rule-the-code-dropped
kind: story
status: implemented
title: The operator's --help still requires a ws_ workspace name
summary: run --help and chat --help tell the operator to rename their directory; 0c31438 removed that requirement and nothing pins help text.
relations:
- derived_from: epic:pinned-interfaces-honest
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Evidence

Found by the adversarial pass against `wt/substrate-pin-comment-names-the-tag` on 2026-08-30, while
attacking `task:substrate-pin-comment-names-the-tag`. It is not that task's defect — it is the same
defect class on a different and much larger surface.

- `crates/harness-cli/src/lib.rs:565` and `:935` — the doc comment clap renders as long help for
  `--substrate-embedded`, on `run` and on `chat`: "The directory must therefore be named
  `ws_something` — substrate's guarded filesystem will not represent any other name — and one that
  is not refuses the run by name rather than quietly writing somewhere else."
- Measured from the built binary, not from the source: `b10x-harness run --help` prints that
  sentence; `chat --help` prints it too, one occurrence each.
- `0c31438` (2026-08-29) is what made it false. It shipped workspace adoption — a workspace root may
  be a directory the operator already owns — edited `crates/harness-cli/src/lib.rs` (+9/-5), and left
  both copies of the sentence standing.
- `crates/substrate-host/src/fs.rs` at substrate `0.2.2` (`43c5a10`): `validate_root_name` requires
  non-empty, not `.` or `..`, no leading `-`, and alphanumeric or `_` or `-`. The `ws_` prefix
  requirement is gone.
- `contracts/cli/b10x-harness/2026-08-30.1/argv.json` pins `long`, `conflicts_with`, `requires` and
  `takes_value`. It pins **no help text**, so neither half of the CLI contract check
  (`scripts/check-cli-contract.py`, `crates/harness-cli/src/contract.rs`) can see this.
- `crates/harness-substrate/src/embedded.rs:206` — the rustdoc on `workspace_adopt` says the name
  "must begin `ws_` and hold only alphanumerics and underscores"; the body 17 lines below at
  `:223-229` accepts no prefix and permits `-`. Same commit, same class, smaller surface.

## Context

`AGENTS.md` invariant 14 exists because a hand-written description of the command line drifts from
the generated one. This is the gap one level out: the *generated* document does not carry help text
at all, so the one part of the CLI surface a person actually reads is the part nothing pins.

The consequence is not hypothetical in the way a doc comment usually is. `--substrate-embedded` is
the flag whose shape change is the reason `contracts/cli/` exists. An operator reading `run --help`
today is told to rename their directory to `ws_something` before pointing the harness at it — the
exact friction `0c31438` was shipped to remove.

## Acceptance

`b10x-harness run --help` and `chat --help` describe the workspace-name rule the code actually
enforces, and the rustdoc on `harness-substrate`'s `workspace_adopt` agrees with its own body.

**Pinning is the better half of the fix and is the open question.** A test asserting `run --help`
does not contain `ws_something` closes this instance and nothing else. Adding a `help` field to the
argv contract pins the surface, which is what would have caught `0c31438` — and it cuts a new
contract version and grows the document by every flag's help text. Whether that trade is worth
making is the decision this story carries.

## Out of scope

The `--substrate-embedded` behaviour itself, which is correct. This story changes prose and,
if the decision goes that way, the argv contract's field set.
