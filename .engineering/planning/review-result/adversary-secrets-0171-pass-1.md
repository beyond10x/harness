---
format: aep.planning-md/3
id: review-result:adversary-secrets-0171-pass-1
kind: review-result
status: active
title: Adversary pass 1 on story:api-key-from-the-secrets-store (wave secrets-0171, before 9389c693)
relations:
- reviews: story:api-key-from-the-secrets-store
revision: 1
---
Adversary pass 1 over the unit at the uncommitted tree before 9389c693 (2026-10-22).

| id | severity | file:line | outcome |
|---|---|---|---|
| A1 | note | crates/harness-llm/src/lib.rs (`secret_with`'s refusal) | fixed in 9389c693: a malformed reference carrying ESC, BEL, a newline, C1 or a bidi override was echoed raw to stderr; the refusal now renders it with `escape_debug`. Red then green: `crates/harness-llm/tests/secrets_adversary.rs::a_refused_reference_reaches_the_terminal_without_a_raw_control_character` |
| A2 | note | crates/harness-llm/src/lib.rs (`Stored::refresh`) | documented: unreachable, no llm client calls `refresh`; a 401 is not resent and the next turn re-reads the store (pinned green) |
| A3 | note | crates/harness-cli/src/lib.rs (`app_server_command`) | fixed: the app-server secret branch had no test; now pinned by `crates/harness-cli/tests/secrets_adversary_cli.rs::a_malformed_reference_is_refused_by_name_on_every_command_before_any_request` |
| A4 | note | crates/harness-llm/src/lib.rs (`Source::secret`) | documented in README: only the default keychain service is read, so a namespace `secretsctl` mounted on another backend is refused as not found |

Held: 23 malformed references refused with 0 store reads; the longest admissible reference accepted; storage refusals name the reference and send nothing; no refusal or Debug carries a value; the conflict rows of the 2026-10-22 contract are symmetric and clap refuses every pair on all four commands; without the flag no command connected to a fixture session-bus socket.
