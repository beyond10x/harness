---
format: aep.planning-md/3
id: review-result:adversary-d288-u1-pass-1
kind: review-result
status: active
title: Adversary pass 1 on wave d288 U1 (repository profiles)
relations:
- reviews: story:repo-local-profiles
revision: 1
---
Adversary pass 1 on U1 (`story:repo-local-profiles`), wave d288, unit `ec20427`, cases `094adca`
on `adv/d288-u1`. The adversary's report, as returned (paths in its part 6 shortened to the scratch
directory name):

unit: U1-adversary-1, story:repo-local-profiles, the findings cover ec20427 as merged into wave/2026-10-30-d288 at 3c2475c; cases committed as 094adca
verdict: NEEDS-CHANGE
cases: executed 25→33, red 8
origin: introduced 5 / pre-existing 0 / undecided 0
wrote-outside-worktree: 6 paths (scratch adv-u1/)
needs-coordinator: none

Cases added (`crates/harness-cli/tests/adversary_d288_u1.rs`), all red:
- `untrust_of_a_deleted_repository_named_relative_to_a_sibling_removes_its_entry` (:131)
- `untrust_of_a_deleted_repository_named_with_a_trailing_slash_removes_its_entry` (:155)
- `untrust_of_a_deleted_repository_through_the_symlinked_parent_it_was_trusted_by_removes_it` (:177)
- `untrust_of_a_trusted_path_that_became_a_symlink_removes_the_entry_for_that_path` (:203)
- `a_refused_key_carrying_control_characters_reaches_stderr_escaped_and_on_one_line` (:232)
- `profiles_explain_prints_a_repository_model_without_raw_control_characters` (:269)
- `the_untrusted_notice_is_one_line_whatever_the_workspace_directory_is_called` (:296)
- `a_repository_max_turns_of_zero_is_refused_naming_the_repository_file` (:320)

Suite: `cargo test -p b10x-harness-cli --locked --no-fail-fast --test repository_profiles --test
security_repo_profiles --test adversary_repo_profiles --test adversary_d288_u1`: adversary_d288_u1 0
passed 8 failed; adversary_repo_profiles 4, repository_profiles 17, security_repo_profiles 4 ok;
EXIT=101. fmt clean, clippy EXIT=0, cli-contract self-test green (14), 17 pinned versions verified.

Findings:
1. `crates/harness-cli/src/trust.rs:341`, NEEDS-CHANGE, introduced: when the path no longer
   canonicalises, `untrust` falls back to `std::path::absolute`, which keeps `..` and a trailing slash
   and resolves no symlink; the entry stays and the command exits 0 with "was not trusted; nothing
   changed". Reached by `profiles untrust ../old-checkout`; a later clone at that path is then trusted
   with no notice.
2. `crates/harness-cli/src/profile.rs:347` (also :336, :340, :342), NEEDS-CHANGE, introduced: refused key
   and table names reach the stderr refusal raw, ESC and newline included, contrary to "No refusal
   quotes the file's text". Reached by a later commit to a trusted repository.
3. `crates/harness-cli/src/lib.rs:3946` (and :3934), CONFIRMED, introduced: `profiles explain` prints a
   trusted repository's `model` raw.
4. `crates/harness-cli/src/profile.rs:394`, INFEASIBLE, introduced: the untrusted notice is 4 lines when
   the workspace directory name holds a newline; nobody found who would have one.
5. `crates/harness-cli/src/profile.rs:373`, CONFIRMED, introduced: the reader accepts `max-turns = 0`
   and the run's zero-bound refusal does not name `.b10x/harness.toml`.

Attacked and could not break: the CLI contract `2026-10-30` against clap; refused-key spellings; the
untrusted file not read; precedence; the credential refusal; the `session.started.profiles` digest;
trust on existing paths.

```findings
- file: crates/harness-cli/src/trust.rs
  line: 341
  category: boundary
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: untrust of a deleted or moved repository named by `..`, a trailing slash, the symlinked parent trust was given, or a path that became a symlink leaves the entry trusted and exits 0 saying nothing changed
- file: crates/harness-cli/src/profile.rs
  line: 347
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: refused key and table names from a trusted repository profile reach stderr raw, so a later commit can write ESC sequences and forged lines to the operator's terminal, contrary to "no refusal quotes the file's text"
- file: crates/harness-cli/src/lib.rs
  line: 3946
  category: boundary
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: profiles explain prints a trusted repository's model value with its control characters raw
- file: crates/harness-cli/src/profile.rs
  line: 394
  category: boundary
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: the untrusted notice spans several lines when the workspace directory name contains a newline; constructed, no caller shown to reach it
- file: crates/harness-cli/src/profile.rs
  line: 373
  category: boundary
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: a repository max-turns of zero is accepted as valid by the reader and the run's zero-bound refusal does not name the repository file it came from
```
