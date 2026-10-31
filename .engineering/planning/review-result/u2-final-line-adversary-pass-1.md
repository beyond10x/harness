---
format: aep.planning-md/3
id: review-result:u2-final-line-adversary-pass-1
kind: review-result
status: active
title: U2 failed-run finished line adversary pass 1
summary: red 3 (after_compaction on an unchanged conversation, bare CR, session line after finished), 3 mutants now caught
relations:
- reviews: story:failed-run-ends-with-a-finished-line
revision: 1
---
unit: story:failed-run-ends-with-a-finished-line, unit commit 1012e54 plus 2 uncommitted test files in the unit tree
verdict: CONFIRMED
cases: executed 646→652, red 3
origin: introduced 5 / pre-existing 1 / undecided 0
wrote-outside-worktree: 3 paths (session scratch; two e2e fixture temp dirs, deleted)
needs-coordinator: decide finding 1, because brief design point 5 contradicts itself in that one case

## Coordinator routing

- Finding 1: decided for the rule the transcripts can see. `after_compaction` is true when a
  `conversation-compacted` warning was emitted before the failing request in this turn, whether or
  not the summary changed the conversation; that is the P2 shape (compaction ran and could not
  help), which is exactly what a reader of the line needs to know. The doc says so; the adversary
  case is turned to assert `true` under a name stating the rule.
- Finding 2: acceptance 3 is qualified (with a session written, `finished` is the last event line
  and only the `session` line follows it); the end-to-end case asserts that.
- Finding 3: taken, `one_line` also splits on a bare `\r`.
- Finding 4: no change; the adversary's cases now catch M1-M3 and are kept.
- Finding 5: taken, the `LoopStop` doc names `Failed` as the stop of a run that ended in error.

## Added cases

| Case | Asserts | Now |
|---|---|---|
| tests.rs:8041 compaction that changed nothing | summary fails, nothing elided or folded, the next request refused → `after_compaction: false` | red |
| tests.rs:8092 byte rule compacted | byte rule elides, next request refused → `true`, turns 1 | green, catches M1 |
| tests.rs:8117 nothing elidable | over 192 KiB with nothing elidable → no compaction, `false` | green, catches M2 |
| tests.rs:8143 resumption refused | exactly one `Finished{Failed{config}}`, turns 1 | green, catches M3 |
| tests.rs:8197 bare carriage return | `message` holds no `\r` | red |
| end_to_end.rs:3704 failed run that files a session | last stdout line is `finished` | red |

Mutants (scratch copy of the tree): M1 `lib.rs:3089` byte-rule branch reports Unchanged; M2
`lib.rs:1301` compact answers true when it elided nothing; M3 `lib.rs:2353` a refused resumption
returns before `finished`. The unit's suite stayed green under each; the adversary's cases go red.

Suite: `cargo test -p b10x-harness-loop --locked` 251 passed, 2 failed; `cargo test -p
b10x-harness-cli --locked --no-fail-fast` end_to_end 83 passed, 1 failed, other binaries 315
passed. clippy `-D warnings` and `cargo fmt --check` pass on both packages.

Attacked and not broken: double `finished` (delegates sequential, side by side and panicking, the
approval suspension, cancel, deadline, answer nudge, stop hook, summary failure or cancel, retries
exhausted, resume); the flag stays out of the serialized checkpoint; no stale flag across turns or
chat lines; delegate children carry their own flag; no credential in `message`; app-server's
`BridgeSink` ignores `Finished`; human mode prints nothing extra.

```findings
- file: crates/harness-loop/src/lib.rs
  line: 3125
  category: acceptance
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: a summary turn that failed with nothing elided still returns Reported, so the next failed request is marked after_compaction true though the conversation was unchanged, contrary to brief point 5's "a compaction that changed the conversation"
- file: crates/harness-cli/src/lib.rs
  line: 2674
  category: acceptance
  severity: note
  verdict: CONFIRMED
  origin: pre-existing
  message: with a session written (the default) the --json record ends with the session line after finished, so acceptance 3's "last stdout line" holds only under --no-session
- file: crates/harness-loop/src/lib.rs
  line: 214
  category: boundary
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: one_line splits on \n and \r\n only, so a bare carriage return survives in message; no reached LoopError source produces one
- file: crates/harness-loop/src/lib.rs
  line: 3089
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: forcing the byte-rule branch of compact_run to report Unchanged left the unit's suite green; adversary case at tests.rs:8092 catches it
- file: crates/harness-loop/src/lib.rs
  line: 1301
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: compact answering true when it elided nothing left the unit's suite green; adversary case at tests.rs:8117 catches it
- file: crates/harness-loop/src/lib.rs
  line: 2353
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: returning a refused resumption's error before finished left the unit's suite green; adversary case at tests.rs:8143 catches it
- file: crates/harness-loop/src/lib.rs
  line: 120
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the LoopStop doc says every variant is a real state and not an exception, but Failed is never the stop of a LoopOutcome
```
