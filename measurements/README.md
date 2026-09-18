# The measurement apparatus for `epic:measured-not-emulated`

This directory is the **apparatus**: the rate card, the fixture, the scorer and the scripts that
make one of that epic's measurements producible end to end — plus the records of what was produced
with it. The apparatus is not the measurement. What it buys is that a paid run is one confirmation
instead of several explorations, and that the figures it reports come from typed events production
already emits rather than from instrumentation added to see them.

| measurement | story | where it stands |
| --- | --- | --- |
| compaction against a real context window | `story:compaction-measured-live` | **the trigger and the ratio are measured**, once, on 2026-09-18 — `runs/m1-tier-a-2026-09-18.jsonl`. The summary prompt is not, and is still owed |
| the flat surface against the three verbs | `story:flat-surface-measured` | open; needs N ≥ 3 paid pairs |

Everything here except `runs/m1-tier-a-2026-09-18.jsonl*` and its session file was produced without
a provider, is free to reproduce, and is `provider_emulated`.

> **Invariant 18.** A rehearsal against the deterministic local endpoint is `provider_emulated`,
> and a rehearsal is **never the measurement**. It does not become one by being run again, by
> agreeing with a later live run, or by being the only evidence there is. No `STATUS.md` row may
> cite a rehearsal as though it were a run, and nothing under `runs/m1-dry-*` or `runs/m2-dry-*`
> closes any story in `epic:measured-not-emulated`.
>
> The one record in this directory that is **not** a rehearsal is `runs/m1-tier-a-2026-09-18.jsonl`,
> which is `vendor_live`: a real request to `https://api.anthropic.com/v1` on
> `claude-haiku-4-5-20251001`, paid for, operator-authorized. It is evidence about compaction's
> trigger and ratio **on that route and that model**, and about nothing else — it promotes no
> contract, re-pins no wire, and says nothing about the Messages route or any other model. The
> rehearsals beside it stay `provider_emulated` after it, exactly as before it.

## What is here

| path | what it is |
| --- | --- |
| `rates/claude-haiku-4-5-2026-09-18.json` | the dated rate card the paid run needs. Without one, `--max-cost-microunits` is unenforceable and the run refuses outright |
| `rates/b10x-emulated-2026-09-18.json` | the same figures keyed to the emulator's own model, so the rehearsal exercises the pricing path. Not a vendor rate and says so |
| `fixtures/make-compaction-workspace.py` | materialises the compaction workspace deterministically; `--check` proves a tree is the one this file describes; `--keys` prints the answers |
| `prompts/compaction-task.txt` | the compaction task: read eight chapters, then report each one's key |
| `prompts/surface-task.txt` | the surface task: find out what tools exist, then use them. It must require discovery or the cost being compared vanishes |
| `score.py` | reads a `run --json` record and reports the figures. A reader over an event stream, not an evaluation harness |
| `dry-run-compaction.sh` | one free rehearsal of the compaction measurement |
| `sweep-window.sh` | the same at several declared windows |
| `dry-run-surfaces.sh` | one free walk per surface, proving the surface half of the scorer |
| `prove-spend-ceiling.sh` | proves `--max-cost-microunits` is enforceable for the model the paid run names, without naming a provider |
| `runs/m1-dry-*`, `runs/m2-dry-*` | records the free rehearsals wrote. `provider_emulated` |
| `runs/m1-tier-a-2026-09-18.jsonl` | the one paid run: `vendor_live`, Tier A compaction, 2026-09-18. `.stderr` beside it is the same run's progress stream |

Nothing in production was instrumented for any of this. Every figure comes from a typed event the
loop already emits — `crates/harness-loop/src/event.rs`, `{"kind": ...}` in kebab-case. What was
added is a fixture, a scorer, and one emulator scenario.

## The trap the rehearsal caught

`PREP-measured-not-emulated.md` § 5.1 proposed a rehearsal needing no new code: start the shipped
`flat-tool` scenario, declare `--context-window 50`, and the record "should contain a
`{"kind":"compacted", ...}` object".

**It does not.** That run emits no `compacted` event at all — not one with `elided_results: 0`,
which is what the prep document expected the failure to look like, but none, which is worse: a
scorer looking for the event cannot tell "compaction never fired" from "compaction fired and freed
nothing". `compact_run` returns without emitting when `elided.count == 0 && summarised == 0 &&
!summary_turn`.

Two properties of the fixture cause it, and both are invisible until something runs:

1. **`elide` protects the newest tool result unconditionally** — "at least one result always
   survives, because a model that cannot see the result of the call it just made is stuck". The
   `flat-tool` scenario makes exactly one `file_read` call, so the only result there is is the
   protected one, `elidable` is zero, and `elided_results` can never exceed 0 at any declared
   window. A rehearsal built on that scenario proves nothing and would have sent the paid run out
   with an apparatus that cannot report its first figure.
2. **`SUMMARY_MIN_FOLD_BYTES` is 8 KiB.** A conversation smaller than that is never folded, so the
   summary branch is skipped too, and with elision contributing nothing the event is suppressed.

The prep document named `KEPT_RESULT_BYTES` (48 KiB) as the mechanism. That is not it:
`protected_bytes` is `KEPT_RESULT_BYTES.min(target / 2)`, so the 48 KiB floor is not binding at a
small window — the code already guards the case the prep document feared. The binding rule is the
unconditional one-result floor, which no window can relax.

It also named the emulator's hardcoded `input_tokens=42` as what clears a 50-token window's 80%
threshold. It is not needed: occupancy is `max(estimated_tokens(bytes), reported_input)`, and the
byte estimate of any real conversation clears 40 on its own.

So the fixture has to produce **several** tool results and enough bulk to fold. That is what
`fixtures/make-compaction-workspace.py` and the `compaction` emulator scenario are for.

## What the rehearsal then measured

At `--context-window 16000` against the fixture, on both wires:

```
compactions 1  turns  9  finished/completed  [provider_emulated]
    fired at 90.3% of the declared window (14445 tokens, on the loop estimate);
    left 0.4598 of the bytes; elided 4 result(s); summary_turn=False summarised=0
    keys: 4 kept, 4 lost, 0 unanswered
```

Four things worth carrying into the paid run, all `provider_emulated`:

* **The trigger overshoots by a whole turn.** Occupancy is checked *between* turns, so the run
  crosses 80% during a turn and compacts after it. At a 16000 window that is 90.3%; at 8000 the
  first firing is 104%; at 4000 every firing is 105–119%. Once one tool result is a large fraction
  of the window the loop is always past the declared wall when it acts. `COMPACTION_TRIGGER_PERCENT
  = 80` is therefore a floor, not the trigger, and the paid run should report the figure it
  actually fired at rather than the constant.
* **A window below about twice one tool result cannot be reached.** At 2500 the run compacts seven
  times and the conversation settles at 4.2–4.8k tokens regardless, because the protected result
  alone exceeds the target. Do not declare a Tier A window smaller than that.
* **The summary turn never fired, at any window tried.** Elision alone reached the target every
  time on a conversation whose weight is in tool results. `STATUS.md:18` asks about "the trigger,
  the ratio and the summary prompt"; on this fixture the paid run will very likely measure the
  first two and leave the third untouched. That is a decision to take before spending, not after.
* **It fired on the loop's own estimate, not the reported count.** Expected here: the emulator
  derives its count from the request bytes, and `measure(items)` is the larger figure. On a real
  provider the reported count also carries the instruction and the tool schemas, which are not in
  `items` at all, so `fired_on` should read `provider-reported count`. If the paid run reports
  `loop estimate`, the provider is under-reporting relative to `measure(items)/4` — the surprise
  the story is looking for, and the scorer names it in one field.

The `keys` line is the fourth figure made checkable: each chapter states one unguessable token, and
the emulator answers out of the transcript it was actually sent, so an elided chapter is reported
`lost` rather than invented. Four kept and four lost is the elision working *and* the measurement
having teeth. On a real model this figure is observed once and is a judgement about output, never
arithmetic — `score.py` says so in the field beside it.

## The rate card, and why it is keyed twice

`Budget::validate` refuses `max_cost_microunits` by name on a run it cannot price, and the command
line validates again before the event record begins, so a cap without a card that prices the
model **stops the run before it starts**. `prove-spend-ceiling.sh` shows all three cases free:

```
A  cap, no card at all           -> refused: `max_cost_microunits` cannot be enforced
B  cap, a card pricing some other model -> refused, identically
C  cap, the dated Haiku card     -> accepted; started.model claude-haiku-4-5-20251001
```

Case C also catches the second trap. Enforceability is decided from the model the run **asked
for**; each turn is priced against the model the provider **reported**. When those differ, the run
starts, spends, and then stops with

```
BudgetUnobservable { name: "max_cost_microunits",
  reason: "a model request omitted usage or reported usage the declared rate card cannot price" }
```

— after the money is gone. The card therefore prices both `claude-haiku-4-5-20251001` (what
`--model haiku` resolves to) and `claude-haiku-4-5` (the family identifier), so a route that
answers as either is priced and the ceiling holds.

## Two corrections to the prep document's command

* There is no `--profile claude`. `-p/--profile` names a *profile* — what a run may do — and a
  profile names a provider. The provider is selected by `[default] provider = "claude"` in
  `$XDG_CONFIG_HOME/b10x/harness.toml`, or by a `[[profiles]]` entry that sets `provider`.
* `providers show claude` prints the built-in default model, `claude-opus-5`, not what `--model
  haiku` will resolve to. The resolved identifier is in `started.model`, which is where the rate
  card's key has to match.

## The paid runs

### Tier A, taken — 2026-09-18, `runs/m1-tier-a-2026-09-18.jsonl`

Operator-authorized. One run of the compaction fixture against `https://api.anthropic.com/v1` on
`claude-haiku-4-5-20251001`, at a **declared** `--context-window` of 16000. Read it with

```
python3 score.py compaction runs/m1-tier-a-2026-09-18.jsonl \
    --context-window 16000 --expect-keys --require-compaction
```

| figure | value |
| --- | --- |
| compactions | 1 |
| fired at | 14,662 occupied tokens — **91.6%** of the declared window |
| `fired_on` | `loop estimate`, not the provider-reported count |
| freed | 4 results, 31,856 bytes; 58,650 → 27,438 bytes, **0.4678** of the conversation left |
| summary turn | **none.** `summary_turn: false`, `summarised_items: 0` |
| terminal | `completed`, 9 turns, 0 retries |
| chapter keys | **8 of 8 recoverable** — observed once, a judgement about output rather than arithmetic |
| usage | 72,511 input (42,896 cached read, 23,526 cache write), 824 output |
| cost | **43,905 micro-USD — $0.043905**, the sum of the record's own `cost` events |

The rehearsal predicted three of these and was right about all three: the trigger overshoots
(90.3% rehearsed, 91.6% live), elision alone reaches the target so no summary turn fires, and the
figure is the loop's own estimate rather than the provider's count. That last one is the finding
the story was looking for: the provider's reported input count stayed **below** `measure(items)/4`
even though a real provider's count also carries the instruction and the tool schemas, which are
not in `items` at all.

**One redaction, and nothing else.** Two absolute paths in the committed record — the profile
source and the session file — carried the operator's home directory, which names nobody's machine
anybody needs and which two separate checks refuse in a tracked file:
`scripts/check-no-home-paths.py` here, and the organisation's shared `personal-paths` rule in
`b10x-gates`. The two do not agree on what a redaction may look like, and the stricter one wins:
the local check treats one account name as a documentation placeholder, the shared rule treats
**any** user directory as a finding whatever the account is called. So neither path was rewritten
to another user directory. The profile source is now the environment variable it actually comes
from, and the session file is now the repository-relative path it actually occupies here.

No figure, event, token count, digest or ordering was touched. The record still parses, and
`score.py` still reports the same numbers from it — which is the test that the redaction touched
nothing that carries a measurement.

**What this run bought, and what it did not.** It is `vendor_live` for the **trigger** and the
**ratio**, on the Responses route and `claude-haiku-4-5-20251001`. It is not evidence for the
**summary prompt**, which never fired — measuring that needs a fixture whose weight is *not* in
tool results, because elision reaches the target first on one whose weight is. It promotes no
contract and re-pins no wire. `story:compaction-measured-live` keeps its third figure open.

### Tier B, not taken

`story:flat-surface-measured` still needs N ≥ 3 paid pairs and is not authorized. Run
`prove-spend-ceiling.sh` and `dry-run-surfaces.sh` first; they are free, and they are why the paid
runs only have to be run once.
