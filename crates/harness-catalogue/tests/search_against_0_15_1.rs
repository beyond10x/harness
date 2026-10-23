//! `search` through the real adapter, held to what harness 0.15.1 answered and to what
//! `CHANGELOG.md` `[Unreleased]` says changed when it moved onto `b10x-tool-search`.
//!
//! The adapter's own tests compare its reply with `tool_search`'s, which is the same code on both
//! sides: a change in the pinned crate moves both and stays green. These cases write the 0.15.1
//! reply out as literals instead, so a reply change reaches a red test and not only a changelog.
//!
//! One field was added since: `deadline_reached`, which `search` v1alpha2 (tools `0.5.0`) answers
//! on every call, so the literals carry it as `false`.

use std::path::Path;
use std::sync::Mutex;

use harness_catalogue::{
    Catalogue, LocalOperations, Operations, ReadWindow, Refused, SearchOptions,
};
use serde_json::{Value, json};

fn workspace() -> tempfile::TempDir {
    tempfile::tempdir().expect("a temporary workspace")
}

fn search(
    root: &Path,
    pattern: &str,
    path: &str,
    options: &SearchOptions,
) -> Result<Value, String> {
    LocalOperations::new(root)
        .expect("a workspace")
        .search(pattern, path, options)
}

fn paths(answer: &Value) -> Vec<String> {
    answer["matches"]
        .as_array()
        .expect("a list of matches")
        .iter()
        .map(|found| found["path"].as_str().expect("a path").to_owned())
        .collect()
}

fn context(lines: std::ops::RangeInclusive<u64>, text: impl Fn(u64) -> String) -> Value {
    Value::Array(
        lines
            .map(|line| json!({"line": line, "text": text(line), "line_truncated": false}))
            .collect(),
    )
}

#[test]
fn a_search_answers_0_15_1s_reply_byte_for_byte() {
    // 0.15.1's `grep`: numbered context under `before`/`after`, a line cut at 400 characters on a
    // character boundary with `line_truncated`, `regex` and `glob` echoed, keys in one order.
    let dir = workspace();
    let long = format!("needle{}", "é".repeat(400));
    let exact = format!("needle{}", "é".repeat(394));
    std::fs::write(
        dir.path().join("a.txt"),
        format!("zero\none\nneedle here\nthree\n{long}\n{exact}\n"),
    )
    .expect("the seed");
    std::fs::create_dir(dir.path().join("sub")).expect("the seed");
    std::fs::write(dir.path().join("sub/b.rs"), "needle\n").expect("the seed");

    let options = SearchOptions {
        regex: true,
        glob: Some("*.txt".to_owned()),
        context: Some(1),
        max_results: None,
    };
    let answer = search(dir.path(), "need+le", ".", &options).expect("the search");
    let expected = json!({
        "pattern": "need+le",
        "matches": [
            {"path": "a.txt", "line": 3, "text": "needle here", "line_truncated": false,
             "before": [{"line": 2, "text": "one", "line_truncated": false}],
             "after": [{"line": 4, "text": "three", "line_truncated": false}]},
            {"path": "a.txt", "line": 5, "text": exact, "line_truncated": true,
             "before": [{"line": 4, "text": "three", "line_truncated": false}],
             "after": [{"line": 6, "text": exact, "line_truncated": false}]},
            {"path": "a.txt", "line": 6, "text": exact, "line_truncated": false,
             "before": [{"line": 5, "text": exact, "line_truncated": true}],
             "after": []},
        ],
        "truncated": false,
        "depth_bound_reached": false,
        "skipped_large_files": 0,
        "skipped_large_paths": [],
        "regex": true,
        "glob": "*.txt",
        "deadline_reached": false,
    });
    assert_eq!(answer.to_string(), expected.to_string());

    let capped = SearchOptions {
        context: Some(6),
        ..SearchOptions::default()
    };
    let answer = search(dir.path(), "needle", "sub", &capped).expect("the search");
    let expected = json!({
        "pattern": "needle",
        "matches": [
            {"path": "sub/b.rs", "line": 1, "text": "needle", "line_truncated": false,
             "before": [], "after": []},
        ],
        "truncated": false,
        "depth_bound_reached": false,
        "skipped_large_files": 0,
        "skipped_large_paths": [],
        "context": 5,
        "note": "`context` was 6 and this build answers at most 5 lines either side of a match, \
                 so 5 is what these matches carry.",
        "deadline_reached": false,
    });
    assert_eq!(answer.to_string(), expected.to_string());
}

#[test]
fn the_bounds_bite_where_0_15_1s_did() {
    let dir = workspace();
    let default = SearchOptions::default();

    // 201 matches: 200 answered, the last on line 200, and the reply says there are more.
    std::fs::write(dir.path().join("many.txt"), "needle\n".repeat(201)).expect("the seed");
    let answer = search(dir.path(), "needle", "many.txt", &default).expect("the search");
    assert_eq!(answer["matches"].as_array().expect("a list").len(), 200);
    assert_eq!(answer["matches"][199]["line"], json!(200));
    assert_eq!(answer["truncated"], json!(true));
    // A larger `max_results` is capped at the same 200.
    let wide = SearchOptions {
        max_results: Some(1000),
        ..SearchOptions::default()
    };
    let answer = search(dir.path(), "needle", "many.txt", &wide).expect("the search");
    assert_eq!(answer["matches"].as_array().expect("a list").len(), 200);
    assert_eq!(answer["truncated"], json!(true));

    // Exactly 200 and then a line that does not match: complete (the change the CHANGELOG lists).
    std::fs::write(
        dir.path().join("two-hundred.txt"),
        format!("{}other\n", "needle\n".repeat(200)),
    )
    .expect("the seed");
    let answer = search(dir.path(), "needle", "two-hundred.txt", &default).expect("the search");
    assert_eq!(answer["matches"].as_array().expect("a list").len(), 200);
    assert_eq!(answer["truncated"], json!(false));

    // A file of exactly 1 MiB is read; one byte more is counted, named and not read.
    let mib = 1024 * 1024;
    std::fs::create_dir(dir.path().join("big")).expect("the seed");
    std::fs::write(
        dir.path().join("big/at.txt"),
        format!("needle\n{}", "x".repeat(mib - 7)),
    )
    .expect("the seed");
    std::fs::write(
        dir.path().join("big/over.txt"),
        format!("needle\n{}", "x".repeat(mib - 6)),
    )
    .expect("the seed");
    let answer = search(dir.path(), "needle", "big", &default).expect("the search");
    assert_eq!(paths(&answer), ["big/at.txt"]);
    assert_eq!(answer["skipped_large_files"], json!(1));
    assert_eq!(answer["skipped_large_paths"], json!(["big/over.txt"]));
    assert_eq!(answer["truncated"], json!(true));

    // Context 5 is five lines either side, numbered; 6 is capped at 5 and says so.
    let text = |line: u64| format!("l{line}");
    let lines = (1..=11)
        .map(|line| {
            if line == 6 {
                "needle".to_owned()
            } else {
                text(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(dir.path().join("ctx.txt"), format!("{lines}\n")).expect("the seed");
    for asked in [5, 6] {
        let options = SearchOptions {
            context: Some(asked),
            ..SearchOptions::default()
        };
        let answer = search(dir.path(), "needle", "ctx.txt", &options).expect("the search");
        assert_eq!(answer["matches"][0]["before"], context(1..=5, text));
        assert_eq!(answer["matches"][0]["after"], context(7..=11, text));
        assert_eq!(answer.get("context").is_some(), asked == 6, "{answer}");
        assert_eq!(answer.get("note").is_some(), asked == 6, "{answer}");
    }
}

#[cfg(unix)]
#[test]
fn search_never_answers_from_outside_the_workspace() {
    use std::os::unix::fs::symlink;

    let outside = workspace();
    std::fs::write(outside.path().join("secret.txt"), "needle outside\n").expect("the seed");
    let dir = workspace();
    std::fs::write(dir.path().join("own.txt"), "needle inside\n").expect("the seed");
    symlink(outside.path(), dir.path().join("out-dir")).expect("a link out");
    symlink(
        outside.path().join("secret.txt"),
        dir.path().join("out-file.txt"),
    )
    .expect("a link out");
    symlink(dir.path().join("own.txt"), dir.path().join("in-link.txt")).expect("a link in");

    let answer = search(dir.path(), "needle", ".", &SearchOptions::default()).expect("the search");
    assert_eq!(paths(&answer), ["in-link.txt", "own.txt"]);
    assert!(!answer.to_string().contains("outside"), "{answer}");

    let sibling = format!(
        "../{}",
        outside
            .path()
            .file_name()
            .expect("a name")
            .to_string_lossy()
    );
    let absolute = outside.path().display().to_string();
    for path in [
        "out-dir",
        "out-file.txt",
        "..",
        "/",
        sibling.as_str(),
        absolute.as_str(),
    ] {
        let refused = search(dir.path(), "needle", path, &SearchOptions::default())
            .expect_err("a path out of the workspace is refused");
        assert_eq!(
            refused,
            format!("`{path}` resolves outside the workspace and was refused")
        );
    }

    // A workspace opened through a link answers workspace-relative paths, not the link's target.
    let via = workspace();
    symlink(dir.path(), via.path().join("ws")).expect("a link to the workspace");
    let answer = search(
        &via.path().join("ws"),
        "needle",
        ".",
        &SearchOptions::default(),
    )
    .expect("the search");
    assert_eq!(paths(&answer), ["in-link.txt", "own.txt"]);
}

#[cfg(unix)]
#[test]
fn an_absolute_or_linked_path_that_names_nothing_outside_is_refused_as_outside_not_as_not_found() {
    // CHANGELOG.md [Unreleased]: a `path` that names nothing is judged by where its deepest
    // existing ancestor lands, and one outside the workspace answers "`<path>` resolves outside
    // the workspace and was refused" where 0.15.1 answered "`<path>`: No such file or directory
    // (os error 2)". An absolute path outside, and a name under a link that leads out, are both
    // that case.
    use std::os::unix::fs::symlink;

    let outside = workspace();
    let dir = workspace();
    symlink(outside.path(), dir.path().join("out-dir")).expect("a link out");
    let absolute = outside.path().join("missing").display().to_string();
    let asked = [absolute.as_str(), "out-dir/missing"];

    let answered = asked
        .iter()
        .map(|path| {
            search(dir.path(), "needle", path, &SearchOptions::default())
                .expect_err("a path that names nothing is refused")
        })
        .collect::<Vec<_>>();
    let in_0_16_0 = asked
        .iter()
        .map(|path| format!("`{path}` resolves outside the workspace and was refused"))
        .collect::<Vec<_>>();
    assert_eq!(answered, in_0_16_0);
}

#[test]
fn a_path_that_names_nothing_is_judged_by_where_its_deepest_existing_ancestor_lands() {
    // CHANGELOG.md [Unreleased]: the ancestors are read as spelled, nearest first. Under
    // `missing/../..` the nearest that exists is the workspace itself, so the answer is the
    // not-found error although the path would climb out; under `../nowhere` it is the workspace's
    // parent, so the answer is the outside refusal.
    let dir = workspace();
    std::fs::write(dir.path().join("notes.txt"), "needle\n").expect("the seed");
    for path in ["missing/../..", "missing/../../elsewhere"] {
        let refused = search(dir.path(), "needle", path, &SearchOptions::default())
            .expect_err("a path that names nothing is refused");
        assert_eq!(
            refused,
            format!("`{path}`: No such file or directory (os error 2)"),
        );
    }
    let refused = search(
        dir.path(),
        "needle",
        "../nowhere",
        &SearchOptions::default(),
    )
    .expect_err("a path that climbs out to nothing is refused");
    assert_eq!(
        refused,
        "`../nowhere` resolves outside the workspace and was refused"
    );
}

/// Records every argv it is asked to start and starts none.
struct Recording {
    reads: LocalOperations,
    programs: Vec<String>,
    started: Mutex<Vec<Vec<String>>>,
}

impl Operations for Recording {
    fn file_read(&self, path: &str, window: ReadWindow) -> Result<Value, String> {
        self.reads.file_read(path, window)
    }
    fn file_write(&self, _path: &str, _text: &str) -> Result<Value, String> {
        Err("this provider writes nothing".to_owned())
    }
    fn file_edit(&self, _path: &str, _old: &str, _new: &str) -> Result<Value, String> {
        Err("this provider writes nothing".to_owned())
    }
    fn dir_list(&self, path: &str) -> Result<Value, String> {
        self.reads.dir_list(path)
    }
    fn search(&self, pattern: &str, path: &str, options: &SearchOptions) -> Result<Value, String> {
        self.reads.search(pattern, path, options)
    }
    fn run(&self, argv: &[String]) -> Result<Value, Refused> {
        self.started
            .lock()
            .expect("not poisoned")
            .push(argv.to_vec());
        Ok(json!({"stdout": "", "exit": 0}))
    }
    fn programs(&self) -> &[String] {
        &self.programs
    }
}

#[test]
fn a_listing_check_is_not_turned_into_a_rewrite_by_an_argument_spelled_as_an_option() {
    // The `tools` 0.3.0 pin this unit took also moves `b10x-tool-toolchain`, whose `plan` now
    // refuses a value beginning with `-` before any separator (tools CHANGELOG [0.3.0] Fixed:
    // "`-w` turned `go_fmt_check` into a rewrite"). At tools 0.2.0 (harness 0.15.1) this call
    // planned `cargo fmt --check -w src/lib.rs` and reached the program. Shaped like the built-in
    // `go_fmt_check`, on `cargo` so it resolves wherever this suite builds.
    let dir = workspace();
    std::fs::write(dir.path().join("probe.lock"), "").expect("the marker");
    std::fs::create_dir(dir.path().join("src")).expect("the seed");
    std::fs::write(dir.path().join("src/lib.rs"), "").expect("the seed");
    let spec = dir.path().join("probe.yaml");
    std::fs::write(
        &spec,
        "version: 1\n\
         toolchains:\n\
         \x20 - name: probe\n\
         \x20   description: A listing check shaped like go_fmt_check.\n\
         \x20   project:\n\
         \x20     all: [probe.lock]\n\
         \x20   sandbox:\n\
         \x20     programs: [cargo]\n\
         \x20     env:\n\
         \x20       HOME: \"{workspace}\"\n\
         \x20       PATH: /usr/local/bin:/usr/bin:/bin\n\
         \x20   tools:\n\
         \x20     - name: probe_fmt_check\n\
         \x20       description: List the named files that are not formatted.\n\
         \x20       role: fmt-check\n\
         \x20       empty-stdout: true\n\
         \x20       parameters:\n\
         \x20         paths: { kind: workspace-path-list, required: true, min-items: 1, max-items: 128 }\n\
         \x20       commands:\n\
         \x20         - argv: [cargo, fmt, --check, { each: paths }]\n",
    )
    .expect("the spec");
    let mut registry = tool_toolchain::Registry::builtins().expect("the built-ins");
    registry.load_file(&spec).expect("the probe spec loads");
    let providers = registry
        .resolve(dir.path(), Some(&["probe".to_owned()]))
        .expect("the probe provider resolves");
    let operations = Recording {
        reads: LocalOperations::new(dir.path()).expect("a workspace"),
        programs: vec!["cargo".to_owned()],
        started: Mutex::new(Vec::new()),
    };
    let catalogue = Catalogue::of_with_toolchains(operations, providers);

    let answer = catalogue.invoke("probe_fmt_check", &json!({"paths": ["-w", "src/lib.rs"]}));
    let refused = answer.expect_err("an option-spelled path is refused before anything starts");
    assert!(refused.to_string().contains("begins with `-`"), "{refused}");
}
