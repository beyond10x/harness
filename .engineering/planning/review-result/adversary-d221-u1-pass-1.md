---
format: aep.planning-md/3
id: review-result:adversary-d221-u1-pass-1
kind: review-result
status: active
title: Adversary pass 1 on the refusal escape (wave d221 U1)
relations:
- reviews: story:substrate-refusal-display-escapes-daemon-bytes
revision: 1
---
Adversary pass 1 (aep:adversary, opus) on `story:substrate-refusal-display-escapes-daemon-bytes`, unit commit 4850c06, cases committed as b74479f on `adv/d221-u1` (on top of wave merge 0d867cd).

```
unit: story:substrate-refusal-display-escapes-daemon-bytes (4850c06), covering adv/d221-u1 at b74479f (cases on top of 0d867cd)
verdict: NEEDS-CHANGE
cases: executed 118→120 (harness-substrate suite, my 2 cases excluded for the before count), plus harness-cli --test adversary_d221_u1 0→1; red 2
origin: introduced 0 / pre-existing 2 / undecided 0
needs-coordinator: whether the CLI status-0 bypass blocks this unit or gets its own story. It is the adopt stderr route the story's context names.
```

1. Diff stat (0d867cd..HEAD) touches 2 files, both tests: `crates/harness-cli/tests/adversary_d221_u1.rs` (+90) and `crates/harness-substrate/tests/adversary_d221_u1.rs` (+86).
2. Cases, each run alone first:
   - `an_unreadable_file_answer_is_displayed_without_raw_control_or_bidi_characters` is red: raw ['\u{9b}', '\u{7f}', '\u{202e}'] in the Unreadable reason, `test result: FAILED. 1 passed; 1 failed`.
   - `a_refusal_escapes_the_wider_control_and_format_classes` is green. It covers U+0085, 009C/D/F, 200E/F, 2066-2069, 061C, 200B-D, FEFF, 2028 and E0041.
   - CLI `a_daemon_answering_status_zero_to_an_adopt_does_not_reach_stderr_raw` is red: raw ESC, BEL, C1, DEL, LF, U+202E in "error: `--substrate-adopt project` was refused: ...", `test result: FAILED. 0 passed; 1 failed`.
3. Suite: `cargo test -p b10x-harness-substrate --no-fail-fast` exited 101; the new Unreadable case is the only failure.
4. Findings:
   - Status-0 bypass. `crates/harness-cli/src/lib.rs:3520-3521` prints `body` raw in the status-0 arm. `client.rs:136` parses `HTTP/1.1 0` from the socket as status 0, so a peer can reach that arm. Pre-existing. What reaches it: `tools`/`run --substrate S --substrate-adopt N` with another program on the socket, the threat the story names.
   - Unreadable leaks. `client.rs:430`, `:624` and `:668` put the daemon's JSON into the `Unreadable` reason through serde_json's Display, which leaves DEL, C1 and bidi characters raw. Pre-existing, outside the story's scope. What reaches it: the file and exec tools pass the error's Display to the model through `tools.rs:451`; no terminal print seen on that route.
5. Could not break: Refused's Display over the wider Cf/Cc/Zl classes; the `body` field byte-for-byte; the 409 hint keyed on the raw body; Debug output; lossy-decoded U+FFFD; adopt Unreadable reasons using `quoted`/`{:?}`. Readability side effects only: combining marks and NBSP become `\u{..}`, a backslash in JSON bodies is doubled.

```findings
- file: crates/harness-cli/src/lib.rs
  line: 3521
  category: acceptance
  severity: blocker
  verdict: NEEDS-CHANGE
  origin: pre-existing
  message: a socket peer answering HTTP status 0 to an adopt lands in the client-own status-0 arm, which prints the daemon body unescaped to stderr (OSC/ESC/C1/U+202E reach the terminal)
- file: crates/harness-substrate/src/client.rs
  line: 430
  category: boundary
  severity: warning
  verdict: NEEDS-CHANGE
  origin: pre-existing
  message: Unreadable reasons quote daemon JSON with serde_json Display (also :624, :668), which leaves DEL, C1 and bidi-format characters raw in the error text
```
