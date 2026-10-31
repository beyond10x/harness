---
title: Embed the loop in a Rust program
sidebar_position: 6
description: Await a run, a resumed approval or a single gated call from an async Rust program through LoopTask, and call the synchronous loop where there is no async runtime.
lede: An async program embeds the loop through LoopTask and awaits each run; the synchronous AgentLoop is the entry for a program without an async runtime.
source: crates/harness-loop/src/task.rs; crates/harness-loop/src/lib.rs; crates/harness-loop/examples/await_a_run.rs; crates/harness-loop/examples/resume_an_approval.rs; crates/harness-loop/examples/blocking_run.rs
---

# Embed the loop in a Rust program

The loop is the library `b10x-harness-loop`. A program with an async runtime embeds it through
`LoopTask`, the recommended embedding: an owned handle whose `run`, `run_in`, `resume_approval` and
`call` return futures. A program without an async runtime calls the synchronous `AgentLoop`
directly.

`LoopTask` runs the synchronous loop on its own thread today, and a later release replaces that
thread without changing `LoopTask`'s methods.

The futures need no particular runtime: the crate depends on none, and the examples below use
tokio only in their own `main`.

## Await a run

`LoopTask::spawn` takes the run's `LoopConfig` and a builder closure, starts the task's thread and
returns at once. The builder runs on that thread and returns the `LoopPorts`: the model port, the
tool port and the approver, plus hooks (`with_hooks`) or a turn environment (`with_environment`)
when the run needs them. The ports are built, used and dropped on that thread and never reach your
runtime, so they need not be `Send`.

```rust
let config = LoopConfig::new("fixture", "Answer briefly.");
let task = LoopTask::spawn(config, |_cancel: &LoopCancel| {
    // On the task's own thread: open the model client and the tools here.
    let model = Scripted::answering(&["hello", "hello again"]);
    Ok::<_, String>(LoopPorts::new(model, NoTools, DenyAll))
});
```

The example's model answers from a script so that it runs offline. In a program, the builder opens
the real ports. It receives the task's cancel token: hand it to a model client that checks one,
such as `harness_llm::client` from `b10x-harness-llm`, so that a cancelled task also stops the turn
in flight. The thread has no runtime context, so a port that runs a private tokio runtime does not
nest inside yours.

Each command takes its sink by value and hands it back with the result. `run` resolves to
`Ran { result, sink }`, where `result` is the run's own `Result<LoopOutcome, LoopError>`. `run_in`
continues a conversation and resolves to `RanIn { result, items, spend, sink }`, with the
conversation and the spend handed back however the run ended:

```rust
// The sink comes back with the result, holding the run's events.
let Ran { result, sink } = task.run("Say hello.", VecLoopSink::new()).await?;
let first = result?;
println!("run: {:?}, streamed {:?}", first.stop, sink.text());

// `run_in` continues a conversation; the items and the spend come back however it ended.
let next = task.run_in(first.items, "Once more.", VecLoopSink::new());
let RanIn {
    result,
    items,
    spend,
    ..
} = next.await?;
let second = result?;
```

A command is sent to the thread when you call the method, not when the future is first polled, and
the thread runs commands one at a time in the order they were sent. `call` runs one tool call
through the same approval gate a model's call meets and resolves to the outcome and the sink.
`task.shutdown().await` ends the task after the commands already sent and resolves once the ports
have dropped; dropping the `LoopTask` ends it the same way without waiting.

The whole program is
[`await_a_run.rs`](https://github.com/beyond10x/harness/blob/main/crates/harness-loop/examples/await_a_run.rs).
Run it with `cargo run -p b10x-harness-loop --example await_a_run`:

```text
run: Completed, streamed "hello"
run_in: Completed, 4 items, 1 turn
```

### Dropping a future

A future you drop before it resolves, for example because a timeout gave up on it, does one of two
things:

- **Its command is running:** dropping it cancels the task's token for good. The run ends with stop
  `cancelled`, the token has no reset, and every later run on the same task stops `cancelled` too.
  Start a new task to run again.
- **Its command has not started:** dropping it skips that command. The command never reaches the
  ports, so an abandoned `call` performs no effect, and nothing is cancelled.

`task.cancel_handle()` returns the same token, for a program that cancels the task on purpose.

### When the task cannot answer

Every future resolves to a `Result` whose error is a `LoopTaskError`. The loop's own `LoopError` is
never one: it is in `result`, beside the sink.

| Variant | Meaning |
|---|---|
| `Build` | The builder returned an error; this command and every later one resolve to it, carrying the error's text. |
| `Panicked` | Something on the task's thread panicked: the builder, a port during a command, or a drop. After a panic in a command the ports are dropped and every later command resolves to the same error. |
| `Gone` | The task's thread ended, or never started, without answering this command. |

A panic is reported once: to the future that reads it, or else by `shutdown()`.

## Resume an approval

A hosted approver does not decide at once. It returns `ApprovalDecision::Deferred`, and the run ends
with stop `AwaitingApproval` before the effect. The outcome carries an `ApprovalCheckpoint`: the
conversation and the accounting, with no credential and no port, ready to be stored. When a person
decides, resume the checkpoint with `resume_approval`, on the same task or on a new one with fresh
ports and the same `LoopConfig`. Like `run`, it resolves to `Ran { result, sink }`.

The first task runs until the approver defers, and the program stores the checkpoint:

```rust
let Ran { result, .. } = task.run("Write the note.", VecLoopSink::new()).await?;
let suspended = result?;
println!("run: {:?}", suspended.stop);
let stored = serde_json::to_string(&suspended.checkpoint)?;
task.shutdown().await?;
```

A new task, with fresh ports, resumes it with the decision:

```rust
let checkpoint: Option<ApprovalCheckpoint> = serde_json::from_str(&stored)?;
let checkpoint = checkpoint.ok_or("the run did not stop for approval")?;
let task = LoopTask::spawn(config, |_cancel: &LoopCancel| {
    let model = Scripted::new(vec![answers("The note is written.")]);
    Ok::<_, String>(LoopPorts::new(model, Notes::new(), Defer))
});
let resume = task.resume_approval(checkpoint, ApprovalDecision::Approved, VecLoopSink::new());
let Ran { result, sink } = resume.await?;
let resumed = result?;
```

A checkpoint resumed under a different configuration is refused. The tool port resolves the call
again before the effect, so a capability withdrawn in the meantime fails closed even after the
approval.

The whole program is
[`resume_an_approval.rs`](https://github.com/beyond10x/harness/blob/main/crates/harness-loop/examples/resume_an_approval.rs).
`cargo run -p b10x-harness-loop --example resume_an_approval` prints the stop, the effect that ran
only after the approval, and the resumed run's answer:

```text
run: AwaitingApproval { checkpoint_id: "note-approval-1" }
effect: write_note ran
resume_approval: Completed, streamed "The note is written."
```

## Without an async runtime

A program without an async runtime uses `AgentLoop` directly. It borrows the model, tool and
approval ports for its lifetime, and its four entry points block the calling thread until they
return:

| Method | What it does |
|---|---|
| `run` | Runs until the model answers, a budget binds or the run is cancelled. |
| `run_in` | Continues the conversation in a caller-held `Vec` and writes the run's spend into a caller-held `RunLedger`, on every exit path. |
| `resume_approval` | Continues a run from an `ApprovalCheckpoint` with a decision. |
| `call` | Runs one tool call outside any turn, through the approval gate. |

```rust
let mut model = Scripted::answering(&["hello"]);
let (mut tools, mut approvals) = (NoTools, DenyAll);
let config = LoopConfig::new("fixture", "Answer briefly.");

// Blocks this thread until the run ends.
let mut sink = VecLoopSink::new();
let outcome = AgentLoop::new(&mut model, &mut tools, &mut approvals, config)
    .run("Say hello.", &mut sink)?;
```

The whole program is
[`blocking_run.rs`](https://github.com/beyond10x/harness/blob/main/crates/harness-loop/examples/blocking_run.rs).
Do not call these methods on a thread that drives an async runtime: they block it, and a port that
owns a private tokio runtime can panic there. That is what `LoopTask`'s own thread is for.
