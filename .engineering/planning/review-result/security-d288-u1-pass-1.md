---
format: aep.planning-md/3
id: review-result:security-d288-u1-pass-1
kind: review-result
status: active
title: Security review pass 1 on wave d288 U1 (repository profiles)
relations:
- reviews: story:repo-local-profiles
revision: 1
---
Security review pass 1 on U1 (`story:repo-local-profiles`), wave d288, unit `ec20427`, cases
`126b12d` on `sec/d288-u1`. The reviewer's report, as returned (condensed only where it repeated logs):

unit: U1-security-1 (commit 126b12d on sec/d288-u1, reviewing ec20427 as merged at 3c2475c)
verdict: red
cases: executed 25→30, red 4
origin: introduced 6, pre-existing 0, undecided 0
wrote-outside-worktree: 9 files in the assigned scratch directory sec-u1/
needs-coordinator: yes. The cleared decision lets a repository choose `provider`, but it does not say whether that choice may make the run use that provider's default credential when the operator never named it (finding 5).

Cases (`crates/harness-cli/tests/security_d288_u1.rs`):
- `a_refused_key_name_carrying_control_characters_is_not_written_raw_to_stderr` (:162): red
- `a_parse_failure_naming_a_key_with_control_characters_is_not_written_raw_to_stderr` (:189): green
- `a_repository_provider_value_carrying_control_characters_is_not_written_raw_to_stderr` (:221): red
- `untrusting_a_deleted_repository_named_through_a_parent_component_removes_its_entry` (:254): red
- `untrusting_a_deleted_repository_by_the_symlink_it_was_trusted_through_removes_its_entry` (:286): red

Suite: adversary_repo_profiles 4, repository_profiles 17, security_repo_profiles 4 ok;
security_d288_u1 1 passed 4 failed; exit 101. fmt and clippy exit 0.

Findings:
1. Refused key names printed raw (`profile.rs:347`, :336, :340, :342), NEEDS-CHANGE, introduced.
2. An unknown `provider` value printed raw (`provider.rs:255`), NEEDS-CHANGE, introduced: the message
   is old, but before this unit no repository file could reach it.
3. `untrust ../<deleted>` leaves the entry (`trust.rs:341`), NEEDS-CHANGE, introduced.
4. Untrust through a dangling symlink leaves the target trusted (`trust.rs:339`), CONFIRMED, introduced.
5. The repository's choice of provider decides which of the operator's default credentials is used
   (`lib.rs:1447`, through :1398-1403), CONFIRMED, introduced, note; read, not run. Each credential goes
   only to the endpoint it is paired with, so nothing is redirected, but AGENTS.md § Credentials says
   "the harness reads nothing it was not pointed at". Decision needed: require the repository's
   provider to be one the operator already names, or document the exception.
6. A `--write` run can rewrite the next run's repository profile (`profile.rs:607`), CONFIRMED,
   introduced, note: the default write scope denies only `.git/**`.

Checked, no defect: permission keys refused; no ambient trust; trust record owner, mode and atomic
save; the check-to-read race grants nothing new; parser messages; `session.started.profiles` holds no
credential; `max-turns` precedence.

```findings
- file: crates/harness-cli/src/profile.rs
  line: 347
  category: integrity
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: a trusted repository profile's refused key names are printed raw into the refusal, so the file's control characters reach the operator's terminal or CI log, against the guide's "No refusal quotes the file's text"
- file: crates/harness-cli/src/provider.rs
  line: 255
  category: integrity
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: an unknown provider value from a trusted repository profile is quoted twice, unescaped, in provider::resolve's refusal, a path that only this unit lets a repository file reach
- file: crates/harness-cli/src/trust.rs
  line: 341
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: untrust of a deleted repository named with a parent component keeps the canonical entry because std::path::absolute keeps .., exits 0 saying it was not trusted, and a repository later cloned there is trusted again
- file: crates/harness-cli/src/trust.rs
  line: 339
  category: boundary
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: untrust through a symlink whose target was deleted makes the dangling link absolute and leaves the target's entry trusted
- file: crates/harness-cli/src/lib.rs
  line: 1447
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the typed-credential refusal is complete for the five credential flags, but when only the repository chose the provider the run still reads that provider's default credential (env var, vendor login file, codex renewal), which the operator never named and AGENTS.md's no-ambient-credential rule does not except
- file: crates/harness-cli/src/profile.rs
  line: 607
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the default write scope denies only .git/**, so a --write run in a trusted workspace can rewrite the .b10x/harness.toml that the next run applies
```

## Coordinator decision on finding 5

Taken by the wave coordinator inside D288 and the cleared
`decision-blocker:repository-profile-trust-policy`: the operator's explicit `profiles trust <PATH>` is
the operator pointing the harness at that repository's plumbing choices, so a repository `provider`
reads that provider's default credential exactly as a typed `--provider` would, and nothing more; the
credential stays paired with its own endpoint. This is written into AGENTS.md § Credentials and the
profiles guide as a named exception, not left implicit. Requiring the operator to name the provider
again would make the admitted `provider` key useless.
