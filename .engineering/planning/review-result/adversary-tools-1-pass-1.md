---
format: aep.planning-md/3
id: review-result:adversary-tools-1-pass-1
kind: review-result
status: active
title: Adversary pass 1 on story:harness-consumes-tools-crates (tools-1 unit 1fb7375a)
relations:
- reviews: story:harness-consumes-tools-crates
revision: 1
---
tools-1 U1 adversary pass 1 — CONFIRMED, 2 notes, no blocker, no warning. HEAD 1fb7375a over base 80a52fe5.

Added one green regression case `combined_offset_and_byte_ceiling_windows_answer_the_same_in_every_workspace`
(`crates/harness-substrate/tests/conformance.rs`): LocalOperations (tool_fs), ConfinedOperations and
Split answer `file_read` the same across 8 shapes × 156 windows with offset>1 and max_bytes set in
one window (1248 questions) — the combination the implementor's 442-question sweep never forms.
Green: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 34 filtered out`.

Findings (both notes, introduced):

1. `crates/harness-catalogue/src/local.rs:480` — `dir_list` delegates to `tool_fs`, which reports a
   non-regular entry (socket/FIFO/device) as `kind:"other"`, `bytes:null`; the pre-move reader
   (`harness-tools/src/local.rs:245-252`) reported `kind:"file"` with the entry's byte length. Not
   byte-for-byte for that edge; regular files, dirs and symlinks unchanged. Reachable, uncommon;
   tools' answer is arguably more correct. The acceptance pins tool names, not field values.

2. `crates/harness-catalogue/src/local.rs:111` — `MAPPING` / `tools_operation` / `harness_operation`
   are public and tested both ways, but no production path consults them: each operation hardcodes
   its tool call (`:243` `self.fs.read`, `:498` `self.fs.glob`, `:517` `editor.write`, `:540`
   `editor.edit`). The doc at `:31-33` ("the one place a harness name meets a tools id") overstates —
   the routing hardcodes the mapping, so the table can drift from it with every test still green.

Residue (not findings): stale doc comments cite deleted paths (`conformance.rs:1006,1010`,
`harness-catalogue/src/{lib.rs:48,operations.rs:5}`, `harness-cli/src/workflow.rs:1545`,
`harness-loop/src/tests.rs:20`); `file_read` reply field order changed (semantically irrelevant).

Attacked and could not break: the byte-ceiling separator fix across combined offset+max_bytes
windows (1248 questions agree, mirrors tool_fs `admit` exactly); the sha256/diff strip (no digest
leaks in any reply or error body); the read-only gate (`run_under_deny_all.rs` is a real
behavioural test; writers at Risk::Medium, run at Risk::High, published only on `operations.writes()`);
the name-mapping round-trip; the deletions (no code dependency on the deleted crates remains;
Cargo.lock carries the four tools crates at tag 0.1.0 rev 046dfb14; invariant 2 holds).

Not independently run: `cargo xtask cli-contract` (contracts/ untouched by the diff), the full gate.

```findings
[{"file": "crates/harness-catalogue/src/local.rs", "line": 480, "category": "contract-drift", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "dir_list now delegates to tool_fs and reports a non-regular entry (socket/FIFO/device) as kind other with bytes null, where the pre-move reader reported kind file with the entry's byte length; regular files, dirs and symlinks unchanged."},
{"file": "crates/harness-catalogue/src/local.rs", "line": 111, "category": "judgement", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "the MAPPING table and tools_operation/harness_operation are public and tested both ways but consulted by no production routing (each operation hardcodes its tool call), so the table can drift from the routing with every test still green despite its doc claiming to be the one place a harness name meets a tools id."}]
```
