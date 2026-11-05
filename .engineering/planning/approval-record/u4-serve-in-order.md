---
format: aep.planning-md/3
id: approval-record:u4-serve-in-order
kind: approval-record
status: approved
title: 'U4: serve every request in the order it was sent'
relations:
- decides: story:bridge-turn-start-after-interrupt-on-every-path
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-11-04T23:33:58Z", actor: "human:timo", revision: 2}
---
## The question

`review-result:adversary-1104-u4-pass-1` found that the deferred queue U4 introduced breaks
in-order serving on two constructed paths (`crates/harness-app-server/src/session.rs:140`,
`settle_interrupts` reads the channel past a held request; `:248`, `next_frame` serves a stashed
malformed frame before a held request), and asked whether request 52 should be refused or served,
and whether to change the code or only the fact-3 doc.

## Who decided

Session `b10x-chef-orchestrator`, 2026-11-05, under the owner's grant for class-C items in the
workspace instructions. The answer, verbatim:

> Decision: serve every request in the order it was sent. Take a U4 fix round:
> 1. session.rs:140, `settle_interrupts`: take `self.take_deferred(control)` before
> `self.reader.next_within(...)`, as the adversary proposed. It cannot loop, per its reasoning.
> 2. session.rs:248, `next_frame`: take `deferred` before the stash, so a held `turn/start` is
> answered before a later malformed frame ends the connection, as the base did.
> 3. Make the fact-3 doc at session.rs:181 match: every reader serves `deferred` first.
> Commit the adversary's file `crates/harness-app-server/tests/adversary_1104_u4.rs` with the
> fix; all 3 cases must be green. Targeted tests only: the crate's lib plus the four test targets
> it ran.

## Commits it authorises

U4 correction 1 on `impl/bridge-turn-start-after-interrupt` and its merge into
`wave/2026-11-04-int`.
