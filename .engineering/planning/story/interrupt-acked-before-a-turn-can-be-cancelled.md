---
format: aep.planning-md/1
id: story:interrupt-acked-before-a-turn-can-be-cancelled
kind: story
status: active
title: An interrupt acknowledged before the turn is cancellable stops nothing
summary: 'turn/started is notified before TurnControl is installed: an interrupt decoded in that window is acked, cancels nothing, and the client receives the answer it cancelled.'
relations:
- derived_from: epic:bridge-mode-proof
- informed_by: story:bridge-interrupt-race-pinned
- serves: vision:b10x-owns-its-loop
revision: 5
---
## Evidence

`crates/harness-app-server/src/lib.rs:321-340`. `turn/start` responds with the turn id and notifies
`turn/started` inside a borrow of the writer, and **only then** builds the `TurnControl` and installs
it in `self.active`:

```rust
{
    let mut writer = self.wire.writer.borrow_mut();
    writer.respond(id, &json!({"turn": {"id": turn_id}}))?;
    writer.notify("turn/started", ...)?;
}
// ... TurnControl built here ...
if let Ok(mut active) = self.active.lock() {
    *active = Some(control.clone());
}
```

An interrupt decoded in that window finds `active == None`. `Wire::drain_control` acknowledges it,
and it cancels nothing: the turn then runs to completion and delivers the answer it was asked to
stop giving, while the client holds an acknowledgement saying it was stopped.

The comment immediately below the gap (`:332-334`) reasons about precisely this failure at the other
end of the turn — "an interrupt decoded just before the clear would be erased, and the turn it was
meant to stop would run to completion while the client held an acknowledgement" — and a fresh token
per turn was chosen to prevent it. The same hole is open at the start.

## How it was found, and how strong the evidence is

Measured, not inferred, while implementing `story:bridge-interrupt-race-pinned` on 2026-08-30.

- **Baseline**, as shipped, under oversubscription (`taskset -c 0-1`, `--test-threads=17`, 50 suite
  runs): **21 red of 50**, split 11/10 across two tests, always `left: "completed"` /
  `right: "interrupted"`, never both in one run — one shared cause.
- **Deterministic reproduction**: a temporary 200 ms sleep injected between `turn/started` and the
  point the turn becomes interruptible fails both tests 5 runs out of 5, unloaded.
- **Control**: with the same injection, the two interrupt tests that already gate on a streamed
  event survive a window a thousand times the natural one. That isolates the cause to the window,
  not to the fixture's pacing.

## Consequence

This is not a test defect. The tests were the only thing that noticed. **A real bridge client that
sends `turn/interrupt` promptly after `turn/start` returns can have its interrupt acknowledged and
ignored**, and will then receive the completed answer it cancelled. Promptly means within the time
it takes this process to build a `TurnControl` and take a mutex — small, but a client that pipelines
the two frames, as `an_interrupt_queued_before_the_turn_starts_does_not_crash_the_server` does, hits
it by construction rather than by luck.

`STATUS.md:17` claims an interrupt "is acted on when its frame is decoded, acknowledged between
streamed events, and distinguished from a client that merely vanished". The first clause is what
does not hold.

## Scope

`crates/harness-app-server/src/lib.rs` — moving `*active = Some(control)` above the `respond`/
`notify` block closes it. The `TurnControl` does not depend on anything the response computes, so the
move is mechanical; what needs argument is whether installing it before the client has been told the
turn exists creates a different window, and what happens to an interrupt for a turn id the client
cannot yet have seen.

Test-side, `crates/harness-cli/tests/bridge_mode.rs` already holds the two cases that catch it: with
this fixed they pass without their `skip_to` gate, which is the check that the fix is real rather
than the gate compensating for it.

## Acceptance

An interrupt decoded between `turn/start` being answered and the turn becoming interruptible cancels
that turn rather than being acknowledged and dropped — pinned by a case that injects the window and
fails without the fix — and `crates/harness-cli/tests/bridge_mode.rs`'s two interrupt tests pass
with their client-side gate removed.

## A test for it is already written

An adversarial pass against `wt/bridge-interrupt` wrote two cases before the run was stopped for
cost. The worktree is gone, so the diff is carried here rather than as a path — a file under an
ignored directory is a record nobody finds later.

- `an_acknowledged_interrupt_never_also_delivers_the_answer` — the assertion this story exists for:
  an interrupt that was acknowledged must not be followed by the completed answer.
- `a_request_arriving_mid_stream_is_refused_before_the_turn_ends`

**Neither was ever run**, so their state is unknown. Apply the patch first and find out; an unrun
test is a starting point, not evidence.

```diff
diff --git a/crates/harness-cli/tests/bridge_mode.rs b/crates/harness-cli/tests/bridge_mode.rs
index b90216c..8e3fb6d 100644
--- a/crates/harness-cli/tests/bridge_mode.rs
+++ b/crates/harness-cli/tests/bridge_mode.rs
@@ -745,3 +745,118 @@ fn an_unknown_request_mid_turn_is_refused_without_killing_the_server() {
         json!("interrupted")
     );
 }
+
+#[test]
+fn an_acknowledged_interrupt_never_also_delivers_the_answer() {
+    // A client that pipelines `turn/start` and `turn/interrupt` — the shape
+    // `an_interrupt_queued_before_the_turn_starts_does_not_crash_the_server` names in its own
+    // first comment, and the one no test performs since that test began waiting for a streamed
+    // delta first.
+    //
+    // The server answers `turn/start` and notifies `turn/started` before it installs the control
+    // its reading thread cancels through (`crates/harness-app-server/src/lib.rs:321-340`). An
+    // interrupt decoded in that window is answered `{"result":{}}` — a success — and cancels
+    // nothing. Whichever way that window is closed, these two cannot both be true of one turn:
+    // the interrupt was acknowledged, and the answer it was meant to stop was delivered anyway.
+    // `an_interrupt_stops_a_turn_that_is_blocked_on_the_model` already says so for a turn that is
+    // mid-stream; nothing said it for a turn that was only just announced.
+    let endpoint = Endpoint::start("slow");
+    let mut bridge = Bridge::start(&endpoint, &[]);
+    bridge.handshake();
+    let thread_id = bridge.start_thread(&json!({}));
+
+    // Both frames written before either answer is read. A pipelining client cannot name the turn
+    // it is cancelling — it has not been told the id yet — and this server reads no `turnId` on
+    // `turn/interrupt`: the reading thread cancels whichever turn is active.
+    bridge.write(&json!({"id": 50, "method": "turn/start", "params": {
+        "threadId": thread_id, "input": [{"type": "text", "text": "go"}],
+    }}));
+    bridge.write(&json!({"id": 99, "method": "turn/interrupt", "params": {
+        "threadId": thread_id,
+    }}));
+
+    let mut acknowledgement = None;
+    let mut frames = Vec::new();
+    loop {
+        let frame = bridge.next_frame();
+        if frame.get("id") == Some(&json!(99)) && frame.get("method").is_none() {
+            acknowledgement = Some(frame);
+            continue;
+        }
+        let terminal = frame.get("method").and_then(Value::as_str) == Some("turn/completed");
+        frames.push(frame);
+        if terminal {
+            break;
+        }
+    }
+    let acknowledgement = acknowledgement.expect("the interrupt is answered at all");
+    assert!(
+        acknowledgement["error"].is_null(),
+        "the server acknowledged the interrupt: {acknowledgement}"
+    );
+    assert!(
+        !methods(&frames).contains(&"item/completed"),
+        "an acknowledged interrupt must not also deliver the answer it was stopped from giving: \
+         {frames:?}"
+    );
+    assert_eq!(
+        frames.last().expect("a terminal frame")["params"]["turn"]["status"],
+        json!("interrupted"),
+        "an interrupt the server answered with a success must have been acted on"
+    );
+}
+
+#[test]
+fn a_request_arriving_mid_stream_is_refused_before_the_turn_ends() {
+    // The path `an_unknown_request_mid_turn_is_refused_without_killing_the_server` is named for:
+    // `Wire::drain_control`, which answers control frames *between streamed events* — the only
+    // moment a running turn is at the wire (`STATUS.md:17`, "acknowledged between streamed
+    // events"). Reaching it needs a frame that does not itself end the turn; an interrupt cancels
+    // the stream, so nothing is emitted afterwards and the answer necessarily comes from the main
+    // loop once the turn is over, by the other code path and with the other message.
+    //
+    // Without this case the whole `drain_control` request arm is unreached: re-nesting the two
+    // `writer.borrow_mut()` calls in `BridgeSink::notify` — the exact regression its comment
+    // warns about — leaves every test in this workspace green.
+    let endpoint = Endpoint::start("slow");
+    let mut bridge = Bridge::start(&endpoint, &[]);
+    bridge.handshake();
+    let thread_id = bridge.start_thread(&json!({}));
+    let turn_id = bridge.start_turn(&thread_id, "go");
+    bridge.skip_to("item/agentMessage/delta");
+
+    bridge.write(&json!({"id": 98, "method": "thread/resume", "params": {}}));
+
+    let mut refusal = None;
+    let mut frames = Vec::new();
+    loop {
+        let frame = bridge.next_frame();
+        if frame.get("id") == Some(&json!(98)) && frame.get("method").is_none() {
+            refusal = Some(frame);
+            continue;
+        }
+        let terminal = frame.get("method").and_then(Value::as_str) == Some("turn/completed");
+        frames.push(frame);
+        if terminal {
+            break;
+        }
+    }
+    let refusal = refusal.expect("the refusal arrives before the turn ends, not after it");
+    assert_eq!(refusal["error"]["code"], json!(-32601), "{refusal}");
+    assert!(
+        refusal["error"]["message"]
+            .as_str()
+            .expect("a message")
+            .contains("while a turn is running"),
+        "the refusal came from the mid-turn path, not from the main loop after the turn: {refusal}"
+    );
+    assert_eq!(
+        frames.last().expect("a terminal frame")["params"]["turn"]["id"],
+        json!(turn_id)
+    );
+    assert_eq!(
+        frames.last().expect("a terminal frame")["params"]["turn"]["status"],
+        json!("completed"),
+        "a refused control frame must not end the turn it arrived during"
+    );
+}
```
