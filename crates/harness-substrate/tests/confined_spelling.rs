//! What `ConfinedOperations` hands substrate when a path is spelled with `.` or `..` components.
//!
//! The confined provider drops `.` components and nothing else
//! (`story:one-spelling-of-a-path-in-every-workspace`). `tests/conformance.rs` asks that a dotted
//! spelling names the plain file in every workspace; this file asks the other half, which only the
//! confined provider can be asked: that a `..` is still substrate's to refuse, **exactly as it
//! refused it before** — the path reaches substrate as written, and the refusal is the one
//! substrate gives that spelling when it is asked directly, code and words alike.
//!
//! The backend here is substrate's own embedded driver behind a recorder, so what reached it is
//! observed rather than inferred.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use b10x_harness_substrate::{
    Backend, ConfinedOperations, Embedded, Facts, ProcessWorkspaceAccess, SubstrateError,
};
use harness_catalogue::{Operations, ReadWindow};
use serde_json::Value;
use tempfile::TempDir;

/// The embedded driver, and every path that reached it.
struct Recording {
    driver: Arc<Embedded>,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Backend for Recording {
    fn machine(&self) -> Result<Facts, SubstrateError> {
        self.driver.machine()
    }

    fn workspace_create(&self, lease_ttl_ms: u64) -> Result<String, SubstrateError> {
        self.driver.workspace_create(lease_ttl_ms)
    }

    fn file_write(&self, workspace: &str, path: &str, text: &str) -> Result<Value, SubstrateError> {
        self.seen
            .lock()
            .expect("not poisoned")
            .push(path.to_owned());
        self.driver.file_write(workspace, path, text)
    }

    fn file_read(&self, workspace: &str, path: &str) -> Result<String, SubstrateError> {
        self.seen
            .lock()
            .expect("not poisoned")
            .push(path.to_owned());
        self.driver.file_read(workspace, path)
    }

    fn exec(
        &self,
        workspace: &str,
        argv: &[String],
        workspace_access: &ProcessWorkspaceAccess,
        remaining: Option<Duration>,
    ) -> Result<Value, SubstrateError> {
        self.driver
            .exec(workspace, argv, workspace_access, remaining)
    }
}

/// A confined provider over `root/work`, the driver it holds, the workspace id, and the recorder.
struct Confined {
    operations: ConfinedOperations,
    driver: Arc<Embedded>,
    workspace: String,
    seen: Arc<Mutex<Vec<String>>>,
    root: TempDir,
}

impl Confined {
    fn open() -> Self {
        let root = tempfile::tempdir().expect("a temporary root");
        std::fs::create_dir(root.path().join("work")).expect("the tree");
        std::fs::create_dir(root.path().join("work/a")).expect("a directory in the tree");
        let driver =
            Arc::new(Embedded::open(root.path(), None).expect("the embedded driver opens"));
        let facts = driver.machine().expect("the driver says what it can do");
        let workspace = driver.workspace_adopt("work").expect("the tree is adopted");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let operations = ConfinedOperations::new(
            Recording {
                driver: Arc::clone(&driver),
                seen: Arc::clone(&seen),
            },
            &facts,
            workspace.clone(),
            Vec::new(),
        );
        Self {
            operations,
            driver,
            workspace,
            seen,
            root,
        }
    }

    /// What reached substrate since the last time this was asked.
    fn reached(&self) -> Vec<String> {
        std::mem::take(&mut *self.seen.lock().expect("not poisoned"))
    }
}

/// Spellings that climb out of the tree, and contain no `.` component, so nothing may touch them.
const CLIMBING: &[&str] = &["../victim.txt", "a/../../victim.txt"];

#[test]
fn a_path_that_climbs_out_is_handed_to_substrate_as_written_and_refused_as_substrate_refuses_it() {
    let confined = Confined::open();
    let victim = confined.root.path().join("victim.txt");
    std::fs::write(&victim, "original\n").expect("a file outside the tree");
    for &path in CLIMBING {
        let direct = confined
            .driver
            .file_read(&confined.workspace, path)
            .map_err(|error| error.to_string());
        let Err(direct) = direct else {
            panic!("substrate itself read `{path}`, outside the tree: {direct:?}");
        };
        assert!(
            direct.contains("workspace.path-escape"),
            "substrate refuses `{path}` as an escape, and this case is about that refusal: {direct}"
        );

        let read = confined.operations.file_read(path, ReadWindow::whole());
        assert_eq!(
            confined.reached(),
            [path],
            "the read reached substrate as written"
        );
        assert_eq!(
            read,
            Err(direct.clone()),
            "`{path}` is refused through ConfinedOperations exactly as substrate refuses it"
        );

        let write = confined.operations.file_write(path, "owned\n");
        assert_eq!(
            confined.reached(),
            [path],
            "the write reached substrate as written"
        );
        let direct_write = confined
            .driver
            .file_write(&confined.workspace, path, "owned\n")
            .map_err(|error| error.to_string());
        assert!(
            direct_write.is_err(),
            "substrate itself wrote `{path}`: {direct_write:?}"
        );
        assert_eq!(
            write, direct_write,
            "`{path}` is refused for a write through ConfinedOperations exactly as substrate \
             refuses it"
        );

        let edit = confined.operations.file_edit(path, "original", "owned");
        assert_eq!(
            confined.reached(),
            [path],
            "the edit's read reached substrate as written"
        );
        assert_eq!(
            edit,
            Err(direct),
            "`{path}` is refused for an edit through ConfinedOperations exactly as substrate \
             refuses it"
        );
    }
    assert_eq!(
        std::fs::read_to_string(&victim).expect("the file outside is still there"),
        "original\n",
        "nothing outside the tree changed"
    );
}

#[test]
fn a_dot_component_is_dropped_before_substrate_and_nothing_else_is() {
    let confined = Confined::open();
    std::fs::write(confined.root.path().join("work/notes.txt"), "kept\n").expect("a seed");
    std::fs::write(confined.root.path().join("work/a/b.txt"), "kept\n").expect("a seed");
    for (spelled, handed) in [
        ("./notes.txt", "notes.txt"),
        ("a/./b.txt", "a/b.txt"),
        ("./a/./b.txt", "a/b.txt"),
        ("./../victim.txt", "../victim.txt"),
        ("a/./../../victim.txt", "a/../../victim.txt"),
    ] {
        let _ = confined.operations.file_read(spelled, ReadWindow::whole());
        assert_eq!(
            confined.reached(),
            [handed],
            "`{spelled}` reaches substrate as `{handed}`: the `.` components and nothing else \
             are dropped"
        );
    }
}
