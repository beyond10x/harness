---
format: aep.planning-md/3
id: epic:async-native-loop
kind: epic
status: draft
title: The loop's internals are asynchronous and LoopTask awaits them on the caller's runtime
summary: 'Follow-up to LoopTask: async ports, no private runtimes, no run thread. Not decomposed; the first decomposition was archived after two adversarial reviews.'
relations:
- serves: vision:b10x-owns-its-loop
- informed_by: epic:embedded-by-a-consumer
- depends_on: epic:loop-awaitable-from-async-programs
revision: 4
---
## Status

Follow-up to `epic:loop-awaitable-from-async-programs`, which ships `LoopTask` over the
synchronous loop. Not decomposed. The first decomposition (seven stories, archived on 2026-10-29)
was found not implementable by two adversarial reviews:
`review-result:async-loop-adversary-feasibility` (unsound, 3 blockers) and
`review-result:async-loop-adversary-blast-radius` (gaps, 1 blocker, 12 majors).

## Outcome

The loop's internals are asynchronous: model, tool, approval, hook, environment and sink ports
return futures; harness-llm, harness-mcp and harness-substrate build no private runtime; `LoopTask`
awaits the loop on the caller's runtime instead of a thread, with its methods unchanged. Required
by the organisation rule that all I/O is async (Atlas ADR, drafted 2026-10-29).

## Design inputs a new decomposition must answer

From the two reviews; each is a finding there.

- How `AgentLoop` holds async ports (a second type, generics or a builder), and the `Send` bounds:
  the loop's sinks are deliberately not `Send` today (`README.md:384`).
- A child-executor seam for side-by-side delegates: `parallel.rs` relays to the run's own thread
  over std mpsc (`crates/harness-loop/src/parallel.rs:80,93,111,149,218`), and
  `two_delegates_of_one_turn_run_at_the_same_time` (`crates/harness-loop/src/tests.rs:6603`) needs
  real concurrency.
- Async forms of `call` (`lib.rs:2120`) and `resume_approval` (`lib.rs:2227`).
- Substrate's synchronous `Backend` trait (`crates/harness-substrate/src/backend.rs:21`) and its
  `Client` over a std `UnixStream`.
- The app server has no runtime (`crates/harness-app-server/src/transport.rs:78`).
- Add async traits under new names; no rename of the synchronous ones (blast-radius finding 17).
- ESS: `spec/domains/loop.yaml` and `spec/COVERAGE.md` cite `run_in` and line numbers.
- Every story leaves the workspace green, so no facade runtime may wrap a port that calls
  `Runtime::block_on`.
