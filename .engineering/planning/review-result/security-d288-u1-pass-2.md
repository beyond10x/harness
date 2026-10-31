---
format: aep.planning-md/3
id: review-result:security-d288-u1-pass-2
kind: review-result
status: active
title: Security review pass 2 on wave d288 U1 (repository profiles, correction 1)
relations:
- reviews: story:repo-local-profiles
revision: 1
---
Security review pass 2 on U1 (`story:repo-local-profiles`), wave d288. It reviews U1 correction 1
(`9631c61`) as merged at `d8af9b7`; its cases are `3b71b49` on `sec/d288-u1`. The reviewer's report,
as returned, condensed where it repeated logs:

unit: U1-security-2
verdict: NEEDS-CHANGE
cases: executed 330→340, red 8
origin: introduced 9, pre-existing 0, undecided 0
wrote-outside-worktree: 22 paths, all in the assigned scratch directory sec-u1/ (logs, commit message, a shell probe)
needs-coordinator: yes. A run started with a typed `--substrate-embedded` or `--substrate` and no profile `write = true` gets no default write scope (`lib.rs:1480`), while the guide says a run that may write cannot rewrite the profile the next run applies. This predates U1.

Cases (`crates/harness-cli/tests/security_d288_u1_p2.rs`):
- `untrust_of_an_existing_path_spelled_through_a_link_and_dot_dot_removes_what_trust_recorded` (:162): red. Untrust removed the bystander `<tmp>/sibling` and kept the `<tmp>/data/x/sibling` entry the operator named.
- `untrust_through_a_dangling_relative_link_under_a_symlinked_directory_removes_its_entry` (:205): red
- `untrust_of_a_cycle_of_links_ends` (:237): green
- `untrust_through_a_dangling_link_does_not_print_its_target_raw` (:274): red
- `a_run_does_not_write_a_repository_model_raw_to_the_terminal` (:301): red. harness-llm's binding-id refusal quotes the model raw. The reviewer first had this precondition wrong and corrected it.
- `the_refusal_of_a_symlinked_b10x_directory_does_not_print_the_link_target_raw` (:351): red
- `profiles_list_and_explain_show_a_trusted_workspace_path_escaped` (:403): red
- `the_default_write_scope_refuses_every_spelling_of_the_repository_profile` (:449): green
- `the_confined_change_tutorial_run_refuses_a_write_to_the_repository_profile` (:503): red
- `the_credential_exception_names_only_flags_a_run_has` (:554): red

Suite results:
- lib 292, adversary_d288_u1 8, adversary_repo_profiles 4, repository_profiles 17, security_d288_u1 5 and security_repo_profiles 4: all passed.
- security_d288_u1_p2: 2 passed, 8 failed; exit 101.
- fmt and clippy exit 0.

What the reviewer could not fault:
- Untrust on a link cycle ends.
- Removing both spellings removes only the link's own location and the typed spelling (apart from F1).
- The `.b10x/**` default refuses every lexical spelling.
- File tools judge where a path lands.
- Process writes are read-only unless a subtree is named.
- The `provider` and `max-turns` refusals hold.
- Typed credentials are refused beside a repository provider.

```findings
- file: crates/harness-cli/src/trust.rs
  line: 350
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: untrust now takes `..` lexically before resolving even an existing path, so `link/../x` removes the link's sibling, an entry the operator did not name, and leaves the target's sibling that trust recorded
- file: crates/harness-cli/src/trust.rs
  line: 414
  category: boundary
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: a dangling relative link is followed against its unresolved parent and `..` collapsed lexically, so untrust through `current -> ../releases/v2` under a symlinked directory leaves the release trusted and exits 0
- file: crates/harness-llm/src/lib.rs
  line: 387
  category: integrity
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: a trusted repository's model holding control characters is admitted at read and quoted raw to stderr by the binding id refusal on every run
- file: crates/harness-cli/src/profile.rs
  line: 258
  category: integrity
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: the refusal of a symlinked .b10x prints the link's repository-controlled target path raw, control characters included
- file: crates/harness-cli/src/lib.rs
  line: 3829
  category: integrity
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: untrust prints the dangling link target it now follows raw; reached only when an operator names a repository's own link
- file: crates/harness-cli/src/lib.rs
  line: 3874
  category: integrity
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: profiles list and profiles explain print a trusted workspace's path raw while the untrusted branch escapes it; needs a workspace name holding control characters
- file: website/docs/tutorials/confined-change.md
  line: 53
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: the tutorial's typed write scope replaces the default and omits .b10x/**=denied, so a run made by following it in a trusted repository can rewrite the profile the next run applies
- file: AGENTS.md
  line: 152
  category: contract-drift
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the credential exception compares a repository provider to a typed --provider flag the CLI does not have, and its "reads ... nothing more" omits the codex renewal that rewrites the vendor login file
- file: crates/harness-cli/src/profile.rs
  line: 457
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: the single-quoted shell_word form breaks out when pasted into fish, the login shell here; the $'...' form is safe, and the docstring promises only POSIX shells
```

## Coordinator routing

Every finding goes to U1 correction 2 in this wave, apart from F9 and the pre-existing question:

- **F1 and F2:** `untrust` canonicalises an existing path first, as `trust` does. It resolves a dangling link's parent before joining the link target. The lexical fallback stays only for what does not exist.
- **F3 to F6:** one rule, applied everywhere: whatever a repository or a path controls is printed through `harness_substrate::inert`. F5 and F6 are INFEASIBLE, but their cases are red, they sit on the surface this unit owns, and the fix is the same call. F3 escapes in harness-llm's binding-id refusal, so the adversary's `explain` case keeps exit 0.
- **F7:** the tutorial's typed scope gains `.b10x/**=denied`.
- **F8:** the paragraph is rewritten to name the flags a run actually has, and to state the codex login renewal. It is rewritten in all three places.
- **F9:** no change. The docstring promises POSIX shells, and the `$'…'` form that harness prints is safe in fish too.
- **Pre-existing question:** a typed `--substrate`/`--substrate-embedded` run without `write = true` gets no default scope. That is a behaviour change outside this story, so it is filed as a draft story. This wave narrows the guide's sentence so it says only what holds: the default applies where harness sets a write scope.
