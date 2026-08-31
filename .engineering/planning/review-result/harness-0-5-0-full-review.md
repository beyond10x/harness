---
format: aep.planning-md/1
id: review-result:harness-0-5-0-full-review
kind: review-result
status: archived
title: Full adversarial review of harness 0.5.0
summary: Security, correctness, contract, budget, cancellation, and governance defects found at 98bd2f2.
tags:
- correctness
- review
- security
relations:
- reviews: vision:b10x-owns-its-loop
revision: 2
---
## Scope

Reviewed release 0.5.0 at commit 98bd2f252636758ab54477a29f1d0724d81b218f: loop semantics, both provider wires, generic HTTP and credential renewal, CLI and bridge surfaces, tool and substrate adapters, pinned contracts, planning/status pages, website, and the repository gate.

Baseline evidence was a green bash scripts/gate.sh, a clean dependency audit, an MSRV build, a successful website typecheck/build, and a clean atlas map. Adversarial probes then exercised redirects, workflow profile dispatch, bridge interrupts, strict rustdoc, and the organisation brand fence.

## Findings

1. **Critical — redirects can exfiltrate wire credentials and bodies.** Both blocking HTTP clients accept redirects. A cross-origin 307 reproduction forwarded the Messages wire's custom credential header and request body. Credential-renewal bodies are exposed by the same policy. Redirects must be refused, with a two-origin regression test.
2. **High — approval prompts are terminal-injectable.** Model-controlled paths, edits, and argv are rendered raw. C0 controls, ESC sequences, CR, and LF must be escaped visibly before /dev/tty output.
3. **High — bridge interrupt settlement can live-lock.** settle_after_interrupt repeatedly consumes and re-stashes an unrelated frame, so a queued interrupt is never read. Settlement must read the underlying stream while preserving unrelated frames.
4. **High — session placement and permissions violate the safety envelope.** An explicit session directory may be inside the workspace, relative XDG_STATE_HOME is accepted, and an existing permissive directory can yield world-readable transcripts. Placement and 0700/0600 permissions must be enforced.
5. **High — configured budgets can silently stop binding.** Missing or partial usage, provider model aliases, unpriced cache creation, and an exact-equality comparison can permit another model request. Unknown accounting must fail closed by name and equality must bind.
6. **High — max_turns excludes model work.** Delegate turns and compaction-summary turns are omitted. Every model request must consume the one total turn budget, including child loops and summaries.
7. **High — workflow run bypasses profiles and can panic.** Its command path skips profile application; -p is ignored and missing endpoint/model reaches expect. Profiles must apply once and missing configuration must refuse.
8. **High — opaque and unknown provider state is lost.** Compaction destroys opaque items, and both stream decoders skip unknown events/content. Unmodelled bytes must remain replayable and must warn, never disappear.
9. **High — malformed or contradictory provider output is invented or trusted.** Explicit empty terminal output can resurrect streamed calls; missing tool arguments default to an empty object; and structured answers are not locally schema-validated. Absence must remain absent and answer validation must be local and bounded.
10. **High — parallel delegates can change semantics.** Forked catalogues mutate independently and merge last-writer-wins. Parallel execution must be limited to surfaces where it is observationally equivalent to sequential execution.
11. **High — metaharness conversion reports facts it did not observe.** Agents, permission denials, delegated-agent count, and cache-creation usage are hard-coded empty or absent despite source events. The conversion must aggregate the actual run.
12. **High — credential renewal leaks and races.** Failed authorization response bodies reach errors, empty returned credentials are accepted, and the whole credential file is replaced after a stale read. Secrets must stay out of errors and a compare-before-replace must reject lost updates.
13. **High — cancellation is advisory during blocking I/O.** A request or SSE read can wait up to the transport timeout after cancellation. Cancellation must actively abort the request and silent-stream tests must prove bounded exit.
14. **High — provider contracts do not pin the implemented boundary.** Event inventories omit accepted terminal/error/reasoning events; Rust tests compare parsed values rather than exact bytes; headers are not pinned; and the checker proves only fixture membership. New immutable contract versions must pin exact bytes and observable headers.
15. **High — repository governance checks disagree with the tree.** The organisation brand fence is red; touched executable gate/checker scripts are not Rust; the declared two loop-owned tools disagree with the shipped skill tool; and generic HTTP contains credential/vendor semantics. The tree and its governing contracts must be reconciled explicitly.
16. **Medium — bounded data is presented as complete.** Oversized bridge outcomes remain successful, skill bodies are read without an early bound, search silently skips large files, file_read can exceed its declared byte bound without truncated, and JSON exchange accepts exact-limit reads without overflow detection.
17. **Medium — timeout and stream termination are incomplete.** Local timeout kills only the direct child, summary work can cross a deadline and launch another request, Messages keeps draining after message_stop, and HTTP ignores Retry-After.
18. **Medium — shipped surfaces disagree.** Confined writes cannot create missing parents while local writes can; README, STATUS, and website describe stale releases/workspace rules; strict rustdoc fails on broken private links.

## Required closure

Every item above has a protocol story with executable regression evidence. Observable CLI and provider changes cut new immutable contract directories and enter CHANGELOG.md. The full repository gate, strict rustdoc, organisation brand fence, website build, and targeted adversarial probes all pass from the isolated remediation worktree.
