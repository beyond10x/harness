//! Second adversary pass for the write-scope class across units 4 and 5
//! (story:a-written-directory-is-judged-by-what-it-holds,
//! story:rust-fmt-judges-the-modules-it-would-rewrite). Drives the built-in `rust_fmt` through
//! the public catalogue against the real `rustfmt` 1.10.0, on the module-resolution paths a
//! public advisory would be checked against: the non-root fallback, a cfg-gated module, a
//! unicode name, a symlinked declaring file, a module several non-roots deep, and the
//! over-refusal corner (this crate's own source must still format under the default profile).
//!
//! Each case that denies a module seeds that module file with unformatted source and asserts it
//! is unchanged after the call: if harness admitted the call, the real `rustfmt` would rewrite
//! the denied file, and the assertion would catch it. Needs a rustup `stable` toolchain under
//! `$RUSTUP_HOME` or `~/.rustup`.

#![cfg(unix)]

use std::path::Path;

use harness_catalogue::{Catalogue, LocalOperations, ResolvedProvider, Scope, ScopeRule};
use serde_json::json;

fn rules(declarations: &[&str]) -> Scope {
    Scope::of(
        declarations
            .iter()
            .map(|declaration| ScopeRule::parse(declaration).expect("a rule"))
            .collect(),
    )
}

/// The real built-in Rust provider resolved in `root`, where rustup `stable` is installed.
fn rust(root: &Path) -> Vec<ResolvedProvider> {
    tool_toolchain::Registry::builtins()
        .expect("the built-ins")
        .resolve(root, Some(&["rust".to_owned()]))
        .expect("the built-in Rust provider resolves where rustup `stable` is installed")
}

/// The catalogue the local adapter serves, starting the providers' programs and nothing else.
fn local(root: &Path, providers: Vec<ResolvedProvider>) -> Catalogue {
    let programs = providers
        .iter()
        .flat_map(|provider| provider.programs.iter().cloned())
        .collect();
    let operations = LocalOperations::unconfined(root, programs).expect("a workspace");
    Catalogue::of_with_toolchains(operations, providers)
}

const MANIFEST: &str = "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";

/// Writes each `(path, text)` under `root`, creating parents.
fn holding(root: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let at = root.join(path);
        std::fs::create_dir_all(at.parent().expect("a parent")).expect("its directory");
        std::fs::write(at, text).expect("a file");
    }
}

/// Unformatted Rust source; the real `rustfmt` rewrites it to `pub fn _m() {}\n`.
const UNFMT: &str = "pub   fn  _m(){}\n";

// --- unit 5: module resolution against the real rustfmt, on its trickiest paths --------------

#[test]
fn rust_fmt_refuses_a_module_reached_only_by_the_non_root_fallback() {
    // The input file `src/a.rs` is a non-root (the directory `src/a` stands beside it), declares
    // `mod c;`, and `src/a/c.rs` does not exist, so rustfmt falls back beside it to `src/c.rs`
    // (probed against rustfmt 1.10.0: it rewrites `src/c.rs`). If harness did not model the
    // fallback it would judge the non-existent `src/a/c.rs`, admit the call, and rustfmt would
    // rewrite the denied `src/c.rs`.
    let dir = tempfile::tempdir().expect("a workspace");
    holding(
        dir.path(),
        &[
            ("Cargo.toml", MANIFEST),
            ("src/a.rs", "mod c;\n"),
            ("src/a/other.rs", ""),
            ("src/c.rs", UNFMT),
        ],
    );
    let catalogue = local(dir.path(), rust(dir.path())).scoped(rules(&["src/c.rs=denied"]));

    let answer = catalogue.invoke("rust_fmt", &json!({"paths": ["src/a.rs"]}));

    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/c.rs")).expect("read"),
        UNFMT,
        "`rust_fmt src/a.rs` rewrote `src/c.rs` (the non-root fallback target) under \
         `src/c.rs=denied`: {answer:?}"
    );
}

#[test]
fn rust_fmt_refuses_a_cfg_gated_module_the_formatter_rewrites_regardless() {
    // `#[cfg(any())] mod gone;` is always-false, yet rustfmt rewrites `src/gone.rs` regardless of
    // the cfg (probed). A scope that denies it must still refuse the call.
    let dir = tempfile::tempdir().expect("a workspace");
    holding(
        dir.path(),
        &[
            ("Cargo.toml", MANIFEST),
            ("src/lib.rs", "#[cfg(any())]\nmod gone;\n"),
            ("src/gone.rs", UNFMT),
        ],
    );
    let catalogue = local(dir.path(), rust(dir.path())).scoped(rules(&["src/gone.rs=denied"]));

    let answer = catalogue.invoke("rust_fmt", &json!({"paths": ["src/lib.rs"]}));

    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/gone.rs")).expect("read"),
        UNFMT,
        "`rust_fmt src/lib.rs` rewrote `src/gone.rs` under a false `cfg` and `src/gone.rs=denied`: \
         {answer:?}"
    );
}

#[test]
fn rust_fmt_refuses_a_unicode_named_module() {
    // `mod café;` resolves to `src/café.rs` (probed: rustfmt rewrites it). The scanner reads the
    // name through proc-macro2's identifier lexer; a denial of the non-ASCII path must hold.
    let dir = tempfile::tempdir().expect("a workspace");
    holding(
        dir.path(),
        &[
            ("Cargo.toml", MANIFEST),
            ("src/lib.rs", "mod café;\n"),
            ("src/café.rs", UNFMT),
        ],
    );
    let catalogue = local(dir.path(), rust(dir.path())).scoped(rules(&["src/café.rs=denied"]));

    let answer = catalogue.invoke("rust_fmt", &json!({"paths": ["src/lib.rs"]}));

    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/café.rs")).expect("read"),
        UNFMT,
        "`rust_fmt src/lib.rs` rewrote `src/café.rs` under `src/café.rs=denied`: {answer:?}"
    );
}

#[test]
fn rust_fmt_refuses_a_module_reached_through_a_symlinked_declaring_file() {
    // `src/a.rs` is a symlink to `real/a.rs`; it declares `mod b;`, and because `src/a` stands
    // beside the link rustfmt resolves it to `src/a/b.rs` (probed: rustfmt rewrites `src/a/b.rs`,
    // using the link's lexical path, not its target). A denial of `src/a/b.rs` must hold.
    let dir = tempfile::tempdir().expect("a workspace");
    holding(
        dir.path(),
        &[
            ("Cargo.toml", MANIFEST),
            ("src/lib.rs", "mod a;\n"),
            ("real/a.rs", "mod b;\n"),
            ("src/a/b.rs", UNFMT),
        ],
    );
    std::os::unix::fs::symlink("../real/a.rs", dir.path().join("src/a.rs"))
        .expect("a symlinked declaring file");
    let catalogue = local(dir.path(), rust(dir.path())).scoped(rules(&["src/a/b.rs=denied"]));

    let answer = catalogue.invoke("rust_fmt", &json!({"paths": ["src/lib.rs"]}));

    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/a/b.rs")).expect("read"),
        UNFMT,
        "`rust_fmt src/lib.rs` rewrote `src/a/b.rs` reached through the symlinked `src/a.rs` \
         under `src/a/b.rs=denied`: {answer:?}"
    );
}

#[test]
fn rust_fmt_refuses_a_module_three_non_roots_deep() {
    // lib -> a (non-root) -> b (non-root under src/a) -> c; rustfmt rewrites `src/a/b/c.rs`
    // (probed). A denial of the deepest file must hold.
    let dir = tempfile::tempdir().expect("a workspace");
    holding(
        dir.path(),
        &[
            ("Cargo.toml", MANIFEST),
            ("src/lib.rs", "mod a;\n"),
            ("src/a.rs", "mod b;\n"),
            ("src/a/b.rs", "mod c;\n"),
            ("src/a/b/c.rs", UNFMT),
        ],
    );
    let catalogue = local(dir.path(), rust(dir.path())).scoped(rules(&["src/a/b/c.rs=denied"]));

    let answer = catalogue.invoke("rust_fmt", &json!({"paths": ["src/lib.rs"]}));

    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/a/b/c.rs")).expect("read"),
        UNFMT,
        "`rust_fmt src/lib.rs` rewrote `src/a/b/c.rs` three non-roots deep under \
         `src/a/b/c.rs=denied`: {answer:?}"
    );
}

// --- unit 5: over-refusal on a real workspace (acceptance: an ordinary crate still formats) ---

#[test]
fn rust_fmt_formats_this_crates_own_source_under_the_default_profile() {
    // The acceptance bullet: "With no refusing rule in the scope nothing is scanned", and an
    // ordinary crate still formats. The default profile DOES refuse (`.git/**`), so the scan
    // runs over this crate's real module tree. If any real module trips the fail-closed scanner,
    // the crate the advisory ships in could not be formatted under its own default profile.
    let dir = tempfile::tempdir().expect("a workspace");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    std::fs::create_dir_all(dir.path().join("src")).expect("src");
    std::fs::write(dir.path().join("Cargo.toml"), MANIFEST).expect("a manifest");
    // Copy every real module file this crate's `src/lib.rs` reaches.
    for entry in std::fs::read_dir(&src).expect("read src") {
        let entry = entry.expect("a dir entry");
        if entry.path().extension().and_then(|e| e.to_str()) == Some("rs") {
            let name = entry.file_name();
            std::fs::copy(entry.path(), dir.path().join("src").join(&name)).expect("copy a module");
        }
    }
    let catalogue =
        local(dir.path(), rust(dir.path())).scoped(rules(&[".git/**=denied", "**=allowed"]));

    let answer = catalogue.invoke("rust_fmt", &json!({"paths": ["src/lib.rs"]}));

    // The built-in `rust_fmt` runs bare `rustfmt`, which may itself fail to format newer syntax;
    // that is not an over-refusal. The scanner's refusal is the one that must not appear.
    if let Err(refused) = &answer {
        let message = refused.message();
        assert!(
            !message.contains("runs only when every module it reaches is judged")
                && !message.contains("Name the files to format, none of which declares a refused"),
            "the module scanner refused this crate's own source under the default profile \
             (over-refusal): {message}"
        );
    }
}

// --- the class, beyond the formatters units 4/5 fixed: a toolchain tool that writes paths it ----
// --- did not declare is never judged by the write scope at all ----------------------------------

#[test]
#[ignore = "by design: a toolchain tool that declares no written paths is a Risk::High process call gated by approval and confinement, not by the write scope (adversary-scope-pass-2, finding 1)"]
fn rust_build_writes_a_scope_denied_path_through_a_build_script_the_scope_never_judged() {
    // The class a public advisory would claim fixed: "a toolchain tool writes a file the write
    // scope refuses, through any path the scope did not judge." The built-in `rust_build` is
    // `workspace-writes: true` and declares no `writes:`, so the executor judges nothing it
    // writes. `cargo build` always runs `build.rs`, which has full filesystem access, so a build
    // script writes `protected/evil.rs` while `protected/**=denied` is in force — a route the
    // write scope never saw. Units 4 and 5 (formatter directories and rustfmt modules) do not
    // touch this path. Needs a rustup `stable` toolchain.
    let dir = tempfile::tempdir().expect("a workspace");
    holding(
        dir.path(),
        &[
            ("Cargo.toml", MANIFEST),
            (
                "Cargo.lock",
                "version = 3\n\n[[package]]\nname = \"probe\"\nversion = \"0.1.0\"\n",
            ),
            ("src/main.rs", "fn main() {}\n"),
            (
                "build.rs",
                "use std::io::Write;\nfn main() {\n    let mut f = \
                 std::fs::File::create(\"protected/evil.rs\").expect(\"denied path\");\n    \
                 f.write_all(b\"pub fn pwned() {}\\n\").unwrap();\n}\n",
            ),
            ("protected/evil.rs", "ORIGINAL\n"),
        ],
    );
    let catalogue = local(dir.path(), rust(dir.path())).scoped(rules(&["protected/**=denied"]));

    let answer = catalogue.invoke("rust_build", &json!({}));

    assert_eq!(
        std::fs::read_to_string(dir.path().join("protected/evil.rs")).expect("read"),
        "ORIGINAL\n",
        "`rust_build` ran a build script that rewrote `protected/evil.rs` under \
         `protected/**=denied`; the write scope judged no path of this call: {answer:?}"
    );
}
