//! `run` through the real adapter, held to what harness 0.15.1 answered for an ordinary command and
//! to what `CHANGELOG.md` `[Unreleased]` says changed when it moved onto `b10x-tool-process`.
//!
//! The adapter's own tests compare its reply with `tool_process`'s, which is the same code on both
//! sides: a change in the pinned crate moves both and stays green. These cases write the 0.15.1
//! reply out as literals instead, so a reply change reaches a red test and not only a changelog.

#![cfg(unix)]

use harness_catalogue::{LocalOperations, Operations, Refusal};
use serde_json::json;

const SHELL: &str = "/bin/sh";

/// A two-byte prefix, 6,000 lines of 13 bytes that each carry a two-byte `é`, and a 14-byte
/// suffix on stdout, one line on stderr, and exit 3: 78,016 bytes, laid out so that both of
/// 0.15.1's cuts fall inside an `é`.
const SCRIPT: &str = "printf '> '; i=0; while [ $i -lt 6000 ]; do printf '%04d é line\\n' $i; \
                      i=$((i+1)); done; printf 'done, exit 3.\\n'; printf 'to stderr\\n' >&2; \
                      exit 3";

/// What [`SCRIPT`] writes to stdout, byte for byte.
fn stream() -> Vec<u8> {
    let mut bytes = b"> ".to_vec();
    for line in 0..6000 {
        bytes.extend_from_slice(format!("{line:04} é line\n").as_bytes());
    }
    bytes.extend_from_slice(b"done, exit 3.\n");
    bytes
}

fn running() -> (tempfile::TempDir, LocalOperations) {
    let dir = tempfile::tempdir().expect("a temporary workspace");
    let operations =
        LocalOperations::unconfined(dir.path(), vec![SHELL.to_owned()]).expect("a workspace");
    (dir, operations)
}

fn sh(script: &str) -> Vec<String> {
    vec![SHELL.to_owned(), "-c".to_owned(), script.to_owned()]
}

#[test]
fn an_ordinary_run_answers_0_15_1s_reply_field_for_field() {
    // 0.15.1 kept the first and the last 32768 bytes of each stream. The head's last byte, at
    // 32767, opens an `é` the cut breaks, so the head ends one byte short; the tail opens on the
    // second byte of an `é`, at 45248, so it starts one byte late. 78016 - 32767 - 32767 = 12482.
    let stream = stream();
    assert_eq!(stream.len(), 78_016, "the stream this case is laid out for");
    assert_eq!(stream[32_767], 0xc3, "the head's cut falls inside an `é`");
    assert_eq!(stream[45_248], 0xa9, "the tail's cut falls inside an `é`");
    let head = std::str::from_utf8(&stream[..32_767]).expect("whole characters");
    let tail = std::str::from_utf8(&stream[45_249..]).expect("whole characters");

    let (_dir, operations) = running();
    let answer = operations
        .run(&sh(SCRIPT))
        .expect("the declared program runs");
    let expected = json!({
        "argv": [SHELL, "-c", SCRIPT],
        "exit": 3,
        "stdout": format!("{head}\n… 12482 bytes omitted here …\n{tail}"),
        "stderr": "to stderr\n",
        "truncated": true,
        "omitted_bytes": 12_482,
        "timed_out": false,
        "timeout_ms": 600_000,
    });
    assert_eq!(answer, expected);
}

#[test]
fn a_short_run_is_answered_whole_with_nothing_omitted() {
    let (_dir, operations) = running();
    let answer = operations
        .run(&sh("printf 'one\\ntwo\\n'; printf 'warn\\n' >&2"))
        .expect("the declared program runs");
    assert_eq!(
        answer,
        json!({
            "argv": [SHELL, "-c", "printf 'one\\ntwo\\n'; printf 'warn\\n' >&2"],
            "exit": 0,
            "stdout": "one\ntwo\n",
            "stderr": "warn\n",
            "truncated": false,
            "omitted_bytes": 0,
            "timed_out": false,
            "timeout_ms": 600_000,
        })
    );
}

#[test]
fn a_program_ended_by_a_signal_answers_a_null_exit() {
    let (_dir, operations) = running();
    let answer = operations
        .run(&sh("kill -KILL $$"))
        .expect("the declared program runs");
    assert_eq!(
        answer,
        json!({
            "argv": [SHELL, "-c", "kill -KILL $$"],
            "exit": null,
            "stdout": "",
            "stderr": "",
            "truncated": false,
            "omitted_bytes": 0,
            "timed_out": false,
            "timeout_ms": 600_000,
        })
    );
}

#[test]
fn a_program_outside_the_declared_set_is_refused_in_0_15_1s_words_and_by_name() {
    let (_dir, operations) = running();
    let refused = operations
        .run(&["/bin/echo".to_owned(), "hi".to_owned()])
        .expect_err("refused");
    assert_eq!(
        refused.message(),
        "`/bin/echo` is not a program this run may start. Declared: /bin/sh."
    );
    assert_eq!(
        refused.refusal(),
        Some(&Refusal::ProgramNotDeclared {
            program: "/bin/echo".to_owned(),
            declared: vec![SHELL.to_owned()],
        })
    );
}

#[test]
fn a_declared_program_that_cannot_start_and_an_empty_argv_are_refused_in_0_15_1s_words() {
    let dir = tempfile::tempdir().expect("a temporary workspace");
    let absent = "/nonexistent/b10x-absent-program";
    let operations =
        LocalOperations::unconfined(dir.path(), vec![absent.to_owned()]).expect("a workspace");

    let refused = operations.run(&[absent.to_owned()]).expect_err("refused");
    assert_eq!(
        refused.message(),
        "`/nonexistent/b10x-absent-program`: No such file or directory (os error 2)"
    );
    assert_eq!(
        refused.refusal(),
        None,
        "a failure, not a rule of the run's"
    );

    let refused = operations.run(&[]).expect_err("refused");
    assert_eq!(refused.message(), "`argv` must name a program");
    assert_eq!(refused.refusal(), None);
}
