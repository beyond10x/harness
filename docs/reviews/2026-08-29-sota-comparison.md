# Harness vs SOTA harnesses — 2026-08-29

Tree at `b2a847e` (main, clean, `cargo check --workspace --locked` exit 0). Compared against the
shape shared by Claude Code, Codex CLI, OpenHands and Aider as of the assistant's knowledge cutoff
(January 2026): persistent session, flat tool surface, interactive per-call approval, token-aware
compaction with summarisation, concurrent read-only tool calls, env + project-memory injection,
head+tail output truncation, line-addressed file reads, regex/glob search, sub-agents.

Every row carries its source. "SOTA" claims are from the assistant's knowledge of those harnesses,
not verified against their current source today.

## Outcome (same day)

Every row of § *Wrong* was worked the same day — #13's three owned items (sub-agents, structured
output, hooks) in a second wave under design 0002: three implementors, two reviewers (16 findings:
3 High, 8 Medium, 5 Low), two fix agents, all closed; a self-review of the wave (`2026-08-29-0002-self-review.md`, 1 Medium, 5 Low, 2 informational) fixed by two more agents; gate 696 passed / 0 failed — by five agents
in parallel plus an integration pass, then two read-only review passes over the result (29 findings: 4 High, 11 Medium, 14 Low —
all fixed or documented by two fix agents the same day; the two review reports are in the session,
their High findings were a summary fold that could split a tool call from its result, a summary
request the Messages wire would reject, a confined read that reported a byte-ceiling prefix as the
whole file, and a batch thread panic that took its siblings' answers with it). Gate after all of it:
595 passed / 0 failed / 1 ignored, exit 0 (`bash scripts/gate.sh`).

| # | what landed | where |
|---|---|---|
| 1 | Sessions on disk, and a run that dies hands its conversation back. `AgentLoop::run_in(&mut items, …)` writes the conversation into the caller's vector on **every** exit path, so a wire failure on turn 20 leaves the first nineteen; the shell files them. `--session-dir`, `--resume <id\|latest>`, `--no-session`, `b10x-harness sessions`. A turn whose stream broke after it started speaking is attempted again, three times, with a `turn-retried` event so a reader knows what to disregard. | `harness-cli/src/transcript.rs`, `harness-loop/src/lib.rs` `run_in`, `end_to_end.rs::a_run_files_its_conversation_and_a_later_one_resumes_it`, `::a_run_that_never_got_an_answer_still_files_what_it_had` |
| 2 | `--approve <auto\|prompt\|deny\|all>`, default `auto`: a person is asked over `/dev/tty` about **this** write and asked again about the next, `y`/`n`/`a`. With no terminal, one stderr line and then refusals — never a silent fall-back. The library default is still `DenyAll` (invariant 12). | `harness-cli/src/approve.rs`, `lib.rs` `approver`, `end_to_end.rs::a_write_is_refused_when_the_run_may_not_ask_anybody`, `::asking_for_a_person_when_there_is_no_terminal_refuses_the_run_by_name` |
| 3 | `Envelope::needs_approval` is `risk > ceiling` and nothing else; `file_edit` is `Medium` like `file_write`. `Idempotency` is still declared, for a scheduler. | `harness-wire/src/envelope.rs`, `harness-tools/src/catalogue.rs` |
| 4 | `harness_tools::Flat` — every entry as its own tool, with its own schema — and `--surface flat` is the **default** on `run`, `chat` and `tools`. `verbs` stays fully served for metaharness's MCP surface and for an arm that compares the two. | `harness-tools/src/flat.rs`, `harness-cli/src/lib.rs` `Published`, `end_to_end.rs::the_binary_reads_a_real_file_by_calling_the_tool_directly` |
| 5 | Head **and** tail of an over-long `run` output are kept, with `\n… N bytes omitted here …\n` between them and `omitted_bytes` in the result. A `cargo test` verdict survives. | `harness-tools/src/local.rs` |
| 6 | `file_read` takes `offset`/`limit` in lines, answers `cat -n`-numbered lines and `lines: {from, to, total}`; a window past the end is refused with the number of lines there are. | `harness-tools/src/catalogue.rs`, `local.rs`, `harness_tools::ReadWindow` |
| 7 | `find` (glob) is a seventh entry; `search` takes `regex`, `glob` and `context`, and a regex that does not compile is refused in the regex crate's own words. | `harness-tools/src/catalogue.rs`, `harness_tools::SearchOptions` |
| 8 | Consecutive published calls whose invoked envelope neither mutates nor asks a person go to the port as one batch (`ToolPort::call_batch`, one thread per call); a write ends the group, and a port that miscounts its outcomes is not trusted with any of them (`batch-miscounted`). | `harness-loop/src/lib.rs`, `harness-tools/src/{flat,verbs}.rs` |
| 9 | Compaction is token-aware: given `--context-window` it fires at 80% of the provider's own last reported input count and frees to 50%, and where eliding tool output cannot reach the target it spends one extra turn on an LLM summary of the earlier run. `--context-window` now reaches `LoopConfig` from `run` and `chat`. | `harness-loop/src/lib.rs`, `LoopConfig::with_context_window`, `LoopEvent::Compacted` |
| 10 | The standing instruction carries an environment block — absolute workspace, OS/arch, UTC date, git branch read from `.git/HEAD` without spawning `git` — and the project's own `AGENTS.md` (else `CLAUDE.md`), bounded at 32 KiB and said so. `--no-project-instructions` is the control. | `harness-cli/src/environment.rs`, `lib.rs::the_instruction_says_where_the_run_is_and_what_day_it_is` |
| 11 | `response.reasoning_summary_text.delta` and `thinking_delta` become `StreamEvent::ReasoningDelta` → `LoopEvent::ReasoningDelta`, rendered to stderr as it arrives. Shown and let go: nothing here is replayed. | `harness-responses/src/lib.rs`, `harness-messages/src/sse.rs`, `harness-cli/src/render.rs` |
| 12 | `b10x-harness chat`: one line at a time on one session, persisted after every turn, `exit` or EOF ends it. No line editing — a shell has that. | `harness-cli/src/lib.rs` `chat_command`, `end_to_end.rs::chat_carries_one_turn_into_the_next_over_one_session` |
| 13 | **Three of five done the same day, by decision** (`docs/design/0002-sub-agents-structured-output-hooks.md`): sub-agents as a loop-owned `delegate` tool (a second loop inside the tool call, same gate, fresh context, remainder of the budget); structured output as a loop-owned `answer` tool the model calls to finish (`--output-schema`, stdout is the JSON, prose exits 2); hooks as a port with the process runner in the shell (`--hooks`, `before-call`/`after-call`/`stop`, narrowing only, never discovered). **The MCP client and multimodal input stay out of scope**, with the reason in `README.md` § Not owned here. | `crates/harness-loop/src/{answer,delegate,hook}.rs`, `crates/harness-cli/src/hooks.rs` |

Two findings outside the numbered list were closed in the same pass, both from peers rather than
from this review: a run refused **before** the loop starts now writes `{"kind":"refused","reason":…}`
under `--json` and exits 1, so a driver never again sees a status and an empty stream; and the argv
surface is pinned as a contract, `contracts/cli/b10x-harness/2026-08-29/`, checked from both
directions.

## Verdict

| | |
|---|---|
| The loop (`harness-loop`) | correct, bounded, well-tested (368 tests, `STATUS.md`) |
| The safety envelope | **stronger** than SOTA — see § Better than SOTA |
| The environment the model gets | **weaker** than SOTA in 7 measurable ways — see § Wrong |
| Supervision by a person | **absent** — no interactive approver, no session, no resume |

## Wrong — ranked by consequence for a real person

| # | consequence | mechanism | evidence |
|---|---|---|---|
| 1 | **A network blip on turn 20 of a $1 run loses the run, and nothing can resume it.** | A stream that fails after its first byte is final (`WitnessedSink`); the loop maps that to `LoopError::Wire` and exits. No transcript is persisted; `LoopOutcome.items` is documented as "ready to be replayed into a following run" and nothing replays it. SOTA discards the partial turn and retries it, and persists the session. | `crates/harness-responses/src/lib.rs:95-115` (retry rule), `crates/harness-loop/src/lib.rs:596-604` (`Err → return Err`), `crates/harness-loop/src/lib.rs:80-82` (`items` doc), `crates/harness-cli/src/lib.rs` `run_command` (no write of items) |
| 2 | **A person at the terminal cannot approve one write and refuse the next.** It is `--yes` (approve everything) or nothing (refuse everything). | Exactly two `ApprovalPort` impls exist: `DenyAll`, `ApproveAll`. No TTY prompt, no allow-once/allow-always. | `grep -rn "impl ApprovalPort" crates/` → 2 hits, `crates/harness-loop/src/approval.rs:38,54`; `crates/harness-cli/src/lib.rs` `run_command`: `if options.yes { ApproveAll } else { DenyAll }` |
| 3 | **`--approve-up-to high` lets `run` and whole-file `file_write` through unasked but refuses every `file_edit`** — the inverse of the risk ordering; an unattended run is pushed toward rewriting whole files. | `needs_approval` = `risk > ceiling \|\| (NonIdempotent && mutates)`; `file_edit` is declared `NonIdempotent`. The idempotency clause was written for `harness-flow`'s `Repeat`, a crate no binary binds. | `crates/harness-wire/src/envelope.rs` `needs_approval`; `crates/harness-tools/src/catalogue.rs` `file_edit()` → `writing(Idempotency::NonIdempotent)`; `crates/harness-cli/Cargo.toml` has no `harness-flow` dependency |
| 4 | **The model spends 33–44 % of its tool calls discovering tools** (their own measurement) and the provider cannot validate arguments. | Three-verb indirection `tool_search/tool_describe/tool_invoke`; `tool_invoke.arguments` is an untyped `object`, `strict: false`. Mitigated by pasting the catalogue into the instruction, but the schema loss stays. The stated reason — neutral names across harnesses — is already met by the entry names (`file_read`, `file_write`, …) published flat. SOTA publishes flat tools. | `crates/harness-tools/src/verbs.rs:1-22`, `crates/harness-tools/src/catalogue.rs` `brief()` doc ("33% to 44% of every tool call"), `crates/harness-responses/src/project.rs` `tool_to_wire` (`"strict": false`) |
| 5 | **The tail of a `cargo test` run — where the summary is — is what gets cut.** | Unconfined `run` keeps the **first** 64 KiB of each stream and drops the rest (`drain`). SOTA keeps head + tail. Confined path caps at 1 MiB with `stdout_truncated`. | `crates/harness-tools/src/local.rs:46` (`MAX_RUN_OUTPUT_BYTES`), `drain()` (`room = cap - kept`); `crates/harness-substrate/src/embedded.rs:416,431` |
| 6 | **A file over 256 KiB can never be read whole, and the middle of any file is unreachable without reading its head.** No line numbers in what is read, which SOTA uses to make edits land. | `file_read` takes `path` + `max_bytes` only (ceiling 256 KiB), reads from byte 0, answers JSON-escaped text. No `offset`, no line range. | `crates/harness-tools/src/catalogue.rs` `file_read()` schema; `crates/harness-tools/src/local.rs:38-39` (`MAX_READ_BYTES*`), `read()`; `grep -n "offset\|start_line" catalogue.rs` → 0 |
| 7 | **Finding a file costs one turn per directory level; a regex or a `*.rs`-only search is impossible.** | No glob/find tool. `dir_list` is non-recursive (500-entry cap). `search` is literal substring only, no file filter, no context lines, skips files > 1 MiB, depth ≤ 12, ≤ 200 matches. | `crates/harness-tools/src/catalogue.rs` `search()`/`dir_list()` schemas; `crates/harness-tools/src/local.rs:37-42,44` (caps), `grep()`, `walk()` |
| 8 | **N independent reads cost N × model-round-trip latency.** | Tool calls of one turn run strictly in order; the whole stack is blocking (`reqwest::blocking`, tokio only as a `block_on` shim). SOTA runs independent read-only calls concurrently. | `crates/harness-loop/src/lib.rs:682` ("Runs the turn's calls in order"), `run_calls`; `crates/harness-substrate/Cargo.toml` tokio comment |
| 9 | **A long run hits the provider's context wall with a hard error; ~60 % of a 128k window is never used.** | Compaction is a fixed **byte** bound (`192 KiB ≈ 50k tokens`) that elides old tool-result payloads only. `--context-window` is validated non-zero and otherwise unused by the loop. No summarisation; user/assistant/reasoning items are never touched, so a run whose weight is reasoning blobs has no strategy. SOTA: token-aware threshold near the window + LLM summary. | `crates/harness-loop/src/lib.rs:293` (`MAX_CONVERSATION_BYTES`), `compact()`; `grep -rn context_window crates/*/src` → only `Endpoint::new` validation |
| 10 | **The model does not know what repository it is in unless told.** | Standing instruction = 6 lines + catalogue + optional `--context <file>`. No cwd/OS/date/git-state block, no `AGENTS.md`/`CLAUDE.md` discovery. SOTA injects both. | `crates/harness-cli/src/lib.rs:46-52` (`DEFAULT_INSTRUCTIONS`), `standing_instruction()`; `grep -n "AGENTS.md\|CLAUDE.md" crates/harness-cli/src/lib.rs` → only a test fixture name |
| 11 | **A person watching a long think sees nothing.** | `response.reasoning_summary_text.delta` is in the ignored list. | `crates/harness-responses/src/lib.rs:457` |
| 12 | **One question, one answer, exit.** No follow-up turn, no REPL, no steering mid-run. | `run` takes one `--input`; bridge mode refuses `turn/steer` by name. | `crates/harness-cli/src/lib.rs` `RunOptions.input`; `README.md` § Two shells |
| 13 | Declared out of scope, and it is the remaining gap: no sub-agents, no hooks, no MCP client, no multimodal input, no structured output. | — | `README.md` § Not owned here |

## Better than SOTA — keep

| what | evidence |
|---|---|
| Credential custody: fetched per call, zeroized on drop, no `Display`, no ambient fallback | `crates/harness-wire/src/bearer.rs`; `AGENTS.md` § Safety envelope |
| `run` is an argv over a declared program set, never a shell; the toolset is derived from what the machine can confine | `crates/harness-tools/src/catalogue.rs` `run()`, `Catalogue::of`; `docs/design/0001` § 3 |
| Every bound refuses by name; nothing is truncated silently; absence stays absence | `crates/harness-wire/src/bound.rs`; `AGENTS.md` invariants 7–8 |
| Approval is a blocking call before the effect, and what asks is derived from the invoked entry, not declared by the tool | `crates/harness-loop/src/lib.rs` `invoke()`; `ToolPort::invoked` |
| Wire neutrality: opaque reasoning items typed by wire, cross-wire replay is a typed refusal; second wire landed with the loop unchanged | `crates/harness-wire/src/turn.rs` `check_opaque_items`; `crates/harness-messages/src/lib.rs:9-24` |
| Contract pins checked from both directions; real-socket emulated suites; 368 tests | `scripts/gate.sh`; `STATUS.md` § Test counts |
| Cost accounting from a dated rate card; compaction cost measured on live runs, not guessed | `crates/harness-loop/src/price.rs`; `lib.rs:296-323` |

## Size

| measure | value |
|---|---|
| source lines, all crates | 21,062 (`cat crates/*/src/*.rs \| wc -l`) |
| of which comment lines | 4,080 |
| test lines (`tests.rs` + `tests/`) | 7,195 |
| `harness-flow` | 1,891 lines, bound by no binary (`crates/harness-cli/Cargo.toml`) |
| bridge mode | never driven by a real client (`STATUS.md` § Bridge mode) |

## Recommended order

Fix in this order; each row is one story.

| order | change | closes |
|---|---|---|
| 1 | Persist the transcript per run (`items` + events), add `run --resume <id>`, retry a turn whose stream broke mid-way by discarding the partial turn | #1, #12 |
| 2 | A TTY `ApprovalPort` (allow-once / allow-always-for-this-entry / deny) as the default when stdin is a terminal | #2 |
| 3 | Drop the idempotency clause from `needs_approval`; `file_edit` risk `Medium` like `file_write` | #3 |
| 4 | Publish the catalogue entries flat, with `strict: true` schemas; keep the verbs only for MCP/eval arms if the eval still needs them | #4 |
| 5 | Head+tail output truncation for `run`; `file_read` gains `offset`/`limit` in lines and returns numbered lines; `search` gains regex + glob filter + context lines; add `find` (glob) | #5, #6, #7 |
| 6 | Token-aware compaction threshold from `--context-window`; LLM summary of elided prefix; keep the monotone rule | #9 |
| 7 | Env block (cwd, OS, date, git branch/status) + `AGENTS.md` discovery into the standing instruction | #10 |
| 8 | Run read-only calls of one turn concurrently (envelope says which are pure) | #8 |
| 9 | Forward reasoning summary deltas as a `LoopEvent` | #11 |
