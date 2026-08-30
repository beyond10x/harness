//! One suite, asked of every implementation of `harness_tools::Operations`.
//!
//! # What an embedder is promised, and what was checking it
//!
//! Three types perform a catalogue entry: `LocalOperations` on the process's own filesystem,
//! `ConfinedOperations` through substrate's guarded IO, and `Split`, which reads through one and
//! effects through the other — the composition `harness-cli` builds. The promise the trait makes
//! is that handing a run a remote workspace gives it *the same answers for the same reasons* as
//! handing it a local one. Each had its own tests and nothing asked them the same question, so the
//! promise was a comment.
//!
//! This is here rather than in `harness-tools` because this is the only crate that can see all
//! three: `harness-substrate` depends on `harness-tools`, and a dependency the other way would
//! invert the layering the boundary rule exists to keep.
//!
//! # What is asserted, and what is deliberately not
//!
//! Only what the trait's own documentation states. Where the trait leaves a decision to the
//! implementation, this suite is silent, because pinning one implementation's answer there would
//! freeze a difference the trait allows:
//!
//! | left to the implementation | why it is not asserted here |
//! |---|---|
//! | `lands` on a symlink | the trait's default answers the path as written, and says so: substrate resolves with `RESOLVE_NO_SYMLINKS` and has no link to follow, the local provider does |
//! | `lands` outside the workspace | the trait says "the catalogue does not act on the error"; the *write* refuses either way, and that is what is asserted |
//! | the shape of a `run` result | the trait says only that it is a `Value`; the local reply and substrate's exec observation carry different fields |
//! | how much of a file one read may reach | both cap, at figures each names; what is shared is that a read which did not reach the end **says so** |
//!
//! # The suite is self-tested, for the reason the gate's home-path check is
//!
//! A conformance suite that could not fail would look exactly like three implementations that
//! agree. So [`Divergent`] is a fourth workspace that breaks one rule on purpose — it answers
//! every read with whatever it managed to read and calls it complete — and two tests assert that
//! the runners below **name it**. Those two are the evidence that a green run of the rest means
//! something.
//!
//! # Which backend the confined workspace holds
//!
//! `Embedded`: substrate's own driver, in this process, over a directory this test makes. It needs
//! no daemon, no socket and no credential, so every case below runs on every machine. Execution is
//! the one thing a host without a delegated cgroup subtree cannot confine, and that is **asserted
//! rather than skipped** — see
//! `execution_is_offered_by_exactly_the_workspaces_this_machine_lets_confine_one`.
//!
//! **No branch in this file reports a pass without asserting something.** That is the property, and
//! it is not the same as having no `return`: where a workspace cannot offer an operation on this
//! machine, the branch asserts that it does not offer it. The sentence here used to claim there was
//! no early return anywhere, and there was one — `the_declared_set_is_what_run_accepts` returned
//! `Ok(())` for every workspace with an empty `programs()`, which on a host with no delegated
//! cgroup subtree is two of the three. That is the `embedded_live.rs` shape this file exists not to
//! repeat, and it was in it.
//!
//! The claim reaches the runners as well, and it had a second hole there. `asked` handed an empty
//! slice answered `Ok`, which `every_workspace` reads as *every workspace met the contract*, and
//! `agreed` handed one workspace compared nothing and answered the one thing it had. Both now
//! refuse, and `the_runner_refuses_to_report_a_pass_when_it_was_handed_no_workspace` and
//! `the_comparison_refuses_to_report_agreement_between_one_workspace_and_itself` are what say so.

use std::path::{Path, PathBuf};
use std::time::Duration;

use b10x_harness_substrate::{Backend, ConfinedOperations, Embedded, Facts};
use harness_tools::{
    LocalOperations, Operations, ReadWindow, Refusal, Refused, SearchOptions, Split,
};
use serde_json::{Value, json};
use tempfile::TempDir;

/// The program a workspace that offers execution is asked to declare.
const DECLARED: &[&str] = &["/bin/echo"];

/// A program no workspace here declares, whatever the machine admits.
const UNDECLARED: &str = "b10x-conformance-no-such-program";

// --- the three workspaces, over three identical trees -------------------------------------------

/// One implementation of the trait, over a tree this suite can seed and read back out of band.
struct Workspace {
    /// What a failure names. The whole point of the suite: a difference is attributable.
    name: &'static str,
    /// The directory the operations act on, as an ordinary reader sees it.
    tree: PathBuf,
    operations: Box<dyn Operations>,
    /// Every tree lives under a temporary root that must outlive the operations over it.
    _root: TempDir,
}

impl Workspace {
    /// Put a file in the tree without going through the operations under test.
    fn seed(&self, relative: &str, text: &str) {
        let target = self.tree.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).expect("the seed's directory");
        }
        std::fs::write(target, text).expect("the seed is written");
    }

    /// What an ordinary reader finds in the tree, or `None` where there is no such file.
    fn on_disk(&self, relative: &str) -> Option<String> {
        std::fs::read_to_string(self.tree.join(relative)).ok()
    }
}

/// A temporary root and the tree under it that every workspace here is opened on.
///
/// Two levels, because substrate adopts a directory *beneath* the root it is opened on, and the
/// reading provider is pointed at that same directory — which is what `harness-cli` does, so that
/// a run reads the tree it writes into rather than one beside it.
fn tree() -> (TempDir, PathBuf) {
    let root = tempfile::tempdir().expect("a temporary root");
    let tree = root.path().join("work");
    std::fs::create_dir(&tree).expect("the tree");
    (root, tree)
}

/// The delegated cgroup subtree this machine was pointed at, if it was pointed at one.
fn cgroup_root() -> Option<PathBuf> {
    std::env::var_os("B10X_CGROUP_ROOT").map(PathBuf::from)
}

/// The confined provider over an embedded driver that has adopted `root/work`.
fn confined(root: &Path, facts: &dyn Fn(&Facts) -> Facts, programs: &[&str]) -> ConfinedOperations {
    let driver = Embedded::open(root, cgroup_root()).expect("the embedded driver opens");
    let machine = driver.machine().expect("the driver says what it can do");
    let workspace = driver.workspace_adopt("work").expect("the tree is adopted");
    ConfinedOperations::new(
        driver,
        &facts(&machine),
        workspace,
        programs.iter().map(|p| (*p).to_owned()).collect(),
    )
}

/// The machine as it is.
fn as_probed(facts: &Facts) -> Facts {
    facts.clone()
}

/// A machine that admits nothing — what a caller holds when there is nothing to ask.
fn admits_nothing(_: &Facts) -> Facts {
    Facts::none()
}

/// The three implementations, each opened on its own tree with `programs` declared.
fn workspaces(programs: &[&str]) -> Vec<Workspace> {
    let (local_root, local_tree) = tree();
    let (confined_root, confined_tree) = tree();
    let (split_root, split_tree) = tree();
    vec![
        Workspace {
            name: "LocalOperations",
            operations: Box::new(
                LocalOperations::unconfined(
                    &local_tree,
                    programs.iter().map(|p| (*p).to_owned()).collect(),
                )
                .expect("the workspace opens"),
            ),
            tree: local_tree,
            _root: local_root,
        },
        Workspace {
            name: "ConfinedOperations",
            operations: Box::new(confined(confined_root.path(), &as_probed, programs)),
            tree: confined_tree,
            _root: confined_root,
        },
        Workspace {
            name: "Split",
            // Exactly what `harness-cli` composes: the local provider reads, the confined one
            // writes and executes, over one tree.
            operations: Box::new(Split::new(
                LocalOperations::new(&split_tree).expect("the reading half opens"),
                confined(split_root.path(), &as_probed, programs),
            )),
            tree: split_tree,
            _root: split_root,
        },
    ]
}

/// The same three, each answering `writes() == false` — an embedder that asked for no effects.
fn read_only_workspaces() -> Vec<Workspace> {
    let (local_root, local_tree) = tree();
    let (confined_root, confined_tree) = tree();
    let (split_root, split_tree) = tree();
    vec![
        Workspace {
            name: "LocalOperations",
            operations: Box::new(LocalOperations::new(&local_tree).expect("the workspace opens")),
            tree: local_tree,
            _root: local_root,
        },
        Workspace {
            name: "ConfinedOperations",
            operations: Box::new(confined(confined_root.path(), &admits_nothing, &[])),
            tree: confined_tree,
            _root: confined_root,
        },
        Workspace {
            name: "Split",
            operations: Box::new(Split::new(
                LocalOperations::new(&split_tree).expect("the reading half opens"),
                confined(split_root.path(), &admits_nothing, &[]),
            )),
            tree: split_tree,
            _root: split_root,
        },
    ]
}

// --- the two runners, and what they say when a workspace differs --------------------------------

/// Ask one question of every workspace and name the ones that answer against the contract.
///
/// **An empty set is a failure, not a pass.** `every_workspace` reads an `Ok` here as *every
/// workspace met the contract*, and handed nothing this used to say that having asked nobody
/// anything. Not reachable from the two callers, which build three workspaces by hand — which was
/// equally true of the early return in `the_declared_set_is_what_run_accepts` until a machine with
/// no delegated cgroup subtree reached it.
fn asked(
    workspaces: &[Workspace],
    check: fn(&Workspace) -> Result<(), String>,
) -> Result<(), String> {
    if workspaces.is_empty() {
        return Err("  no workspace was asked, so nothing was checked".to_owned());
    }
    let failures: Vec<String> = workspaces
        .iter()
        .filter_map(|workspace| {
            check(workspace)
                .err()
                .map(|why| format!("  {}: {why}", workspace.name))
        })
        .collect();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

/// Every workspace meets the contract, or the ones that do not are named.
fn every_workspace(behaviour: &str, check: fn(&Workspace) -> Result<(), String>) {
    if let Err(named) = asked(&workspaces(DECLARED), check) {
        panic!("{behaviour}\n{named}");
    }
}

/// The same, over workspaces that say they change nothing.
fn every_read_only_workspace(behaviour: &str, check: fn(&Workspace) -> Result<(), String>) {
    if let Err(named) = asked(&read_only_workspaces(), check) {
        panic!("{behaviour}\n{named}");
    }
}

/// Every workspace gives the same answer, or the two that differ are named with both answers.
///
/// Answers the one thing they all said, so the caller can see it was a real answer.
///
/// **Two is the least that can disagree.** Handed one workspace the comparison loop never runs and
/// this would answer `Ok` having compared nothing, which is the same hole as [`asked`] on an empty
/// slice and the same one the module header claims this file does not have.
fn agreed(workspaces: &[Workspace], ask: fn(&Workspace) -> Value) -> Result<Value, String> {
    let Some((head, rest)) = workspaces
        .split_first()
        .filter(|(_, rest)| !rest.is_empty())
    else {
        return Err(format!(
            "{} workspace(s) were asked, so nothing was compared",
            workspaces.len()
        ));
    };
    let first = head.name;
    let expected = ask(head);
    for workspace in rest {
        let answer = ask(workspace);
        if answer != expected {
            return Err(format!(
                "`{first}` and `{}` answer differently:\n  {first}: {expected}\n  {}: {answer}",
                workspace.name, workspace.name
            ));
        }
    }
    Ok(expected)
}

/// Every workspace answers the same, and answers rather than refuses.
///
/// The second half is not decoration. [`answered`] folds a refusal into the value so that one
/// workspace refusing where another answers is a visible disagreement — which means three
/// workspaces refusing *identically* would agree, and the comparison would pass having compared
/// three failures. Every question asked through here is one all three are expected to answer.
fn all_agree(behaviour: &str, ask: fn(&Workspace) -> Value) {
    match agreed(&workspaces(DECLARED), ask) {
        Err(named) => panic!("{behaviour}\n{named}"),
        Ok(shared) if shared.get("refused").is_some() => {
            panic!("{behaviour}\n  every workspace refused, so nothing was compared: {shared}");
        }
        Ok(_) => {}
    }
}

/// A refusal folded into a value, so a workspace that refuses where another answers is a visible
/// disagreement rather than a panic inside the comparison.
fn answered(result: Result<Value, String>) -> Value {
    result.unwrap_or_else(|why| json!({"refused": why}))
}

// --- reading one file: the window contract ------------------------------------------------------

const FIVE_LINES: &str = "alpha\nbeta\ngamma\ndelta\nepsilon\n";

#[test]
fn every_workspace_answers_the_same_window_of_the_same_file() {
    all_agree(
        "a window of a file is one answer, and the workspace a run holds must not change it:",
        |workspace| {
            workspace.seed("notes.txt", FIVE_LINES);
            answered(
                workspace
                    .operations
                    .file_read("notes.txt", ReadWindow::lines(2, 2)),
            )
        },
    );
}

#[test]
fn every_workspace_answers_the_same_whole_small_file() {
    all_agree(
        "a file that fits under every ceiling is answered whole, the same way, by all of them:",
        |workspace| {
            workspace.seed("sub/notes.txt", FIVE_LINES);
            answered(
                workspace
                    .operations
                    .file_read("sub/notes.txt", ReadWindow::whole()),
            )
        },
    );
}

#[test]
fn every_workspace_answers_an_empty_file_as_the_file_and_not_as_an_absence() {
    all_agree(
        "an empty file read from line 1 is the file, answered whole:",
        |workspace| {
            workspace.seed("empty.txt", "");
            answered(
                workspace
                    .operations
                    .file_read("empty.txt", ReadWindow::whole()),
            )
        },
    );
}

/// A window that stopped before the last line says so, and says where it stopped.
///
/// Invariant 8 in the shape the trait states it: `lines.total` is what keeps a window from being
/// mistaken for a whole file. Asserted for its meaning and not only for agreement, because three
/// implementations that were equally wrong would agree.
fn a_window_that_stops_short_says_so(workspace: &Workspace) -> Result<(), String> {
    workspace.seed("notes.txt", FIVE_LINES);
    let answer = workspace
        .operations
        .file_read("notes.txt", ReadWindow::lines(1, 2))
        .map_err(|why| format!("refused a window inside the file: {why}"))?;
    if answer["truncated"] != json!(true) {
        return Err(format!(
            "read 2 of 5 lines and did not say the answer was cut: {answer}"
        ));
    }
    if answer["lines"]["to"] != json!(2) || answer["lines"]["total"] != json!(5) {
        return Err(format!(
            "read 2 of 5 lines and did not say which of how many: {answer}"
        ));
    }
    Ok(())
}

#[test]
fn a_read_that_did_not_reach_the_end_says_so_in_every_workspace() {
    every_workspace(
        "a window mistaken for a whole file is what `lines.total` exists to stop:",
        a_window_that_stops_short_says_so,
    );
}

/// `offset: 0` names no line, and is refused rather than read as line 1.
fn line_zero_is_refused(workspace: &Workspace) -> Result<(), String> {
    workspace.seed("notes.txt", FIVE_LINES);
    let window = ReadWindow {
        offset: Some(0),
        ..ReadWindow::whole()
    };
    match workspace.operations.file_read("notes.txt", window) {
        Ok(answer) => Err(format!("answered a window starting at line 0: {answer}")),
        Err(why) if why.contains("lines are numbered from 1") => Ok(()),
        Err(why) => Err(format!("refused line 0 without saying why: {why}")),
    }
}

#[test]
fn line_zero_is_refused_by_every_workspace_in_the_same_words() {
    every_workspace(
        "lines are numbered from 1, so 0 names no line and no workspace may guess at one:",
        line_zero_is_refused,
    );
}

/// A window past the end of the file is refused, never answered empty.
///
/// The trait's own `# Errors`: "the window names lines the file does not have". An empty answer
/// would read to the model as *the file has nothing there*, which is a different fact.
fn a_window_past_the_end_is_refused(workspace: &Workspace) -> Result<(), String> {
    workspace.seed("notes.txt", FIVE_LINES);
    match workspace
        .operations
        .file_read("notes.txt", ReadWindow::lines(9, 2))
    {
        Ok(answer) => Err(format!(
            "answered a window past the end of a 5-line file instead of refusing it: {answer}"
        )),
        Err(why) if why.contains("line 9") && why.contains("notes.txt") => Ok(()),
        Err(why) => Err(format!(
            "refused without naming the file and the line asked for: {why}"
        )),
    }
}

#[test]
fn a_window_past_the_end_of_a_file_is_refused_by_every_workspace() {
    every_workspace(
        "a window the file does not have is a refusal; an empty answer would read as an empty \
         file:",
        a_window_past_the_end_is_refused,
    );
}

/// A directory is not a file, and reading one is refused.
fn a_directory_is_not_a_file(workspace: &Workspace) -> Result<(), String> {
    workspace.seed("sub/notes.txt", FIVE_LINES);
    match workspace.operations.file_read("sub", ReadWindow::whole()) {
        Ok(answer) => Err(format!("read a directory as a file: {answer}")),
        Err(why) if why.is_empty() => Err("refused a directory with an empty sentence".to_owned()),
        Err(_) => Ok(()),
    }
}

#[test]
fn a_directory_is_not_a_file_in_any_workspace() {
    every_workspace(
        "`file_read` answers a file; a directory is refused in words:",
        a_directory_is_not_a_file,
    );
}

/// A path that leaves the workspace is refused, whichever door the bytes would have gone through.
fn a_path_outside_the_workspace_is_refused(workspace: &Workspace) -> Result<(), String> {
    let outside = workspace.tree.parent().expect("the tree has a parent");
    std::fs::write(outside.join("outside.txt"), "not yours\n").expect("a file outside the tree");
    match workspace
        .operations
        .file_read("../outside.txt", ReadWindow::whole())
    {
        Ok(answer) => Err(format!("read a file outside the workspace: {answer}")),
        Err(why) if why.is_empty() => Err("refused with an empty sentence".to_owned()),
        Err(_) => Ok(()),
    }
}

#[test]
fn a_path_outside_the_workspace_is_refused_by_every_workspace() {
    every_workspace(
        "the workspace is the boundary, and every implementation of it refuses the same escape:",
        a_path_outside_the_workspace_is_refused,
    );
}

// --- changing one file --------------------------------------------------------------------------

const TWICE: &str = "alpha\nbeta\nalpha\n";

/// An edit whose text is nowhere changes nothing and says so.
fn an_edit_that_names_no_place_changes_nothing(workspace: &Workspace) -> Result<(), String> {
    workspace.seed("notes.txt", FIVE_LINES);
    let outcome = workspace
        .operations
        .file_edit("notes.txt", "not in the file", "replacement");
    if let Ok(answer) = outcome {
        return Err(format!("reported an edit that matched nothing: {answer}"));
    }
    if workspace.on_disk("notes.txt").as_deref() != Some(FIVE_LINES) {
        return Err("refused the edit and changed the file anyway".to_owned());
    }
    Ok(())
}

#[test]
fn an_edit_that_names_no_place_changes_nothing_in_any_workspace() {
    every_workspace(
        "an edit that matched nothing must not report success: the model would believe it landed:",
        an_edit_that_names_no_place_changes_nothing,
    );
}

/// An edit whose text appears twice changes nothing and says how many places it found.
fn an_edit_that_names_several_places_changes_nothing(workspace: &Workspace) -> Result<(), String> {
    workspace.seed("notes.txt", TWICE);
    match workspace
        .operations
        .file_edit("notes.txt", "alpha", "omega")
    {
        Ok(answer) => return Err(format!("edited one of two matching places: {answer}")),
        Err(why) if why.contains("2 times") => {}
        Err(why) => return Err(format!("refused without saying how many places: {why}")),
    }
    if workspace.on_disk("notes.txt").as_deref() != Some(TWICE) {
        return Err("refused the edit and changed the file anyway".to_owned());
    }
    Ok(())
}

#[test]
fn an_edit_that_names_several_places_changes_nothing_in_any_workspace() {
    every_workspace(
        "an edit must name one place; several is a refusal that says how many:",
        an_edit_that_names_several_places_changes_nothing,
    );
}

/// An edit that names exactly one place lands there, and nowhere else.
fn an_edit_that_names_one_place_lands(workspace: &Workspace) -> Result<(), String> {
    workspace.seed("notes.txt", FIVE_LINES);
    workspace
        .operations
        .file_edit("notes.txt", "gamma", "GAMMA")
        .map_err(|why| format!("refused an edit that names one place: {why}"))?;
    match workspace.on_disk("notes.txt").as_deref() {
        Some("alpha\nbeta\nGAMMA\ndelta\nepsilon\n") => Ok(()),
        other => Err(format!("the edit did not land where it named: {other:?}")),
    }
}

#[test]
fn an_edit_that_names_one_place_lands_there_in_every_workspace() {
    every_workspace(
        "the one thing an edit is for, and every workspace does it to the same bytes:",
        an_edit_that_names_one_place_lands,
    );
}

/// A whole-file write puts exactly those bytes there, in every workspace.
fn a_write_puts_exactly_those_bytes_there(workspace: &Workspace) -> Result<(), String> {
    workspace
        .operations
        .file_write("written.txt", FIVE_LINES)
        .map_err(|why| format!("refused a write inside the workspace: {why}"))?;
    match workspace.on_disk("written.txt").as_deref() {
        Some(FIVE_LINES) => Ok(()),
        other => Err(format!("the write did not land: {other:?}")),
    }
}

#[test]
fn a_write_puts_exactly_those_bytes_there_in_every_workspace() {
    every_workspace(
        "what went in is what an ordinary reader of the tree finds:",
        a_write_puts_exactly_those_bytes_there,
    );
}

/// A workspace that answers `writes() == false` changes nothing, by any route.
///
/// `Operations::writes` is the one question the catalogue asks to decide which entries exist. An
/// embedder that goes around the catalogue — which is a supported thing to do, the trait is public
/// — must get the same answer from the provider itself, or `writes()` is a hint rather than a fact.
fn a_workspace_that_says_it_cannot_write_does_not(workspace: &Workspace) -> Result<(), String> {
    if workspace.operations.writes() {
        return Err("said it writes, when it was opened read-only".to_owned());
    }
    workspace.seed("notes.txt", FIVE_LINES);
    if let Ok(answer) = workspace.operations.file_write("written.txt", "anything") {
        return Err(format!(
            "wrote a file after answering `writes() == false`: {answer}"
        ));
    }
    if workspace.on_disk("written.txt").is_some() {
        return Err("`file_write` left a file behind in a read-only workspace".to_owned());
    }
    if let Ok(answer) = workspace
        .operations
        .file_edit("notes.txt", "gamma", "GAMMA")
    {
        return Err(format!(
            "edited a file after answering `writes() == false`: {answer}"
        ));
    }
    if workspace.on_disk("notes.txt").as_deref() != Some(FIVE_LINES) {
        return Err("`file_edit` changed a file in a read-only workspace".to_owned());
    }
    Ok(())
}

#[test]
fn a_workspace_that_says_it_cannot_write_does_not_in_any_implementation() {
    every_read_only_workspace(
        "`writes()` is what the catalogue publishes on; a provider that answers `false` and then \
         writes makes it a hint:",
        a_workspace_that_says_it_cannot_write_does_not,
    );
}

// --- where a write lands --------------------------------------------------------------------

/// A plain relative path lands under its own name, in every workspace.
///
/// Only the plain case: the trait's default answers the path as written and argues for it, so a
/// symlink or an escape is where the implementations are *allowed* to differ and this suite says
/// nothing about them.
///
/// **`sub/` is seeded first, and that is load-bearing.** Asked about `sub/new.txt` in a tree with
/// no `sub/`, all three agree on *where* the write would land for exactly the case where they
/// disagree on *whether* it lands at all — see
/// `a_write_under_a_directory_that_does_not_exist_yet_is_one_answer_in_every_workspace`, which
/// pins that split. Agreement about the address of a write that only one of them performs is
/// agreement about nothing.
fn a_plain_path_lands_under_its_own_name(workspace: &Workspace) -> Result<(), String> {
    workspace.seed("sub/already.txt", FIVE_LINES);
    match workspace.operations.lands("sub/new.txt") {
        Ok(landed) if landed == "sub/new.txt" => Ok(()),
        Ok(landed) => Err(format!("a new file under `sub/` lands at `{landed}`")),
        Err(why) => Err(format!("could not say where a plain path lands: {why}")),
    }
}

#[test]
fn a_plain_path_lands_under_its_own_name_in_every_workspace() {
    every_workspace(
        "the scope judges what `lands` answers, so a plain path must answer the same everywhere:",
        a_plain_path_lands_under_its_own_name,
    );
}

// --- what a workspace does not offer ------------------------------------------------------------

/// A reading operation either finds the one file in the tree or refuses in words.
///
/// The trait's rule for an operation an implementation does not perform: `Operations::unavailable`
/// is "a sentence rather than a silence: a tool that answered nothing would look to the model like
/// one that had worked". The confined provider offers none of these three and says so; the local
/// one and `Split`'s reading half answer them.
fn a_reading_operation_answers_or_refuses_in_words(workspace: &Workspace) -> Result<(), String> {
    workspace.seed("notes.txt", FIVE_LINES);
    let asked = [
        ("dir_list", workspace.operations.dir_list(".")),
        (
            "search",
            workspace
                .operations
                .search("beta", ".", &SearchOptions::default()),
        ),
        ("find", workspace.operations.find("*.txt", ".", None)),
    ];
    for (what, answer) in asked {
        match answer {
            Ok(value) if value.to_string().contains("notes.txt") => {}
            Ok(value) => {
                return Err(format!(
                    "`{what}` answered without naming the one file in the tree: {value}"
                ));
            }
            Err(why) if why.contains("is not offered by this workspace") => {}
            Err(why) => {
                return Err(format!(
                    "`{what}` refused without saying this workspace does not offer it: {why}"
                ));
            }
        }
    }
    Ok(())
}

#[test]
fn a_reading_operation_answers_or_refuses_in_words_in_every_workspace() {
    every_workspace(
        "an operation a workspace does not offer says so; an empty answer reads as an empty tree:",
        a_reading_operation_answers_or_refuses_in_words,
    );
}

// --- execution: the declared set ----------------------------------------------------------------

/// A program outside the declared set is refused **by name**, and the name carries the set.
///
/// The one refusal in this trait that is named rather than only worded, and the reason `run`
/// answers `Refused` where every other operation answers a `String`. `declared` must be what
/// `Operations::programs` says, or a reader of the record is told the run could have started
/// something it could not.
fn an_undeclared_program_is_refused_by_name(workspace: &Workspace) -> Result<(), String> {
    let argv = [UNDECLARED.to_owned()];
    let refused: Refused = match workspace.operations.run(&argv) {
        Ok(answer) => return Err(format!("started a program nobody declared: {answer}")),
        Err(refused) => refused,
    };
    match refused.refusal() {
        Some(Refusal::ProgramNotDeclared { program, declared }) => {
            if program != UNDECLARED {
                return Err(format!(
                    "refused `{program}`, which is not what was asked for"
                ));
            }
            if declared.as_slice() != workspace.operations.programs() {
                return Err(format!(
                    "the refusal names {declared:?} as the declared set and `programs()` says {:?}",
                    workspace.operations.programs()
                ));
            }
            Ok(())
        }
        None => Err(format!(
            "refused in words alone, so the record cannot count it: {refused}"
        )),
    }
}

#[test]
fn an_undeclared_program_is_refused_by_name_in_every_workspace() {
    every_workspace(
        "`run` is the one operation with a declared set, and falling outside it is a named \
         refusal, not a failure:",
        an_undeclared_program_is_refused_by_name,
    );
}

/// An empty argv names no program, and is refused without claiming one was declared away.
fn an_empty_argv_is_refused_without_naming_a_program(workspace: &Workspace) -> Result<(), String> {
    match workspace.operations.run(&[]) {
        Ok(answer) => Err(format!(
            "ran something for an argv with nothing in it: {answer}"
        )),
        Err(refused) if refused.refusal().is_some() => Err(format!(
            "blamed the declared set for an argv that names no program: {refused}"
        )),
        Err(refused) if refused.message().is_empty() => {
            Err("refused an empty argv with an empty sentence".to_owned())
        }
        Err(_) => Ok(()),
    }
}

#[test]
fn an_empty_argv_is_refused_without_naming_a_program_in_every_workspace() {
    every_workspace(
        "an argv with no program in it is refused, and not as a program the run may not start:",
        an_empty_argv_is_refused_without_naming_a_program,
    );
}

/// `run` accepts exactly the set `programs()` names — both halves of that, on every machine.
///
/// Where the set is empty the statement is *this workspace starts nothing*, and that half is
/// asserted here rather than left to `an_undeclared_program_is_refused_by_name`. The two overlap on
/// purpose: a branch that returned `Ok` because there was nothing to try would report a pass for a
/// workspace nothing had been asked of, which is exactly what this file claims not to do. On a host
/// with no delegated cgroup subtree that branch is the one two of the three workspaces take.
fn the_declared_set_is_what_run_accepts(workspace: &Workspace) -> Result<(), String> {
    let declared = workspace.operations.programs().to_vec();
    let Some(program) = declared.first() else {
        let argv = [DECLARED[0].to_owned()];
        return match workspace.operations.run(&argv) {
            Ok(answer) => Err(format!(
                "declared no program and started `{}` anyway: {answer}",
                DECLARED[0]
            )),
            Err(refused) => match refused.refusal() {
                Some(Refusal::ProgramNotDeclared { declared, .. }) if declared.is_empty() => Ok(()),
                Some(Refusal::ProgramNotDeclared { declared, .. }) => Err(format!(
                    "`programs()` is empty and the refusal names {declared:?} as the declared set"
                )),
                None => Err(format!(
                    "declared no program and refused without naming the rule: {refused}"
                )),
            },
        };
    };
    let argv = [program.clone(), "conformance".to_owned()];
    match workspace
        .operations
        .run_within(&argv, Some(Duration::from_secs(30)))
    {
        Ok(_) => Ok(()),
        Err(refused) if refused.refusal().is_some() => Err(format!(
            "refused `{program}` for not being declared, and `programs()` says it is: {refused}"
        )),
        Err(refused) => Err(format!("could not start a declared program: {refused}")),
    }
}

#[test]
fn the_declared_set_is_what_run_accepts_in_every_workspace() {
    every_workspace(
        "`programs()` is what the catalogue publishes the `run` entry's schema from, so it must be \
         what `run` accepts:",
        the_declared_set_is_what_run_accepts,
    );
}

/// Execution is offered by exactly the workspaces this machine lets confine one.
///
/// **This is the test that replaces a skip.** The confined provider on a host with no delegated
/// cgroup subtree offers no execution, and an early return there would make a machine that cannot
/// run the case indistinguishable from one where it passed. So the machine's own answer is read
/// and asserted against what each workspace publishes.
#[test]
fn execution_is_offered_by_exactly_the_workspaces_this_machine_lets_confine_one() {
    let driver_root = tempfile::tempdir().expect("a temporary root");
    let driver = Embedded::open(driver_root.path(), cgroup_root()).expect("the driver opens");
    let confines = driver
        .machine()
        .expect("the driver says what it can do")
        .confines_execution();

    let expected: Vec<(&str, bool)> = vec![
        // Nothing confines it and the constructor says so in its own name; the declared set stands.
        ("LocalOperations", true),
        // Substrate publishes execution only where the machine can confine a process.
        ("ConfinedOperations", confines),
        // `Split` executes through its effecting half, so it publishes exactly what that half does.
        ("Split", confines),
    ];
    let observed: Vec<(&str, bool)> = workspaces(DECLARED)
        .iter()
        .map(|workspace| (workspace.name, !workspace.operations.programs().is_empty()))
        .collect();
    assert_eq!(
        observed,
        expected,
        "this machine {} confine a process ({}), and the toolset must follow the machine",
        if confines { "can" } else { "cannot" },
        if cgroup_root().is_some() {
            "`B10X_CGROUP_ROOT` names a subtree"
        } else {
            "`B10X_CGROUP_ROOT` is unset, so no exec facts are reported"
        }
    );
}

// --- the suite is shareable across threads, because the trait requires it -----------------------

#[test]
fn every_implementation_is_shareable_across_threads() {
    // `Catalogue::invoke_batch` runs a turn's pure reads on one thread each, so a provider that is
    // not `Send + Sync` is one the batch cannot hold. A compile-time assertion, asked of all three.
    fn shareable<T: Send + Sync>() {}
    shareable::<LocalOperations>();
    shareable::<ConfinedOperations>();
    shareable::<Split>();
    shareable::<Box<dyn Operations>>();
}

// --- the suite's own self-test ------------------------------------------------------------------

/// A workspace that breaks one rule on purpose.
///
/// It answers every read with whatever it managed to read and calls it complete: no refusal for a
/// window the file does not have, and `truncated: false` whatever it did not reach. That is the
/// exact shape of the failure invariant 8 exists to stop, and it is here so the two tests below can
/// show that this suite catches it and says which workspace it was.
struct Divergent(LocalOperations);

impl Operations for Divergent {
    fn file_read(&self, path: &str, window: ReadWindow) -> Result<Value, String> {
        let text = self.0.file_read(path, window).map_or_else(
            |_| String::new(),
            |answer| answer["text"].as_str().unwrap_or_default().to_owned(),
        );
        Ok(json!({"path": path, "text": text, "truncated": false}))
    }

    fn file_write(&self, path: &str, text: &str) -> Result<Value, String> {
        self.0.file_write(path, text)
    }

    fn file_edit(&self, path: &str, old: &str, new: &str) -> Result<Value, String> {
        self.0.file_edit(path, old, new)
    }

    fn dir_list(&self, path: &str) -> Result<Value, String> {
        self.0.dir_list(path)
    }

    fn search(&self, pattern: &str, path: &str, options: &SearchOptions) -> Result<Value, String> {
        self.0.search(pattern, path, options)
    }

    fn find(&self, glob: &str, path: &str, max_results: Option<usize>) -> Result<Value, String> {
        self.0.find(glob, path, max_results)
    }

    fn run(&self, argv: &[String]) -> Result<Value, Refused> {
        self.0.run(argv)
    }

    fn run_within(&self, argv: &[String], remaining: Option<Duration>) -> Result<Value, Refused> {
        self.0.run_within(argv, remaining)
    }

    fn lands(&self, path: &str) -> Result<String, String> {
        self.0.lands(path)
    }

    fn programs(&self) -> &[String] {
        self.0.programs()
    }

    fn writes(&self) -> bool {
        self.0.writes()
    }
}

/// The local provider and one that breaks the read contract, side by side.
fn a_local_workspace_and_a_divergent_one() -> Vec<Workspace> {
    let (local_root, local_tree) = tree();
    let (other_root, other_tree) = tree();
    vec![
        Workspace {
            name: "LocalOperations",
            operations: Box::new(
                LocalOperations::unconfined(&local_tree, Vec::new()).expect("the workspace opens"),
            ),
            tree: local_tree,
            _root: local_root,
        },
        Workspace {
            name: "Divergent",
            operations: Box::new(Divergent(
                LocalOperations::unconfined(&other_tree, Vec::new()).expect("the workspace opens"),
            )),
            tree: other_tree,
            _root: other_root,
        },
    ]
}

#[test]
fn a_workspace_that_answers_a_read_differently_is_named_by_this_suite() {
    // Without this, a suite that cannot fail and three implementations that agree look the same.
    let named = agreed(&a_local_workspace_and_a_divergent_one(), |workspace| {
        workspace.seed("notes.txt", FIVE_LINES);
        answered(
            workspace
                .operations
                .file_read("notes.txt", ReadWindow::lines(2, 2)),
        )
    })
    .expect_err("the comparison notices a workspace that answers differently");
    assert!(
        named.contains("Divergent") && named.contains("LocalOperations"),
        "the failure must name both workspaces and what each answered: {named}"
    );
}

#[test]
fn a_workspace_that_breaks_the_read_contract_is_named_by_this_suite() {
    for (behaviour, check) in [
        (
            "a window past the end",
            a_window_past_the_end_is_refused as fn(&Workspace) -> Result<(), String>,
        ),
        (
            "a window that stops short",
            a_window_that_stops_short_says_so,
        ),
    ] {
        let named = asked(&a_local_workspace_and_a_divergent_one(), check)
            .expect_err("the runner notices a workspace that breaks the contract");
        assert!(
            named.contains("Divergent"),
            "{behaviour}: the failure must name the workspace that broke it: {named}"
        );
        assert!(
            !named.contains("LocalOperations"),
            "{behaviour}: and must not blame the one that kept it: {named}"
        );
    }
}

// --- added by adversarial verification: questions this suite does not yet ask --------------------
//
// Every case below is asked through the runners above, so a failure names the implementation that
// answers differently — the property the story's acceptance statement turns on. Three of them are
// **red against `e8d9f6b`**: three behaviours differ between the implementations and the suite as
// committed is green.

/// The same five lines with CRLF endings, which is what a file written on the other platform holds.
const FIVE_CRLF_LINES: &str = "alpha\r\nbeta\r\ngamma\r\ndelta\r\nepsilon\r\n";

/// A byte ceiling the **caller named** answers the same lines everywhere.
///
/// Not the carve-out the module header makes. That one is about each provider's *own* default
/// ceiling, "at figures each names". This is `max_bytes`, which the caller states and
/// `Catalogue::of`'s `file_read` schema publishes to the model
/// (`harness-tools/src/catalogue.rs:646`), so a model can and does name it. `ReadWindow::max_bytes`
/// is documented as "How many bytes of the file this read may take" — one number, one meaning.
///
/// It has two meanings on a CRLF file. `LocalOperations` charges the `\r` to the ceiling by name
/// (`harness-tools/src/local.rs`, `Line::length`: "separator excluded, `\r` included. What the byte
/// ceiling is charged"); `ConfinedOperations` splits with `str::lines`, which dropped the `\r`
/// before the weight was taken (`harness-substrate/src/tools.rs:194,216`). One byte a line, so the
/// two answer a different number of lines of the same file under the same stated ceiling.
#[test]
fn a_caller_named_byte_ceiling_answers_the_same_lines_of_a_crlf_file_in_every_workspace() {
    all_agree(
        "`max_bytes` is a number the caller states and the model can send; the same number over the \
         same bytes must reach the same line:",
        |workspace| {
            workspace.seed("crlf.txt", FIVE_CRLF_LINES);
            answered(workspace.operations.file_read(
                "crlf.txt",
                ReadWindow {
                    offset: None,
                    limit: None,
                    max_bytes: Some(12),
                },
            ))
        },
    );
}

/// A write to a path whose parent directory does not exist yet is **not** one answer today.
///
/// # This case pins a difference instead of asserting the contract, on purpose
///
/// The one write a model makes constantly — a new module, a new test, a new document under a
/// directory the tree does not have yet. `LocalOperations` creates the parents and has its own test
/// saying so (`harness-tools/src/local.rs`,
/// `a_new_file_under_directories_that_do_not_exist_yet_still_writes`); the confined route answers
/// `resource.not-found` and writes nothing. `Split` executes through its confined half, and `Split`
/// is what `harness-cli` composes, so this is a live difference a run sees between
/// `--substrate-embedded` and without it — not a latent one.
///
/// It is pinned rather than closed because closing it is not this unit's change and neither
/// direction is small. Making the confined route create its parents needs a directory route on
/// `Backend`, which carries five operations and has two implementations and a wire contract behind
/// it. Making the unconfined provider stop creating them removes documented, tested behaviour that
/// real runs use. **Story: `story:a-confined-write-makes-its-own-parents`.**
///
/// **When this case goes red, do not widen it.** Red here means one of the two sides moved. If the
/// confined route learned to create parents, delete this case and let
/// `a_write_puts_exactly_those_bytes_there_in_every_workspace` be asked of `deep/down/new.txt`
/// instead — the contract is then assertable and this pin is in the way. If the unconfined provider
/// stopped creating them, the same. Anything else is a regression in whichever side changed.
fn a_write_under_a_directory_that_does_not_exist_yet(workspace: &Workspace) -> Result<(), String> {
    let outcome = workspace
        .operations
        .file_write("deep/down/new.txt", FIVE_LINES);
    let on_disk = workspace.on_disk("deep/down/new.txt");
    // What each does **today**, read off the implementations and not off the trait.
    let creates_parents = workspace.name == "LocalOperations";
    let held = if creates_parents {
        outcome.is_ok() && on_disk.as_deref() == Some(FIVE_LINES)
    } else {
        // **`is_none`, and not "the bytes are not the ones asked for".** The note above says this
        // route writes *nothing*, and a comparison against `FIVE_LINES` does not say that: a write
        // that made `deep/down/` and left half a file in it answers `Err` and is not `FIVE_LINES`,
        // so it read to this pin as a clean refusal. A partial write is the one failure a pin on a
        // write path exists to catch.
        outcome.is_err() && on_disk.is_none()
    };
    if held {
        return Ok(());
    }
    Err(format!(
        "this workspace {} create the parents of a new file and left {} at `deep/down/new.txt`, \
         and now the outcome is {outcome:?} with {on_disk:?} there — see \
         `story:a-confined-write-makes-its-own-parents` and this case's own note before changing \
         anything here",
        if creates_parents { "did" } else { "did not" },
        if creates_parents {
            "the file it was given"
        } else {
            "nothing"
        },
    ))
}

#[test]
fn a_write_under_a_directory_that_does_not_exist_yet_is_one_answer_in_every_workspace() {
    every_workspace(
        "a new file under a new directory is the commonest write there is, and today one workspace \
         does it where two refuse — this case holds that split still and exactly:",
        a_write_under_a_directory_that_does_not_exist_yet,
    );
}

/// `./notes.txt` and `notes.txt` do **not** name one file in every workspace today.
///
/// # A second difference pinned rather than closed, and why the pin is where it is
///
/// A leading `./` names the same file by every reading of the trait — it is not what `# Errors`
/// means by "outside the workspace". `LocalOperations` reads it, because `Path::join` and
/// `canonicalize` resolve it; the confined route hands the path to substrate, which answers
/// `workspace.path-escape`. `Split` reads through its local half, so the same spelling is a file
/// through two of the three and a boundary violation through one.
///
/// **The fix does not belong in this crate.** `harness-substrate/src/backend.rs` says it in its own
/// words: "A path that leaves the workspace is refused by substrate and never by this crate:
/// re-implementing containment here would make two answers to one question, and the wrong one would
/// be the one nobody was looking at." Normalising `./` here is the first half of exactly that. The
/// two places it could live are substrate's own path handling and
/// `harness_tools::Catalogue`, which is the one gate every provider is reached through — and that
/// second one changes what every entry receives, for every embedder, which is a design change with
/// its own blast radius. Smaller than the parent-directory split above: nothing published tells a
/// model that `./` is legal for `file_read`, so this is habit rather than a documented workflow.
/// **Story: `story:one-spelling-of-a-path-in-every-workspace`.**
///
/// **When this case goes red**, whichever side moved, delete it and fold `./notes.txt` into
/// `every_workspace_answers_the_same_whole_small_file` — the contract is assertable at that point
/// and this pin only hides it.
///
/// # It quotes nothing of substrate's, and says how it knows the spelling is the reason
///
/// The refusing side is checked as *a refusal* plus one fact this repository owns: the **same
/// provider, the same tree, the same call** reads `notes.txt` under its plain name. That is what
/// makes the refusal about the spelling rather than about the file, and it holds whatever words
/// substrate refuses in. Matching a substring of substrate's error — `path-escape`, or the prose
/// beside it — would put a string this repository does not control inside its own gate, and a
/// rename over there, or a `Debug` rendering becoming a written sentence, would turn this red for
/// no change in behaviour.
fn a_path_spelled_with_a_leading_dot(workspace: &Workspace) -> Result<(), String> {
    workspace.seed("notes.txt", FIVE_LINES);
    let dotted = workspace
        .operations
        .file_read("./notes.txt", ReadWindow::whole());
    let plain = workspace
        .operations
        .file_read("notes.txt", ReadWindow::whole());
    // The control. Whatever happens to `./notes.txt`, the file itself is readable here — so a
    // refusal below is about how the path was spelled and not about the tree.
    if plain.as_ref().map(|answer| &answer["lines"]["total"]) != Ok(&json!(5)) {
        return Err(format!(
            "could not read `notes.txt` under its plain name, so this case cannot say anything \
             about the spelling: {plain:?}"
        ));
    }
    // What each does **today**: only the provider that hands the path to substrate refuses it.
    let refuses_the_spelling = workspace.name == "ConfinedOperations";
    match (&dotted, refuses_the_spelling) {
        (Ok(answer), false) if answer == plain.as_ref().expect("read above") => Ok(()),
        (Err(_), true) => Ok(()),
        _ => Err(format!(
            "this workspace {} read `./notes.txt` today, and now the answer is {dotted:?} — see \
             `story:one-spelling-of-a-path-in-every-workspace` and this case's own note before \
             changing anything here",
            if refuses_the_spelling {
                "refused to"
            } else {
                "did"
            },
        )),
    }
}

#[test]
fn a_path_spelled_with_a_leading_dot_names_the_same_file_in_every_workspace() {
    every_workspace(
        "`./notes.txt` is inside the workspace by every reading of the trait, and today one \
         workspace answers it as an escape — this case holds that split still and exactly:",
        a_path_spelled_with_a_leading_dot,
    );
}

/// A write that leaves the workspace is refused, and the file outside is untouched.
///
/// The module header says this is asserted — "the *write* refuses either way, and that is what is
/// asserted". Nothing above asked it: every `file_write` in this file names a path inside the tree,
/// and `a_path_outside_the_workspace_is_refused_by_every_workspace` asks it of `file_read` alone.
/// The trait states it for the write in its own `# Errors` ("or is outside the workspace"), so it
/// is a rule of the contract rather than an implementation's choice.
fn a_write_outside_the_workspace_is_refused(workspace: &Workspace) -> Result<(), String> {
    let outside = workspace.tree.parent().expect("the tree has a parent");
    let victim = outside.join("victim.txt");
    std::fs::write(&victim, "original\n").expect("a file outside the tree");
    let outcome = workspace.operations.file_write("../victim.txt", "owned\n");
    if std::fs::read_to_string(&victim).unwrap_or_default() != "original\n" {
        return Err(format!(
            "the write left the workspace and overwrote a file outside it: {outcome:?}"
        ));
    }
    match outcome {
        Ok(answer) => Err(format!(
            "reported a write to a path outside the workspace: {answer}"
        )),
        Err(why) if why.is_empty() => Err("refused with an empty sentence".to_owned()),
        Err(_) => Ok(()),
    }
}

#[test]
fn a_write_outside_the_workspace_is_refused_by_every_workspace() {
    every_workspace(
        "the workspace bounds the effects as well as the reads, and the file outside stays as it \
         was:",
        a_write_outside_the_workspace_is_refused,
    );
}

/// The same, for the other effecting operation on a file.
fn an_edit_outside_the_workspace_is_refused(workspace: &Workspace) -> Result<(), String> {
    let outside = workspace.tree.parent().expect("the tree has a parent");
    let victim = outside.join("edited.txt");
    std::fs::write(&victim, FIVE_LINES).expect("a file outside the tree");
    let outcome = workspace
        .operations
        .file_edit("../edited.txt", "gamma", "GAMMA");
    if std::fs::read_to_string(&victim).unwrap_or_default() != FIVE_LINES {
        return Err(format!(
            "the edit left the workspace and changed a file outside it: {outcome:?}"
        ));
    }
    match outcome {
        Ok(answer) => Err(format!(
            "reported an edit to a path outside the workspace: {answer}"
        )),
        Err(why) if why.is_empty() => Err("refused with an empty sentence".to_owned()),
        Err(_) => Ok(()),
    }
}

#[test]
fn an_edit_outside_the_workspace_is_refused_by_every_workspace() {
    every_workspace(
        "an edit reads and then writes, and both halves stop at the same boundary:",
        an_edit_outside_the_workspace_is_refused,
    );
}

/// A line longer than one reply may carry is cut at the same place in every workspace.
///
/// The longest line this suite otherwise reads is `epsilon`, seven characters. Both providers cut a
/// line at 2,000 characters and both say which lines they cut in `truncated_lines`, and
/// `harness-substrate/src/tools.rs:132` calls that figure "the same figure the unconfined provider
/// uses … so a run's replies must not change shape when it is confined" — a stated agreement with
/// nothing asking either side about it. Setting the confined provider's figure to `7` leaves the
/// committed suite 22/22 green; this case is what notices.
#[test]
fn a_line_longer_than_one_reply_may_carry_is_cut_the_same_way_in_every_workspace() {
    all_agree(
        "the per-line ceiling and `truncated_lines` are a stated agreement between the two \
         providers, and a run's replies must not change shape when it is confined:",
        |workspace| {
            workspace.seed("wide.txt", &format!("{}\n", "x".repeat(2_500)));
            answered(
                workspace
                    .operations
                    .file_read("wide.txt", ReadWindow::whole()),
            )
        },
    );
}

// --- second adversarial pass: the boundaries the CRLF fix has to hold at -------------------------

/// Every line-ending shape a file can have, against every window that can bite, in all three.
///
/// # Why a sweep and not another example
///
/// The fix for the CRLF ceiling replaced `str::lines` with `split_inclusive('\n')` and made two
/// claims at once: that the weight charged is the line *as the file holds it* — `\r` in, separator
/// out, which is what `Line::length` charges on the unconfined side — and that what is *shown*
/// still follows `str::lines`'s own rule, where a `\r` is a line ending only when a newline follows
/// it. Those are separate properties of one expression and they come apart at the ends of a file: a
/// last line with no newline, a lone `\r`, a file that is only separators, `\r\r\n`.
///
/// So this asks all three the same read over 17 file shapes and 26 windows — 442 questions each —
/// and names the first shapes and windows where any two answer differently. The `max_bytes` sweep
/// covers 0 and 1 as well as every ceiling that can fall inside or exactly on a line.
///
/// **A refusal is compared as *a refusal* and not by its words.** The trait's `Err` is a sentence
/// and the trait does not shape it; two providers refusing the same read in different words are
/// keeping the contract, and pinning the wording here would freeze a difference the trait allows.
/// What each refusal is required to *say* has its own cases above, and one thing it must not say
/// has [`a_refusal_for_a_short_file_does_not_blame_a_byte_ceiling`].
#[test]
fn every_line_ending_shape_answers_the_same_under_every_window_in_every_workspace() {
    /// A refusal is one outcome, whatever it says; an answer is compared whole.
    fn outcome(result: Result<Value, String>) -> Value {
        result.unwrap_or_else(|_| json!("refused"))
    }

    let shapes: &[(&str, &str)] = &[
        ("empty", ""),
        ("one_newline", "\n"),
        ("two_newlines", "\n\n"),
        ("one_crlf", "\r\n"),
        ("two_crlf", "\r\n\r\n"),
        ("lone_cr_inside_a_line", "a\rb\n"),
        ("cr_before_crlf", "a\r\r\n"),
        ("trailing_cr_and_no_newline", "alpha\nbeta\r"),
        ("no_trailing_newline", "alpha\nbeta"),
        ("cr_only_endings", "alpha\rbeta\rgamma"),
        ("five_lf", FIVE_LINES),
        ("five_crlf", FIVE_CRLF_LINES),
        ("mixed_endings", "alpha\r\nbeta\ngamma\r\n"),
        ("blank_line_between_crlf", "a\r\n\r\nb\r\n"),
        ("blank_line_between_lf", "a\n\nb\n"),
        ("multibyte_crlf", "héllo\r\nwörld\r\n"),
        ("byte_order_mark", "\u{feff}alpha\r\nbeta\r\n"),
    ];
    let mut windows: Vec<(String, ReadWindow)> = vec![
        ("whole".to_owned(), ReadWindow::whole()),
        ("lines(1,1)".to_owned(), ReadWindow::lines(1, 1)),
        ("lines(1,2)".to_owned(), ReadWindow::lines(1, 2)),
        ("lines(2,1)".to_owned(), ReadWindow::lines(2, 1)),
        ("lines(1,99)".to_owned(), ReadWindow::lines(1, 99)),
    ];
    // 0 and 1 included: the first line is answered whatever the ceiling says, and both providers
    // have to agree that it is.
    for ceiling in 0..=20u64 {
        windows.push((
            format!("max_bytes({ceiling})"),
            ReadWindow {
                offset: None,
                limit: None,
                max_bytes: Some(ceiling),
            },
        ));
    }

    let spaces = workspaces(DECLARED);
    let mut differences: Vec<String> = Vec::new();
    for (shape, content) in shapes {
        let file = format!("{shape}.txt");
        for workspace in &spaces {
            workspace.seed(&file, content);
        }
        for (window_name, window) in &windows {
            let answers: Vec<(&str, Value)> = spaces
                .iter()
                .map(|workspace| {
                    (
                        workspace.name,
                        outcome(workspace.operations.file_read(&file, *window)),
                    )
                })
                .collect();
            let (first, expected) = &answers[0];
            for (name, answer) in &answers[1..] {
                if answer != expected {
                    differences.push(format!(
                        "  {shape} @ {window_name}:\n    {first}: {expected}\n    {name}: {answer}"
                    ));
                }
            }
        }
    }
    assert!(
        differences.is_empty(),
        "the same bytes and the same window must answer the same through every workspace; {} of \
         {} questions differ:\n{}",
        differences.len(),
        shapes.len() * windows.len(),
        differences
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// A refusal must not blame a byte ceiling that nothing reached.
///
/// `ConfinedOperations::file_read` computes `ceiling_cut` — whether the route answered its whole
/// ceiling — and consults it for `bytes`, `truncated`, `lines.total` and the `note` it attaches.
/// The **refusal** for an `offset` past the end does not consult it: it is one sentence for both
/// cases, and it tells the model "the route answers from the start of the file up to a byte ceiling
/// of N bytes, and that is where it stopped", plus, in the answer path's own words, that lines past
/// that ceiling "cannot be reached on this path at any `offset`".
///
/// On a three-byte file under a 1 MiB route ceiling nothing stopped anywhere. A model reading line
/// 2 of a one-line file is told the file was cut off by a limit, which is the mirror of invariant
/// 8: not a truncation reported as whole, but a whole answer reported as truncated — and the move
/// it invites, giving up on a file it has entirely seen, is worse than the one invariant 8 stops.
/// The unconfined provider says the true thing in the same case: "`x` has 1 lines and `offset`
/// names line 2, which is past the end."
fn a_refusal_for_a_short_file_does_not_blame_a_byte_ceiling(
    workspace: &Workspace,
) -> Result<(), String> {
    workspace.seed("short.txt", "only one line\n");
    let Err(why) = workspace
        .operations
        .file_read("short.txt", ReadWindow::lines(2, 1))
    else {
        return Err("answered a window past the end of a one-line file".to_owned());
    };
    if why.contains("byte ceiling") || why.contains("cannot be reached") {
        return Err(format!(
            "a 14-byte file was refused as though a byte ceiling had cut it: {why}"
        ));
    }
    Ok(())
}

#[test]
fn a_refusal_for_a_short_file_does_not_blame_a_byte_ceiling_in_any_workspace() {
    every_workspace(
        "a refusal the model reads has to be true; a file nothing truncated must not be refused as \
         one that was:",
        a_refusal_for_a_short_file_does_not_blame_a_byte_ceiling,
    );
}

// --- second adversarial pass: the runner's own empty case ---------------------------------------

/// `asked` reports a pass when it was handed no workspace at all.
///
/// The header now claims **"No branch in this file reports a pass without asserting something."**
/// [`agreed`] holds that line itself — handed nothing it answers "no workspace was asked, so
/// nothing was compared" — and [`all_agree`] holds a second one, refusing a comparison in which
/// every workspace refused. [`asked`], which is what [`every_workspace`] and
/// [`every_read_only_workspace`] run on, holds neither: an empty slice produces an empty `failures`
/// and `Ok(())`, which `every_workspace` reads as *every workspace met the contract*.
///
/// Not reachable from the two callers today, both of which build three workspaces by hand. That is
/// the same thing that was true of `the_declared_set_is_what_run_accepts`'s early return until a
/// machine with no delegated cgroup subtree reached it, and the guard `agreed` already carries is
/// four lines.
#[test]
fn the_runner_refuses_to_report_a_pass_when_it_was_handed_no_workspace() {
    let nothing: [Workspace; 0] = [];
    let outcome = asked(&nothing, a_plain_path_lands_under_its_own_name);
    assert!(
        outcome.is_err(),
        "`asked` answered {outcome:?} for an empty set of workspaces, so `every_workspace` would \
         report a pass having asked nobody anything — the property the module header states"
    );
}

/// `agreed` reports agreement when there was nobody to disagree.
///
/// The other half of the header's claim, one runner over. Handed a single workspace the comparison
/// loop never ran, and `all_agree` would take that `Ok` as *all three answered the same*. Its own
/// callers pass three, and so did `the_declared_set_is_what_run_accepts`'s caller pass a workspace
/// that could take the branch nobody expected to be taken.
#[test]
fn the_comparison_refuses_to_report_agreement_between_one_workspace_and_itself() {
    let alone: Vec<Workspace> = workspaces(DECLARED).into_iter().take(1).collect();
    let outcome = agreed(&alone, |workspace| {
        workspace.seed("notes.txt", FIVE_LINES);
        answered(
            workspace
                .operations
                .file_read("notes.txt", ReadWindow::whole()),
        )
    });
    assert!(
        outcome.is_err(),
        "`agreed` answered {outcome:?} for one workspace, so `all_agree` would report that every \
         workspace agreed having compared nothing"
    );
    assert!(
        agreed(&[], |_| json!(null)).is_err(),
        "and the empty case is the same hole with nothing in it"
    );
}

// --- second adversarial pass: is the parent-directory pin tight in both directions? --------------

/// A workspace that leaves a partial file behind and then reports the write failed.
///
/// Exactly the shape a write that created its parents, wrote some of the bytes and then hit
/// substrate's refusal would leave: a file on disk that is not what was asked for, and an `Err`.
struct LeavesAPartialFile(LocalOperations);

impl Operations for LeavesAPartialFile {
    fn file_write(&self, path: &str, text: &str) -> Result<Value, String> {
        let partial: String = text.chars().take(5).collect();
        let _ = self.0.file_write(path, &partial);
        Err("workspace.file-write: resource.not-found".to_owned())
    }

    fn file_read(&self, path: &str, window: ReadWindow) -> Result<Value, String> {
        self.0.file_read(path, window)
    }

    fn file_edit(&self, path: &str, old: &str, new: &str) -> Result<Value, String> {
        self.0.file_edit(path, old, new)
    }

    fn dir_list(&self, path: &str) -> Result<Value, String> {
        self.0.dir_list(path)
    }

    fn search(&self, pattern: &str, path: &str, options: &SearchOptions) -> Result<Value, String> {
        self.0.search(pattern, path, options)
    }

    fn find(&self, glob: &str, path: &str, max_results: Option<usize>) -> Result<Value, String> {
        self.0.find(glob, path, max_results)
    }

    fn run(&self, argv: &[String]) -> Result<Value, Refused> {
        self.0.run(argv)
    }

    fn lands(&self, path: &str) -> Result<String, String> {
        self.0.lands(path)
    }

    fn programs(&self) -> &[String] {
        self.0.programs()
    }

    fn writes(&self) -> bool {
        self.0.writes()
    }
}

/// The parent-directory pin passes a confined workspace that left a partial file behind.
///
/// The pin's own note says the confined route "answers `resource.not-found` and **writes
/// nothing**", and the two halves it checks are `outcome.is_ok()` and
/// `on_disk(..) == Some(FIVE_LINES)`. A write that created `deep/down/` and left the wrong bytes in
/// it satisfies both: the outcome is an `Err` as expected, and the file on disk is not `FIVE_LINES`
/// so `landed` is `false` as expected. The half that says *nothing is on disk* is not asserted, and
/// a partial write is the one failure a pin on a write path exists to catch.
#[test]
fn the_parent_directory_pin_notices_a_write_that_left_something_behind() {
    let (root, tree) = tree();
    let partial = vec![Workspace {
        // The pin branches on this name, so this is the workspace it holds to *not writing*.
        name: "ConfinedOperations",
        operations: Box::new(LeavesAPartialFile(
            LocalOperations::unconfined(&tree, Vec::new()).expect("the workspace opens"),
        )),
        tree,
        _root: root,
    }];
    let outcome = asked(&partial, a_write_under_a_directory_that_does_not_exist_yet);
    let left_behind = partial[0].on_disk("deep/down/new.txt");
    assert!(
        outcome.is_err(),
        "the write left `{left_behind:?}` at `deep/down/new.txt` and reported a failure, and the \
         pin answered {outcome:?} — it holds `on_disk(..) == Some(FIVE_LINES)` and not `nothing is \
         on disk`, so a partial write reads to it as a clean refusal"
    );
}

// --- second adversarial pass: the two story ids the pins point at -------------------------------

/// The ids written into the source are the two the coordinator is filing, character for character.
#[test]
fn the_pinned_cases_name_stories_by_their_exact_ids() {
    let source = include_str!("conformance.rs");
    for id in [
        "story:a-confined-write-makes-its-own-parents",
        "story:one-spelling-of-a-path-in-every-workspace",
    ] {
        assert!(
            source.contains(id),
            "a pin points at `{id}` and nothing in this file spells it that way"
        );
    }
}
