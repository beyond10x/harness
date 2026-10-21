//! A trailing `/.` after a name, asked of every workspace.
//!
//! `story:one-spelling-of-a-path-in-every-workspace` drops `.` components in
//! `ConfinedOperations` so that a spelling names one file in every workspace. `notes.txt/.` is a
//! spelling POSIX resolves only when `notes.txt` is a directory; `LocalOperations` asks the
//! operating system and is refused (`ENOTDIR`) for a read, and its writer refuses a `fresh/.` that
//! names nothing as not a file. These cases ask that the confined provider, and `Split` over both,
//! give the same kind of answer: refused where the others refuse, answered where they answer, and
//! the same file on disk afterwards. Refusal wording is each provider's own and is not compared.

use std::path::PathBuf;

use b10x_harness_substrate::{Backend, ConfinedOperations, Embedded};
use harness_catalogue::{LocalOperations, Operations, ReadWindow, Split};
use serde_json::Value;
use tempfile::TempDir;

struct Workspace {
    name: &'static str,
    tree: PathBuf,
    operations: Box<dyn Operations>,
    _root: TempDir,
}

fn tree() -> (TempDir, PathBuf) {
    let root = tempfile::tempdir().expect("a temporary root");
    let tree = root.path().join("work");
    std::fs::create_dir(&tree).expect("the tree");
    (root, tree)
}

fn confined(root: &TempDir) -> ConfinedOperations {
    let driver = Embedded::open(root.path(), None).expect("the embedded driver opens");
    let facts = driver.machine().expect("the driver says what it can do");
    let workspace = driver.workspace_adopt("work").expect("the tree is adopted");
    ConfinedOperations::new(driver, &facts, workspace, vec!["/bin/echo".to_owned()])
}

fn workspaces() -> Vec<Workspace> {
    let (local_root, local_tree) = tree();
    let (confined_root, confined_tree) = tree();
    let (split_root, split_tree) = tree();
    vec![
        Workspace {
            name: "LocalOperations",
            operations: Box::new(
                LocalOperations::unconfined(&local_tree, vec!["/bin/echo".to_owned()])
                    .expect("the workspace opens"),
            ),
            tree: local_tree,
            _root: local_root,
        },
        Workspace {
            name: "ConfinedOperations",
            operations: Box::new(confined(&confined_root)),
            tree: confined_tree,
            _root: confined_root,
        },
        Workspace {
            name: "Split",
            operations: Box::new(Split::new(
                LocalOperations::new(&split_tree).expect("the reading half opens"),
                confined(&split_root),
            )),
            tree: split_tree,
            _root: split_root,
        },
    ]
}

/// What a workspace did, compared across workspaces, and the detail a failure prints beside it.
struct Observed {
    name: &'static str,
    compared: String,
    detail: String,
}

fn observed(name: &'static str, result: &Result<Value, String>, extra: &str) -> Observed {
    let (outcome, detail) = match result {
        Ok(value) => ("answered", value.to_string()),
        Err(why) => ("refused", why.clone()),
    };
    Observed {
        name,
        compared: format!("{outcome}{extra}"),
        detail,
    }
}

fn agree(behaviour: &str, all: &[Observed]) {
    if all.iter().any(|one| one.compared != all[0].compared) {
        let named: Vec<String> = all
            .iter()
            .map(|one| format!("  {}: {} [{}]", one.name, one.compared, one.detail))
            .collect();
        panic!("{behaviour}\n{}", named.join("\n"));
    }
}

#[test]
fn a_trailing_dot_after_a_file_is_read_alike_in_every_workspace() {
    let all: Vec<Observed> = workspaces()
        .iter()
        .map(|workspace| {
            std::fs::write(workspace.tree.join("notes.txt"), "kept\n").expect("a seed");
            let read = workspace
                .operations
                .file_read("notes.txt/.", ReadWindow::whole());
            observed(workspace.name, &read, "")
        })
        .collect();
    agree(
        "`notes.txt/.` is answered in one workspace and refused in another:",
        &all,
    );
}

#[test]
fn a_write_to_a_trailing_dot_that_names_nothing_lands_alike_in_every_workspace() {
    let all: Vec<Observed> = workspaces()
        .iter()
        .map(|workspace| {
            let write = workspace.operations.file_write("fresh/.", "made\n");
            let created = workspace.tree.join("fresh").exists();
            observed(
                workspace.name,
                &write,
                &format!(", file `fresh` created: {created}"),
            )
        })
        .collect();
    agree(
        "a write to `fresh/.` does something different in different workspaces:",
        &all,
    );
}
