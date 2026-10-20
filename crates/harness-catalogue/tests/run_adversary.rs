//! Adversary cases for `run` on `b10x-tool-process` (story:run-moves-onto-tool-process).
//!
//! The 0.15.1 side of each comparison is not a literal worked out by hand: it is harness 0.15.1's
//! own runner, copied into [`v0_15_1`] from `crates/harness-catalogue/src/local.rs` at `df37462b`
//! (its `exec` and helpers are byte-identical to tag `0.15.1`'s), run on the same machine against
//! the same workspace. A reply the move changed without `CHANGELOG.md` saying so reaches a red
//! case here, and so does a run that does something 0.15.1 never did.

#![cfg(unix)]

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use harness_catalogue::{Catalogue, LocalOperations, Operations};
use serde_json::{Value, json};

const SHELL: &str = "/bin/sh";
const ENV: &str = "/usr/bin/env";

fn sh(script: &str) -> Vec<String> {
    vec![SHELL.to_owned(), "-c".to_owned(), script.to_owned()]
}

fn owned(argv: &[&str]) -> Vec<String> {
    argv.iter().map(|item| (*item).to_owned()).collect()
}

/// Runs `argv` through the adapter and through 0.15.1's runner, and names how the two replies
/// differ, if they do. The serialised text is compared, so field order counts too.
fn compare(
    case: &str,
    operations: &LocalOperations,
    inherited: &[String],
    argv: &[String],
    remaining: Option<Duration>,
) -> Option<String> {
    let now = operations
        .run_within(argv, remaining)
        .map_err(|refused| refused.message().to_owned());
    let then = v0_15_1::exec(
        operations.root(),
        inherited,
        argv,
        operations.programs(),
        remaining,
    )
    .map_err(|refused| refused.message().to_owned());
    match (now, then) {
        (Ok(now), Ok(then)) => {
            // As text, so a field moved out of its place counts as a difference too.
            let (ours, theirs) = (now.to_string(), then.to_string());
            (ours != theirs).then(|| format!("{case}: {}", difference(&now, &then)))
        }
        (Err(now), Err(then)) => {
            (now != then).then(|| format!("{case}: refused `{now}`, 0.15.1 refused `{then}`"))
        }
        (now, then) => Some(format!("{case}: {now:?} against 0.15.1's {then:?}")),
    }
}

fn difference(now: &Value, then: &Value) -> String {
    let (Some(ours), Some(theirs)) = (now.as_object(), then.as_object()) else {
        return format!("{now} against 0.15.1's {then}");
    };
    let our_keys: Vec<&String> = ours.keys().collect();
    let their_keys: Vec<&String> = theirs.keys().collect();
    if our_keys != their_keys {
        return format!("fields {our_keys:?} against 0.15.1's {their_keys:?}");
    }
    for (key, value) in ours {
        let old = &theirs[key];
        if value == old {
            continue;
        }
        return match (value.as_str(), old.as_str()) {
            (Some(ours), Some(theirs)) => {
                let at = ours
                    .bytes()
                    .zip(theirs.bytes())
                    .position(|(a, b)| a != b)
                    .unwrap_or(ours.len().min(theirs.len()));
                format!(
                    "`{key}` differs at byte {at}, {} bytes against 0.15.1's {}: {:?} against {:?}",
                    ours.len(),
                    theirs.len(),
                    window(ours, at),
                    window(theirs, at)
                )
            }
            _ => format!("`{key}` is {value} against 0.15.1's {old}"),
        };
    }
    "the same fields and values, in a different order".to_owned()
}

fn window(text: &str, at: usize) -> String {
    let bytes = text.as_bytes();
    let from = at.saturating_sub(8).min(bytes.len());
    let to = (at + 16).min(bytes.len());
    String::from_utf8_lossy(&bytes[from..to]).into_owned()
}

/// `len` bytes of lines of ASCII.
fn ascii(len: usize) -> Vec<u8> {
    let letters = b"abcdefghijklmnopqrstuvwxyz";
    (0..len)
        .map(|at| {
            if at % 64 == 63 {
                b'\n'
            } else {
                letters[at % 26]
            }
        })
        .collect()
}

/// `prefix` ASCII bytes, then `character` repeated, then ASCII padding: `total` bytes of valid
/// UTF-8, laid so that each cut falls at a different offset inside a character as `prefix` moves.
fn straddling(character: &str, prefix: usize, total: usize) -> Vec<u8> {
    let mut bytes = vec![b'x'; prefix];
    while bytes.len() + character.len() <= total {
        bytes.extend_from_slice(character.as_bytes());
    }
    bytes.resize(total, b'y');
    bytes
}

/// A fixed-seed xorshift, so a red case names the same streams on every machine.
struct Seeded(u64);

impl Seeded {
    fn below(&mut self, bound: usize) -> usize {
        let mut next = self.0;
        next ^= next << 13;
        next ^= next >> 7;
        next ^= next << 17;
        self.0 = next;
        usize::try_from(next % u64::try_from(bound).expect("a small bound")).expect("below bound")
    }

    fn text(&mut self, at_least: usize) -> String {
        const PALETTE: [&str; 9] = ["a", "z", "\n", "é", "ß", "€", "中", "😀", "🦀"];
        let mut text = String::new();
        while text.len() < at_least {
            text.push_str(PALETTE[self.below(PALETTE.len())]);
        }
        text
    }
}

fn streams(
    operations: &LocalOperations,
    case: &str,
    out: &[u8],
    err: &[u8],
    code: usize,
) -> Option<String> {
    let root = operations.root();
    std::fs::write(root.join("out.bin"), out).expect("stdout's bytes");
    std::fs::write(root.join("err.bin"), err).expect("stderr's bytes");
    compare(
        case,
        operations,
        &[],
        &sh(&format!("cat out.bin; cat err.bin >&2; exit {code}")),
        None,
    )
}

#[test]
fn ordinary_runs_answer_0_15_1s_reply_byte_for_byte() {
    let dir = tempfile::tempdir().expect("a workspace");
    let operations =
        LocalOperations::unconfined(dir.path(), vec![SHELL.to_owned()]).expect("a workspace");
    let mut differences = Vec::new();

    // Each stream at, below and above the bound: 32768 a side, 65536 whole.
    for len in [
        0, 1, 32_767, 32_768, 32_769, 65_535, 65_536, 65_537, 65_538, 65_539, 98_304, 200_001,
    ] {
        differences.extend(streams(
            &operations,
            &format!("ascii {len}"),
            &ascii(len),
            &ascii(len / 3),
            len % 7,
        ));
    }
    // Valid UTF-8 with both cuts at every offset inside a two-, three- and four-byte character.
    for character in ["é", "€", "😀"] {
        for prefix in 0..character.len() {
            for total in [65_537, 65_538, 65_541, 70_001, 100_003] {
                let bytes = straddling(character, prefix, total);
                differences.extend(streams(
                    &operations,
                    &format!("{character} prefix {prefix} total {total}"),
                    &bytes,
                    &bytes,
                    0,
                ));
            }
        }
    }
    // Random valid UTF-8 on both streams, fixed seed.
    let mut seeded = Seeded(0x5eed_0fad_7e25_aa01);
    for round in 0..12 {
        let out_len = 60_000 + seeded.below(80_000);
        let err_len = seeded.below(140_000);
        let out = seeded.text(out_len);
        let err = seeded.text(err_len);
        differences.extend(streams(
            &operations,
            &format!("random round {round}"),
            out.as_bytes(),
            err.as_bytes(),
            round,
        ));
    }
    // Exit codes and signals, and both streams small.
    for script in [
        "exit 0",
        "exit 1",
        "exit 126",
        "exit 255",
        "kill -HUP $$",
        "kill -TERM $$",
        "kill -KILL $$",
        "kill -USR1 $$",
        "printf 'a\\nb'; printf 'warn' >&2; exit 42",
        "exec 1>&-; printf 'only stderr' >&2; exit 0",
    ] {
        differences.extend(compare(script, &operations, &[], &sh(script), None));
    }

    assert!(
        differences.is_empty(),
        "{} replies differ from 0.15.1's:\n{}",
        differences.len(),
        differences.join("\n")
    );
}

#[test]
fn refusals_and_the_declared_set_answer_0_15_1s_words() {
    let dir = tempfile::tempdir().expect("a workspace");
    let tool = dir.path().join("tool.sh");
    std::fs::write(&tool, "#!/bin/sh\nprintf 'relative %s' \"$1\"\n").expect("a script");
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755))
            .expect("executable");
    }
    let operations = LocalOperations::unconfined(
        dir.path(),
        owned(&[SHELL, "./tool.sh", "/nonexistent/b10x-adversary"]),
    )
    .expect("a workspace");
    let mut differences = Vec::new();
    for argv in [
        owned(&[]),
        owned(&["/bin/echo", "hi"]),
        owned(&["sh", "-c", "exit 0"]),
        owned(&["/usr/bin/sh", "-c", "exit 0"]),
        owned(&["/bin//sh", "-c", "exit 0"]),
        owned(&["./sh", "-c", "exit 0"]),
        owned(&["./tool.sh", "arg"]),
        owned(&["tool.sh", "arg"]),
        owned(&["/nonexistent/b10x-adversary"]),
    ] {
        differences.extend(compare(&format!("{argv:?}"), &operations, &[], &argv, None));
    }
    assert!(
        differences.is_empty(),
        "{} answers differ from 0.15.1's:\n{}",
        differences.len(),
        differences.join("\n")
    );
}

#[test]
fn the_environment_is_0_15_1s_name_for_name_with_and_without_inheriting() {
    const ALLOW_LIST: [&str; 12] = [
        "PATH",
        "HOME",
        "LANG",
        "LC_ALL",
        "TERM",
        "TMPDIR",
        "CARGO_HOME",
        "RUSTUP_HOME",
        "RUSTUP_TOOLCHAIN",
        "CARGO_TARGET_DIR",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ];
    assert!(
        std::env::var_os("CARGO_PKG_NAME").is_some()
            && std::env::var_os("CARGO_MANIFEST_DIR").is_some(),
        "precondition: the test process holds variables off the allow-list"
    );
    let dir = tempfile::tempdir().expect("a workspace");
    let named = owned(&[
        "CARGO_PKG_NAME",
        "CARGO_MANIFEST_DIR",
        "B10X_ADVERSARY_NEVER_SET",
    ]);
    let plain = LocalOperations::unconfined(dir.path(), vec![ENV.to_owned()]).expect("a workspace");
    let widened = LocalOperations::unconfined(dir.path(), vec![ENV.to_owned()])
        .expect("a workspace")
        .inheriting(named.clone());
    let argv = owned(&[ENV, "-0"]);

    let mut differences = Vec::new();
    differences.extend(compare("allow-list only", &plain, &[], &argv, None));
    differences.extend(compare(
        "inheriting three names",
        &widened,
        &named,
        &argv,
        None,
    ));
    assert!(differences.is_empty(), "{}", differences.join("\n"));

    for (operations, extra) in [(&plain, &[][..]), (&widened, &named[..])] {
        let answer = operations.run(&argv).expect("env runs");
        let started_with: BTreeSet<String> = answer["stdout"]
            .as_str()
            .expect("stdout")
            .split('\0')
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                entry
                    .split_once('=')
                    .map_or(entry, |(name, _)| name)
                    .to_owned()
            })
            .collect();
        let expected: BTreeSet<String> = ALLOW_LIST
            .iter()
            .map(|name| (*name).to_owned())
            .chain(extra.iter().cloned())
            .filter(|name| std::env::var_os(name).is_some())
            .collect();
        assert_eq!(started_with, expected, "inheriting {extra:?}: {answer}");
    }
}

#[test]
fn a_symlinked_root_runs_where_it_pointed_when_opened_as_0_15_1_did() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let real = dir.path().join("real");
    let other = dir.path().join("other");
    let link = dir.path().join("link");
    std::fs::create_dir(&real).expect("real");
    std::fs::create_dir(&other).expect("other");
    std::os::unix::fs::symlink(&real, &link).expect("link");
    let canonical = real.canonicalize().expect("canonical");
    let operations = LocalOperations::unconfined(&link, vec![SHELL.to_owned()]).expect("opens");
    let argv = sh("pwd -P");
    let mut differences = Vec::new();

    differences.extend(compare("through the link", &operations, &[], &argv, None));
    let answer = operations.run(&argv).expect("runs");
    assert_eq!(
        answer["stdout"],
        json!(format!("{}\n", canonical.display())),
        "{answer}"
    );

    std::fs::remove_file(&link).expect("the link goes");
    std::os::unix::fs::symlink(&other, &link).expect("and comes back elsewhere");
    differences.extend(compare(
        "after the link moved",
        &operations,
        &[],
        &argv,
        None,
    ));
    let answer = operations.run(&argv).expect("runs");
    assert_eq!(
        answer["stdout"],
        json!(format!("{}\n", canonical.display())),
        "{answer}"
    );

    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn the_comparison_sees_a_change_the_changelog_names() {
    // The guard on every "answers as 0.15.1 did" case above: one invalid byte at the very start
    // and an `é` across the head's cut. 0.15.1 left the cut through the `é`; `process.run` moves
    // it (`CHANGELOG.md`, "A cut moves only for a character it broke"). A comparison that saw no
    // difference here would make the cases above green whatever the runner answered.
    let dir = tempfile::tempdir().expect("a workspace");
    let operations =
        LocalOperations::unconfined(dir.path(), vec![SHELL.to_owned()]).expect("a workspace");
    let mut bytes = vec![0xff];
    bytes.resize(32_767, b'x');
    bytes.extend_from_slice("é".as_bytes());
    bytes.resize(70_000, b'y');
    let seen = streams(&operations, "an invalid first byte", &bytes, b"", 0);
    assert_eq!(
        seen.as_deref(),
        Some("an invalid first byte: `omitted_bytes` is 4465 against 0.15.1's 4464"),
    );
}

#[test]
fn a_bound_already_spent_or_shorter_than_the_program_answers_as_0_15_1_did() {
    let dir = tempfile::tempdir().expect("a workspace");
    let operations =
        LocalOperations::unconfined(dir.path(), vec![SHELL.to_owned()]).expect("a workspace");
    let mut differences = Vec::new();
    for remaining in [Duration::ZERO, Duration::from_millis(250)] {
        differences.extend(compare(
            &format!("sleep 1 with {remaining:?} left"),
            &operations,
            &[],
            &sh("sleep 1"),
            Some(remaining),
        ));
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn a_double_forked_descendant_holding_stdout_is_killed_at_the_bound_and_its_output_kept() {
    let dir = tempfile::tempdir().expect("a workspace");
    let operations =
        LocalOperations::unconfined(dir.path(), vec![SHELL.to_owned()]).expect("a workspace");
    let started = Instant::now();
    let answer = operations
        .run_within(
            &sh("printf before; (printf child; sleep 3 & printf '%s' $! > d.pid) & exit 0"),
            Some(Duration::from_millis(300)),
        )
        .expect("runs");
    let held = started.elapsed();
    let (pid, alive) = reap(dir.path(), "d.pid");
    assert_eq!(answer["stdout"], json!("beforechild"), "{answer}");
    assert_eq!(answer["exit"], json!(0), "{answer}");
    assert_eq!(answer["timed_out"], json!(true), "{answer}");
    assert!(
        held < Duration::from_millis(1_500),
        "held {held:?}: {answer}"
    );
    assert!(!alive, "double-forked {pid} outlived the call: {answer}");
}

#[test]
fn a_descendant_holding_only_stderr_is_bounded_too() {
    let dir = tempfile::tempdir().expect("a workspace");
    let operations =
        LocalOperations::unconfined(dir.path(), vec![SHELL.to_owned()]).expect("a workspace");
    let started = Instant::now();
    let answer = operations
        .run_within(
            &sh("sleep 3 > /dev/null & printf '%s' $! > d.pid; exit 0"),
            Some(Duration::from_millis(300)),
        )
        .expect("runs");
    let held = started.elapsed();
    let (pid, alive) = reap(dir.path(), "d.pid");
    assert_eq!(answer["timed_out"], json!(true), "{answer}");
    assert_eq!(answer["exit"], json!(0), "{answer}");
    assert!(
        held < Duration::from_millis(1_500),
        "held {held:?}: {answer}"
    );
    assert!(!alive, "{pid} outlived the call: {answer}");
}

#[test]
fn a_typed_rust_call_through_local_operations_answers_as_0_15_1_did() {
    let dir = tempfile::tempdir().expect("a workspace");
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"adversary\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .expect("a manifest");
    std::fs::create_dir(dir.path().join("src")).expect("src");
    std::fs::write(dir.path().join("src/lib.rs"), "pub fn formatted() {}\n").expect("a source");
    let providers = match tool_toolchain::Registry::builtins()
        .and_then(|registry| registry.resolve(dir.path(), Some(&["rust".to_owned()])))
    {
        Ok(providers) => providers,
        Err(why) => {
            eprintln!("skipped: the built-in Rust provider does not resolve here: {why}");
            return;
        }
    };
    let programs: Vec<String> = providers
        .iter()
        .flat_map(|provider| provider.programs.iter().cloned())
        .collect();
    let operations =
        LocalOperations::unconfined(dir.path(), programs.clone()).expect("a workspace");
    let root = operations.root().to_path_buf();
    let catalogue = Catalogue::of_with_toolchains(operations, providers);
    let argv = owned(&["cargo", "fmt", "--all", "--", "--check"]);

    let now = catalogue
        .invoke("rust_fmt_check", &json!({}))
        .expect("a formatted crate passes");
    let then = v0_15_1::exec(&root, &[], &argv, &programs, None).expect("0.15.1 runs it");
    assert_eq!(now.to_string(), then.to_string());

    std::fs::write(root.join("src/lib.rs"), "pub fn   unformatted( ) {}\n").expect("a source");
    let now = catalogue.invoke("rust_fmt_check", &json!({}));
    let then = v0_15_1::exec(&root, &[], &argv, &programs, None).expect("0.15.1 runs it");
    match now {
        Ok(now) => assert_eq!(now.to_string(), then.to_string()),
        Err(refused) => assert_eq!(
            refused.message(),
            format!("formatting check failed: {}", then["stdout"])
        ),
    }
}

/// The pid a script wrote to `file`, and whether it was alive when asked; a live one is killed.
fn reap(root: &Path, file: &str) -> (i32, bool) {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let text = std::fs::read_to_string(root.join(file)).expect("the script wrote the pid");
    let pid: i32 = text.trim().parse().expect("a pid");
    let alive = kill(Pid::from_raw(pid), None).is_ok();
    if alive {
        let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
    }
    (pid, alive)
}

// --- a group id freed before the bound -----------------------------------------------------------

const IN_NAMESPACE: &str = "B10X_ADVERSARY_IN_PID_NAMESPACE";
const UNSHARE: [&str; 6] = [
    "--user",
    "--map-root-user",
    "--pid",
    "--fork",
    "--mount-proc",
    "--",
];

/// Whether this process is the one to run `case`'s body: `true` inside the namespace; outside it,
/// `case` is run again as PID 1 of a new user and pid namespace — where the next pid can be chosen
/// by writing `ns_last_pid`, so an id the runner freed can be handed to another group on purpose —
/// its verdict asserted here, and `false` answered.
fn inside_a_pid_namespace(case: &str) -> bool {
    if std::env::var_os(IN_NAMESPACE).is_some() {
        return true;
    }
    let probe = Command::new("unshare").args(UNSHARE).arg("true").output();
    if !probe.is_ok_and(|probe| probe.status.success()) {
        eprintln!("skipped: no unprivileged user and pid namespace on this machine");
        return false;
    }
    let output = Command::new("unshare")
        .args(UNSHARE)
        .arg(std::env::current_exe().expect("this test binary"))
        .args(["--exact", case, "--nocapture", "--test-threads=1"])
        .env(IN_NAMESPACE, "1")
        .output()
        .expect("the case runs in the namespace");
    assert!(
        output.status.success(),
        "{case} in a pid namespace: {}\n{}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    false
}

/// The program records its pid, leaves `sleep` holding stdout in a session of its own and ends at
/// once: its process group is then empty and its id free. A decoy is started in a group of its own
/// under that freed id, before the bound. Answers the run's reply and the decoy's status after
/// it: `None` while the decoy is alive.
fn a_freed_group_id_taken_by_a_decoy(
    run: impl Fn(&LocalOperations, &[String], Duration) -> Value,
) -> (Value, Option<std::process::ExitStatus>) {
    use std::os::unix::process::CommandExt as _;

    use nix::errno::Errno;
    use nix::sys::signal::kill;
    use nix::unistd::Pid;

    let dir = tempfile::tempdir().expect("a workspace");
    let operations =
        LocalOperations::unconfined(dir.path(), vec![SHELL.to_owned()]).expect("a workspace");
    let root = operations.root().to_path_buf();
    let decoy = std::thread::spawn(move || {
        let program = loop {
            if let Some(pid) = std::fs::read_to_string(root.join("program.pid"))
                .ok()
                .and_then(|text| text.trim().parse::<i32>().ok())
            {
                break pid;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        // Reaped by the runner, not only ended: only then is the id free to be handed out.
        while kill(Pid::from_raw(program), None) != Err(Errno::ESRCH) {
            std::thread::sleep(Duration::from_millis(1));
        }
        std::fs::write("/proc/sys/kernel/ns_last_pid", (program - 1).to_string())
            .expect("construction: the next pid can be chosen");
        let decoy = Command::new("/bin/sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .expect("construction: the decoy starts");
        assert_eq!(
            i32::try_from(decoy.id()).ok(),
            Some(program),
            "construction: the decoy took the program's freed id"
        );
        decoy
    });
    let answer = run(
        &operations,
        &sh("printf '%s' $$ > program.pid; setsid sleep 3 & exit 0"),
        Duration::from_millis(1_500),
    );
    let mut decoy = decoy.join().expect("the decoy was placed");
    let status = decoy.try_wait().expect("the decoy can be asked");
    let _ = decoy.kill();
    let _ = decoy.wait();
    (answer, status)
}

#[test]
#[ignore = "b10x-tool-process kills the program's group id after reaping it; fix requested upstream (adversary-run-pass-1, finding 2)"]
fn the_kill_at_the_bound_does_not_reach_a_group_that_took_the_programs_freed_id() {
    if !inside_a_pid_namespace(
        "the_kill_at_the_bound_does_not_reach_a_group_that_took_the_programs_freed_id",
    ) {
        return;
    }
    let (answer, decoy) = a_freed_group_id_taken_by_a_decoy(|operations, argv, bound| {
        operations
            .run_within(argv, Some(bound))
            .expect("the declared program runs")
    });
    assert_eq!(
        answer["timed_out"],
        json!(true),
        "precondition: the run reached its bound: {answer}"
    );
    assert!(
        decoy.is_none(),
        "the kill at the bound reached a process group that was not the run's: the program had \
         ended and been reaped, its id was free, and another group took it; the decoy ended with \
         {decoy:?}. Reply: {answer}"
    );
}

#[test]
fn under_0_15_1_the_same_construction_leaves_the_other_group_alive() {
    if !inside_a_pid_namespace("under_0_15_1_the_same_construction_leaves_the_other_group_alive") {
        return;
    }
    let (answer, decoy) = a_freed_group_id_taken_by_a_decoy(|operations, argv, bound| {
        v0_15_1::exec(
            operations.root(),
            &[],
            argv,
            operations.programs(),
            Some(bound),
        )
        .expect("the declared program runs")
    });
    // 0.15.1's own bug, the one the story fixes: it waited for the escaped `sleep` past its bound.
    assert_eq!(answer["timed_out"], json!(false), "{answer}");
    assert!(
        decoy.is_none(),
        "0.15.1 killed the decoy: {decoy:?}. Reply: {answer}"
    );
}

/// Harness 0.15.1's runner: `exec` and its helpers from `crates/harness-catalogue/src/local.rs` at
/// `df37462b`, with `self.root` and `self.inherited` made arguments and nothing else changed.
mod v0_15_1 {
    use std::fmt::Write as _;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use harness_catalogue::{Refusal, Refused};
    use serde_json::{Value, json};

    const MAX_RUN_OUTPUT_BYTES: usize = 64 * 1024;
    const MAX_RUN_SECONDS: u64 = 600;
    const INHERITED_ENV: &[&str] = &[
        "PATH",
        "HOME",
        "LANG",
        "LC_ALL",
        "TERM",
        "TMPDIR",
        "CARGO_HOME",
        "RUSTUP_HOME",
        "RUSTUP_TOOLCHAIN",
        "CARGO_TARGET_DIR",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ];

    pub(super) fn exec(
        root: &Path,
        inherited: &[String],
        argv: &[String],
        programs: &[String],
        remaining: Option<Duration>,
    ) -> Result<Value, Refused> {
        let Some(program) = argv.first() else {
            return Err("`argv` must name a program".into());
        };
        if !programs.iter().any(|allowed| allowed == program) {
            return Err(Refusal::ProgramNotDeclared {
                program: program.clone(),
                declared: programs.to_vec(),
            }
            .into());
        }

        let mut command = std::process::Command::new(program);
        command
            .args(&argv[1..])
            .current_dir(root)
            .env_clear()
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        {
            use std::os::unix::process::CommandExt as _;
            command.process_group(0);
        }
        for name in INHERITED_ENV
            .iter()
            .copied()
            .chain(inherited.iter().map(String::as_str))
        {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        let mut child = command
            .spawn()
            .map_err(|error| Refused::from(format!("`{program}`: {error}")))?;

        let out = child.stdout.take().expect("stdout was piped");
        let err = child.stderr.take().expect("stderr was piped");
        let out = std::thread::spawn(move || drain(out));
        let err = std::thread::spawn(move || drain(err));

        let ceiling = Duration::from_secs(MAX_RUN_SECONDS);
        let bound = remaining.map_or(ceiling, |left| left.min(ceiling));
        let deadline = Instant::now() + bound;
        let mut killed = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(error) => return Err(format!("`{program}`: {error}").into()),
            }
            if Instant::now() >= deadline {
                killed = true;
                terminate(&mut child);
                break child
                    .wait()
                    .map_err(|error| Refused::from(format!("`{program}`: {error}")))?;
            }
            std::thread::sleep(Duration::from_millis(25));
        };

        let empty = || Drained {
            text: String::new(),
            omitted: 0,
        };
        let stdout = out.join().unwrap_or_else(|_| empty());
        let stderr = err.join().unwrap_or_else(|_| empty());
        let omitted = stdout.omitted + stderr.omitted;
        Ok(json!({
            "argv": argv,
            "exit": status.code(),
            "stdout": stdout.text,
            "stderr": stderr.text,
            "truncated": omitted > 0,
            "omitted_bytes": omitted,
            "timed_out": killed,
            "timeout_ms": u64::try_from(bound.as_millis()).unwrap_or(u64::MAX),
        }))
    }

    fn terminate(child: &mut std::process::Child) {
        use nix::sys::signal::{Signal, killpg};
        use nix::unistd::Pid;

        if let Ok(pid) = i32::try_from(child.id()) {
            let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
        }
        let _ = child.kill();
    }

    struct Drained {
        text: String,
        omitted: u64,
    }

    fn drain(stream: impl std::io::Read) -> Drained {
        let half = MAX_RUN_OUTPUT_BYTES / 2;
        let (head, tail, beyond_head) = read_ends(stream, half);
        keep_both_ends(&head, &tail, head.len() as u64 + beyond_head)
    }

    fn read_ends(mut stream: impl std::io::Read, half: usize) -> (Vec<u8>, Vec<u8>, u64) {
        let mut head: Vec<u8> = Vec::new();
        let mut tail: std::collections::VecDeque<u8> = std::collections::VecDeque::new();
        let mut beyond_head: u64 = 0;
        let mut buffer = [0_u8; 8192];
        while let Ok(read) = stream.read(&mut buffer) {
            if read == 0 {
                break;
            }
            let mut bytes = &buffer[..read];
            let room = half.saturating_sub(head.len());
            if room > 0 {
                let taken = room.min(bytes.len());
                head.extend_from_slice(&bytes[..taken]);
                bytes = &bytes[taken..];
            }
            beyond_head += bytes.len() as u64;
            tail.extend(bytes.iter().copied());
            if tail.len() > half {
                tail.drain(..tail.len() - half);
            }
        }
        (head, tail.into_iter().collect(), beyond_head)
    }

    fn keep_both_ends(head: &[u8], tail: &[u8], total: u64) -> Drained {
        if head.len() as u64 + tail.len() as u64 >= total {
            let mut whole = head.to_vec();
            whole.extend_from_slice(tail);
            return Drained {
                text: String::from_utf8_lossy(&whole).into_owned(),
                omitted: 0,
            };
        }
        let head = &head[..whole_prefix(head)];
        let tail = &tail[whole_suffix(tail)..];
        let omitted = total - head.len() as u64 - tail.len() as u64;
        let mut text = String::from_utf8_lossy(head).into_owned();
        let _ = write!(text, "\n… {omitted} bytes omitted here …\n");
        text.push_str(&String::from_utf8_lossy(tail));
        Drained { text, omitted }
    }

    fn whole_prefix(bytes: &[u8]) -> usize {
        match std::str::from_utf8(bytes) {
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            Ok(_) | Err(_) => bytes.len(),
        }
    }

    fn whole_suffix(bytes: &[u8]) -> usize {
        bytes
            .iter()
            .position(|byte| byte & 0b1100_0000 != 0b1000_0000)
            .unwrap_or(bytes.len())
    }
}
