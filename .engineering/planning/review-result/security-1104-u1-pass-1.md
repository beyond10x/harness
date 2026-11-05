---
format: aep.planning-md/3
id: review-result:security-1104-u1-pass-1
kind: review-result
status: active
title: Security review pass 1 on wave 2026-11-04 U1 (typed write scope)
relations:
- reviews: story:typed-substrate-run-gets-the-default-write-scope
- reviews: story:measured-exec-documents-the-io-controller
revision: 2
---
unit: U1 (`ee38176`, merged at `ba1fa0a`), findings cover tree `review/sec-u1` at `58c2d1a` plus one uncommitted test file
verdict: nothing found
cases: executed 91→97, red 0
origin: introduced 0 / pre-existing 0 / undecided 0
wrote-outside-worktree: 9 files in the scratch directory, plus one probe directory I created there and deleted
needs-coordinator: none

**1. `git --no-pager diff --stat`:** empty, because the only change is a new, untracked file. `git status --short` shows:
```
?? crates/harness-cli/tests/security_1104_u1.rs
```
That is the only path I touched in the tree, and it is a test file. No implementation file changed.

**2. The cases I added** (`crates/harness-cli/tests/security_1104_u1.rs`). Each runs the shipped binary against a fake model server inside the test, which asks for the writes listed. I run its first, standalone time (`test result: ok. 6 passed; 0 failed`) and all are still green:

| Case | What it asserts | Now |
|---|---|---|
| `a_typed_embedded_run_refuses_every_spelling_of_the_repository_profile` | A typed `--substrate-embedded` run with no scope leaves `.b10x/harness.toml` unchanged. The model tries `./.b10x/harness.toml`, `.b10x/./harness.toml`, `src/../.b10x/harness.toml`, `.b10x//harness.toml` and `.b10x/harness.toml/`. The ordinary write to `notes.md` still goes through. | green |
| `a_typed_embedded_run_refuses_the_repository_profile_through_a_link` | Symlinks `cfg -> .b10x` and `alias.toml -> .b10x/harness.toml`: writing through either leaves the profile unchanged. | green |
| `a_typed_embedded_chat_refuses_the_repository_profile` | The same default holds for `chat`. | green |
| `a_trusted_repository_profile_cannot_choose_the_write_scope` | A trusted `.b10x/harness.toml` that carries `write-scope = ["**=allowed"]` makes the run refuse. Nothing is written. | green |
| `profiles_explain_names_a_configured_scope_for_a_typed_confined_run` | With `[default] write-scope` set and no `write` key, `profiles explain` names that scope for a typed confined run, not the default. | green |
| `tools_describes_the_scope_a_typed_confined_run_applies` | `tools --substrate-embedded` gives the same output with no scope as with the default scope typed. **This case proves little:** a direct probe showed the `tools` output carries no scope at all, so the two outputs always match (see part 4). | green |

**3. The suite run, after my cases existed:**
- `cargo test -p b10x-harness-cli --test security_1104_u1 --test typed_write_scope --test repository_profiles --test security_repo_profiles`:
  - security_1104_u1: `test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.22s`
  - typed_write_scope: `test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.94s`
  - repository_profiles: `test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.21s`
  - security_repo_profiles: `test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 23.91s`
- `cargo test -p b10x-harness-substrate --lib`: `test result: ok. 64 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s`
- `cargo clippy -p b10x-harness-cli --all-targets --locked -- -D warnings`: exit 0.
- `cargo fmt -p b10x-harness-cli --check`: exit 1 on the first try, and only on my new file. I ran `cargo fmt -p b10x-harness-cli`, which changed only that file, and the re-check exited 0. Clippy ran before the reformat; formatting does not change what it checks.

Where 91 comes from: the implementor reported 6, 17 and 64; `security_repo_profiles`, which the implementor did not report, ran 4 here. With my file, 97.

**4. Judgement findings:** none for this unit. One thing outside the diff that I did not raise: the `tools --write-scope` help says "so `tools` answers with it", but `tools` prints no scope, whether typed or default (I ran the built binary directly). The help line predates this unit (`lib.rs:1205-1215`).

**5. What I checked and could not fault:**
- **Every path that writes:**
  - `run` and `chat` both go through `apply_profiles` (`lib.rs:4343`, `:4353`), which calls `fill_write_scope` on the no-config-directory early return (`:1457`) and on the normal path (`:1529`). Every error path aborts the run.
  - The app-server has no substrate and no write tools.
  - `tools` and `context show` only list or render; they never write.
- **Daemon (`--substrate`) path:** the scope never goes to the daemon. Harness enforces it itself:
  - `prepare` builds it from the filled options (`lib.rs:2333`).
  - The socket branch of `published` applies it with `.scoped(scope.clone())` (`lib.rs:3600`).
  - The catalogue refuses before any backend call (`harness-catalogue/src/catalogue.rs:462-477`).
  - This is the same code the embedded path uses (`lib.rs:3557`).
- **Who can set the scope:** a repository profile cannot. The loader refuses the key, and `overlay_plumbing` (`profile.rs:638-648`) blocks it a second time, which my repository-profile case confirms. Only the operator's `[default]` and `-p` profiles can set one. A profile without `write = true` cannot widen what a typed run may write compared with the base, where a typed run had no scope at all.
- **Rule matching:** Harness normalises `.`, `..` and empty segments before matching (`scope.rs:449`). A path ending in `/` is judged as a directory, and `.b10x` itself, written as a directory, is refused by what it can hold (`scope.rs:299-320`).
  - Links are substrate's to decide: `ConfinedOperations::lands` resolves no link (`harness-substrate/src/tools.rs:460`), and substrate's file layer opens every path with `RESOLVE_NO_SYMLINKS | RESOLVE_BENEATH` (pinned `substrate-host/src/fs.rs:27`). The link case passes because of that.
  - `.B10X` is a different file on Linux, which every substrate path requires, so I did not test it.
- **`profiles explain`** prints the same scope `fill_write_scope` applies (`lib.rs:4017-4039`), for both the default and a configured scope.
- **The io documentation story:** the README, help and withheld-message text match the pinned substrate. `probe_resource_usage` requires only the cpu, memory and pids counters and sets `block_io` only when `io` is enabled for the children (`substrate-host/src/probe.rs:314-345`). Exec usage leaves the I/O counters out when `io` is absent (`process.rs:4070`, test `:4449`).

**6. Paths written outside the worktree**, all under `~/.cache/scratch/fbf1604f-db7b-423e-bed5-b9a4dc96d66a/harness-1104/sec-u1/`:
- `builds.tsv`
- `cargo-timed.sh` (a timing wrapper; it calls `build-slot.sh` with the env and nice/ionice prefix from the common file)
- logs: `new-case.log`, `gate-cli.log`, `gate-sub.log`, `fmt.log`, `fmt2.log`, `fmt3.log`, `clippy.log`
- `probe/`, created and deleted

`builds.tsv`:
```
2026-11-04T21:48:51Z	1.5	83.2	1.9G	test -p b10x-harness-cli --test security_1104_u1	0
2026-11-04T23:12:23Z	0.6	18.0	2.0G	test -p b10x-harness-cli --test security_1104_u1 --test typed_write_scope --test repository_profiles --test security_repo_profiles	0
2026-11-04T23:30:21Z	0.5	0.5	2.7G	test -p b10x-harness-substrate --lib	0
2026-11-04T23:30:50Z	0.0	0.0	2.7G	fmt -p b10x-harness-cli --check	1
2026-11-04T23:30:51Z	0.6	0.6	3.2G	clippy -p b10x-harness-cli --all-targets --locked -- -D warnings	0
2026-11-04T23:31:37Z	0.0	55.6	3.2G	fmt -p b10x-harness-cli	0
2026-11-05T00:27:12Z	0.0	0.0	3.2G	fmt -p b10x-harness-cli --check	0
```
- Totals: 3.2 cargo minutes, 158.0 wall minutes (most of it waiting for the build slot). Neither the 8G nor the 20-minute tell line was reached.
- Peak `target/`: 3.2G.
- `df -h /` before the first build: 73G free; before the gate: 51G free.

```findings
[]
```
