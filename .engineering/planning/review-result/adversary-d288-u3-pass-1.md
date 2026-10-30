---
format: aep.planning-md/3
id: review-result:adversary-d288-u3-pass-1
kind: review-result
status: active
title: Adversary pass 1 on wave d288 U3 (injective substrate escape)
relations:
- reviews: story:substrate-escape-is-injective
revision: 1
---
Adversary pass 1 on U3 (`story:substrate-escape-is-injective`), wave d288, unit `ee042db`, cases
`38e8e75` on `adv/d288-u3`. The adversary's report, as returned:

unit: U3-adversary-1 (story:substrate-escape-is-injective), covering adv/d288-u3 at 38e8e75 (base aeaa4dc, unit ee042db)
verdict: red. 1 NEEDS-CHANGE (introduced), plus 3 judgement findings
cases: executed 129→134, red 1
origin: introduced 3 / pre-existing 0 / undecided 1
wrote-outside-worktree: 4 paths (in the assigned scratch dir)
needs-coordinator: whether a serde `Unexpected::Str` reason counts under acceptance bullet 3 (I treat it as yes)

Commit: `38e8e75`, by `b10x-bot[bot]` as both author and committer.

Cases added in `crates/harness-substrate/tests/adversary_d288_u3.rs`:
- `a_machine_answer_whose_facts_is_a_string_shows_the_daemon_text_escaped_once`: red. The daemon
  answers `{"result":{"facts":"a\u001b\\b"}}` to `GET /v1/machine`; the reason holds serde's `{:?}`
  quote of the daemon's text, so the display escapes it twice.
- `every_scalar_value_reads_back_from_its_display`: green.
- `texts_built_from_escape_material_read_back_from_their_display`: green.
- `no_hazardous_scalar_value_passes_through_raw`: green.
- `the_edges_of_the_rewritten_ranges_show_as_written_here`: green.

Suite: `cargo test -p b10x-harness-substrate --locked --no-fail-fast --lib --test <each of the 13
targets>`: exit 101, every target green except `adversary_d288_u3` (4 passed; 1 failed). fmt clean,
clippy clean.

Findings:
- F1, NEEDS-CHANGE, introduced (`crates/harness-substrate/src/client.rs:297`, also `:284`, `:689`,
  `embedded.rs:297`/`:300`): `Unreadable { reason: error.to_string() }` holds serde's message, which
  writes an unexpected string as `string {:?}`, so `inert` escapes that quote a second time. Reached by
  `b10x-harness --substrate <socket>` when a daemon answers `facts` as a string. Fix: do not pass
  serde's `Display` into a reason.
- F2, risk, introduced (`client.rs:520`, `:532`, `:544`, `:138`): a `"` inside the id is no longer
  escaped and can fake the end of its quotes; each message carries one daemon value, so the line stays
  injective. Cosmetic, no case.
- F3, risk, introduced (`embedded.rs:274`): `driver_said` joins `message` and `address` unescaped;
  nothing found reaches it (substrate-host 0b9d50c passes fixed field names and messages). INFEASIBLE.
- F4, risk, mixed origin (`client.rs:421`, `:623`, `:667`, `embedded.rs:433`, `withheld.rs:89`): JSON
  text then `inert` shows a raw ESC as `\\u001b`; injective, readability only; introduced at five
  sites, pre-existing at `no workspace id in` (`client.rs:456`, `:510`).

```findings
- file: crates/harness-substrate/src/client.rs
  line: 297
  category: acceptance
  severity: blocker
  verdict: NEEDS-CHANGE
  origin: introduced
  message: serde's Unexpected::Str Debug-quotes daemon text inside an Unreadable reason, so the now backslash-escaping inert shows a raw ESC as \\u{1b} (case a_machine_answer_whose_facts_is_a_string_shows_the_daemon_text_escaped_once)
- file: crates/harness-substrate/src/client.rs
  line: 520
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: a quote character in a daemon-supplied id, root name, origin or status line is no longer escaped and can fake the end of its quotes; each message holds one daemon value, so the line stays injective
- file: crates/harness-substrate/src/embedded.rs
  line: 274
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: driver_said joins message and address unescaped, so two refusals splitting the same text differently display alike, but substrate-host 0b9d50c only ever passes fixed field names and fixed messages
- file: crates/harness-substrate/src/withheld.rs
  line: 89
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: undecided
  message: JSON text passed through inert shows a raw ESC as \\u001b beside a raw C1 as \u{9b}; injective but escaped twice, introduced at the file/exec/output/withheld sites and pre-existing at "no workspace id in"
```
