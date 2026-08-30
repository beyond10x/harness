//! The workspace-name rule the help pages state, against the one the binary keeps.
//!
//! `story:help-text-names-a-rule-the-code-dropped` accepts when "`b10x-harness run --help` and
//! `chat --help` describe the workspace-name rule the code **actually enforces**". The pages no
//! longer say `ws_something`. What they say instead is "one path component of alphanumerics, `_`
//! and `-`", which is the sentence the refusal uses and is the sentence
//! `crates/harness-cli/src/lib.rs:2661-2669` and substrate's `validate_root_name` were written
//! from — and both of those check `is_ascii_alphanumeric`.
//!
//! So the pages describe a rule strictly wider than the enforced one, and the names in the gap are
//! not exotic: `café`, `Projekt-Übung`, `日本語`. An operator whose directory is one of them reads
//! a refusal that names a rule their directory satisfies, and has nothing to act on.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

struct Output {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

fn invoke(arguments: &[&str]) -> Output {
    let output = Command::new(BINARY)
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("the binary runs");
    Output {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// A directory of the given name, inside a root that can be substrate's.
fn adoptable_workspace(name: &str) -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().expect("a temporary directory");
    let workspace = root.path().join(name);
    fs::create_dir(&workspace).expect("create the workspace directory");
    (root, workspace)
}

/// clap wraps long help to the terminal's width, so the phrase is looked for in flowed text.
fn flowed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The pages say "alphanumerics" and the binary means ASCII alphanumerics.
///
/// **Measured first, in the order the unit's own case measures it.** `café` is a directory name
/// made of nothing but letters under every reading of the word "alphanumerics" a person has, and
/// this binary refuses to adopt it — the refusal is asserted here, from the shipped binary, before
/// any page is read. Then the pages are asked whether they say so.
///
/// They do not. `run --help` and `chat --help` state the rule as "one path component of
/// alphanumerics, `_` and `-`, and may not be `.`, `..` or begin with `-`", with no mention of
/// ASCII, and the refusal repeats it word for word. So the operator who is refused is handed a
/// rule their directory already satisfies and cannot tell what to rename it to — which is the
/// failure this story was opened about, moved rather than closed: the old sentence told them to
/// rename a directory that did not need renaming, the new one refuses a rename it will not accept
/// either.
///
/// This is not a Unicode edge case reached by a fuzzer. `café`, `Projekt-Übung`, `Ordner`,
/// `mañana`, `日本語` are ordinary directory names on ordinary machines, and `--substrate-embedded`
/// is the flag whose whole purpose after `0c31438` is to be pointed at a project the operator
/// already has, under the name it already has.
#[test]
fn the_help_pages_say_which_alphanumerics_the_binary_means() {
    let mut refused: Vec<String> = Vec::new();
    for name in ["café", "Projekt-Übung", "日本語"] {
        let (_root, workspace) = adoptable_workspace(name);
        let output = invoke(&[
            "tools",
            "--substrate-embedded",
            "--workspace",
            &workspace.display().to_string(),
        ]);
        if output.status == Some(0) {
            continue;
        }
        assert!(
            output.stderr.contains("alphanumerics"),
            "`{name}` was refused for some other reason than its name: {}",
            output.stderr
        );
        refused.push(name.to_owned());
    }
    assert!(
        !refused.is_empty(),
        "no non-ASCII workspace name was refused, so this case is asserting nothing about the \
         pages; the rule changed and this case has to be re-derived from it"
    );

    for command in ["run", "chat"] {
        let help = invoke(&[command, "--help"]);
        assert_eq!(help.status, Some(0), "`{command} --help`: {}", help.stderr);
        let page = flowed(&help.stdout);
        assert!(
            page.contains("alphanumerics"),
            "`{command} --help` states the workspace-name rule in some other words than the \
             refusal's, and this case has to be re-derived from them:\n{}",
            help.stdout
        );
        assert!(
            page.contains("ASCII") || page.contains("ascii"),
            "`{command} --help` states the rule as `alphanumerics` and this binary means ASCII \
             alphanumerics: it refuses {refused:?}. An operator whose directory is one of those \
             reads a rule their name satisfies, in the page and again in the refusal, and has \
             nothing to rename it to.\n{}",
            help.stdout
        );
    }
}

/// The two other pages the same sentence is rendered on are not held to it.
///
/// `RunOptions` (`crates/harness-cli/src/lib.rs:473`) is flattened by `run`, `chat` **and
/// `workflow run`**; `ToolsOptions` (`:907`) carries a second copy of the same paragraph for
/// `tools`. Four pages, two sources. The unit's own case
/// (`crates/harness-cli/tests/end_to_end.rs:487`) asserts on `run` and `chat`, which are the same
/// source — so the whole `ToolsOptions` copy is unpinned, and reverting it alone puts `ws_something`
/// back on `tools --help` with the suite still green.
///
/// This case is the one that would catch that mutant. It is green at `1acad51`: both copies were
/// corrected. It is here so that the *next* edit to either one is caught on all four pages rather
/// than on the two that happen to share a struct.
#[test]
fn every_page_the_workspace_rule_is_rendered_on_states_the_same_rule() {
    let pages: [&[&str]; 4] = [
        &["run", "--help"],
        &["chat", "--help"],
        &["tools", "--help"],
        &["workflow", "run", "--help"],
    ];
    for page in pages {
        let help = invoke(page);
        let named = page.join(" ");
        assert_eq!(help.status, Some(0), "`{named}`: {}", help.stderr);
        let text = flowed(&help.stdout);
        assert!(
            text.contains("--substrate-embedded"),
            "`{named}` renders `--substrate-embedded`, or this case is looking at the wrong page"
        );
        assert!(
            !text.contains("ws_"),
            "`{named}` still requires a `ws_` workspace name, which this binary dropped at \
             substrate 0.2.2:\n{}",
            help.stdout
        );
        assert!(
            text.contains("one path component"),
            "`{named}` has to state the rule that is enforced, in the words the refusal uses:\n{}",
            help.stdout
        );
    }
}
