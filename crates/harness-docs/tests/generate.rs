//! `harness-docs generate`, `generate --check` and `provenance`, run as the binary on this
//! repository, and the status list's refusal of evidence that does not exist.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use harness_docs::status::{Capability, data_for};

/// The repository root: two levels above this crate.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A fresh directory under the tree's own build directory, never the system temporary one.
fn scratch(name: &str) -> PathBuf {
    let dir = root()
        .join("target/harness-docs")
        .join(format!("test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// `harness-docs` with `args`, its environment isolated from the operator's configuration.
fn docs(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_harness-docs"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .output()
        .unwrap()
}

fn describe(output: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// A copy of the repository's hand-written pages under `out`, plus the generated ones, so the
/// check can be run against a tree a test may break.
fn generated_copy(name: &str) -> (PathBuf, String) {
    let out = scratch(name);
    let root = root();
    copy_tree(&root.join("website/docs"), &out.join("website/docs"));
    let root_arg = root.to_str().unwrap().to_owned();
    let written = docs(
        &out,
        &[
            "generate",
            "--root",
            &root_arg,
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert!(written.status.success(), "{}", describe(&written));
    (out, root_arg)
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn check(out: &Path, root: &str) -> Output {
    docs(
        out,
        &[
            "generate",
            "--check",
            "--root",
            root,
            "--out",
            out.to_str().unwrap(),
        ],
    )
}

#[test]
fn generate_check_passes_on_the_tree() {
    let home = scratch("check-home");
    let root = root();
    let output = docs(
        &home,
        &["generate", "--check", "--root", root.to_str().unwrap()],
    );
    assert!(output.status.success(), "{}", describe(&output));
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn a_changed_extra_or_missing_page_fails_the_check_naming_each() {
    let (out, root) = generated_copy("drift");
    let clean = check(&out, &root);
    assert!(clean.status.success(), "{}", describe(&clean));

    let cli = out.join("website/docs/reference/cli.md");
    let edited = fs::read_to_string(&cli).unwrap() + "\nedited by hand\n";
    fs::write(&cli, edited).unwrap();
    fs::write(
        out.join("website/docs/reference/extra.md"),
        "---\ntitle: Extra\nsidebar_position: 9\ndescription: Stray.\n---\n\nstray\n",
    )
    .unwrap();
    fs::remove_file(out.join("website/docs/status.md")).unwrap();

    let drifted = check(&out, &root);
    assert_eq!(drifted.status.code(), Some(1), "{}", describe(&drifted));
    let stderr = String::from_utf8_lossy(&drifted.stderr);
    for named in [
        "website/docs/reference/cli.md",
        "website/docs/reference/extra.md",
        "website/docs/status.md",
    ] {
        assert!(stderr.contains(named), "{named} not named:\n{stderr}");
    }
    for unchanged in [
        "website/docs/reference/toolchains.md",
        "website/docs/reference/configuration.md",
        "website/docs/reference/wires.md",
    ] {
        assert!(
            !stderr.contains(unchanged),
            "an unchanged page is named:\n{stderr}"
        );
    }
    fs::remove_dir_all(out).unwrap();
}

#[test]
fn generate_keeps_the_hand_written_reference_pages() {
    let (out, _) = generated_copy("keep");
    for page in ["configuration.md", "wires.md"] {
        let kept = fs::read_to_string(out.join("website/docs/reference").join(page)).unwrap();
        let original =
            fs::read_to_string(root().join("website/docs/reference").join(page)).unwrap();
        assert_eq!(kept, original, "{page}");
    }
    fs::remove_dir_all(out).unwrap();
}

#[test]
fn a_hand_written_page_with_a_brace_or_a_capitalised_tag_in_prose_fails_the_check() {
    let (out, root) = generated_copy("passive");
    fs::write(
        out.join("website/docs/guides/a.md"),
        "---\ntitle: A\nsidebar_position: 9\ndescription: D.\nlede: L.\nsource: S.\n---\n\nCode `{ok}` and `<Ok>`.\n\nA { in prose.\n\nA <Value> tag.\n\n:::caution Planned\n:::\n",
    )
    .unwrap();
    let refused = check(&out, &root);
    assert_eq!(refused.status.code(), Some(1), "{}", describe(&refused));
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("guides/a.md:11"), "{stderr}");
    assert!(stderr.contains("guides/a.md:13"), "{stderr}");
    assert!(stderr.contains("guides/a.md:15"), "{stderr}");
    assert!(!stderr.contains("guides/a.md:9"), "{stderr}");
    fs::remove_dir_all(out).unwrap();
}

#[test]
fn a_page_without_its_front_matter_keys_fails_the_check_naming_each_key() {
    let (out, root) = generated_copy("front-keys");
    fs::write(
        out.join("website/docs/guides/b.md"),
        "---\ntitle: B\ndescription: D.\n---\n\nBody.\n",
    )
    .unwrap();
    fs::write(
        out.join("website/docs/tutorials/c.md"),
        "---\ntitle: C\nsidebar_position: 3\ndescription: D.\n---\n\nBody.\n",
    )
    .unwrap();
    fs::write(
        out.join("website/docs/tutorials/d.md"),
        "No front matter.\n",
    )
    .unwrap();
    let refused = check(&out, &root);
    assert_eq!(refused.status.code(), Some(1), "{}", describe(&refused));
    let stderr = String::from_utf8_lossy(&refused.stderr);
    for named in [
        "guides/b.md: no sidebar_position",
        "guides/b.md: no lede",
        "guides/b.md: no source",
        "tutorials/d.md: no front matter",
    ] {
        assert!(stderr.contains(named), "{named} not named:\n{stderr}");
    }
    assert!(!stderr.contains("tutorials/c.md"), "{stderr}");
    fs::remove_dir_all(out).unwrap();
}

#[test]
fn a_page_pointing_at_internal_material_or_naming_a_stale_version_fails_the_check() {
    let (out, root) = generated_copy("public");
    let index = out.join("website/docs/index.md");
    let text = fs::read_to_string(&index).unwrap();
    fs::write(
        &index,
        text.replace(env!("CARGO_PKG_VERSION"), "0.0.0-stale")
            + "\nSee [the design](../../docs/design/0001.md).\n",
    )
    .unwrap();
    let refused = check(&out, &root);
    assert_eq!(refused.status.code(), Some(1), "{}", describe(&refused));
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("docs/design"), "{stderr}");
    assert!(
        stderr.contains("index.md does not name the workspace version"),
        "{stderr}"
    );
    fs::remove_dir_all(out).unwrap();
}

#[test]
fn every_generated_page_carries_the_front_matter_and_the_generator_line() {
    let (out, _) = generated_copy("front");
    for page in [
        "website/docs/reference/cli.md",
        "website/docs/reference/toolchains.md",
        "website/docs/reference/crates.md",
        "website/docs/status.md",
    ] {
        let text = fs::read_to_string(out.join(page)).unwrap();
        let (front, body) = text
            .strip_prefix("---\n")
            .and_then(|rest| rest.split_once("\n---\n"))
            .unwrap_or_else(|| panic!("{page} has no front matter"));
        assert_eq!(
            front.lines().next(),
            Some("# generated by harness-docs, do not edit"),
            "{page}"
        );
        for key in [
            "title:",
            "sidebar_position:",
            "description:",
            "custom_edit_url: null",
        ] {
            assert!(
                front.lines().any(|line| line.starts_with(key)),
                "{page}: {key}"
            );
        }
        assert_eq!(
            body.trim_start_matches('\n').lines().next(),
            Some("<!-- generated by harness-docs, do not edit -->"),
            "{page}"
        );
        assert!(!text.contains("story:"), "{page} names a story");
    }
    let status: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.join("website/data/status.json")).unwrap())
            .unwrap();
    assert_eq!(status["format"], "b10x-status/1");
    fs::remove_dir_all(out).unwrap();
}

#[test]
fn the_cli_page_lists_every_command_flag_and_the_contract_version() {
    let page = harness_docs::cli::page();
    assert!(page.contains("## `b10x-harness run`"), "{page}");
    assert!(page.contains("Usage: b10x-harness run"), "{page}");
    assert!(page.contains("## `b10x-harness providers show`"), "{page}");
    assert!(
        page.contains(harness_cli::contract::ARGV_CONTRACT_VERSION),
        "{page}"
    );
    assert!(!page.contains("b10x-harness help"), "{page}");
    assert!(!page.contains("b10x-harness workflow"), "{page}");
    let contract: serde_json::Value = serde_json::from_str(&harness_cli::contract::argv()).unwrap();
    for rows in contract["arguments"].as_object().unwrap().values() {
        for row in rows.as_array().unwrap() {
            // A positional has no long form, and `--help` is named once in the page's intro
            // rather than in every table.
            let Some(long) = row["long"].as_str().filter(|long| *long != "--help") else {
                continue;
            };
            assert!(page.contains(&format!("`{long}")), "{long} missing");
        }
    }
    for command in contract["subcommands"].as_array().unwrap() {
        let command = command.as_str().unwrap();
        assert!(
            page.contains(&format!("## `b10x-harness {command}`")),
            "{command} missing"
        );
    }
}

#[test]
fn the_toolchains_page_is_the_tools_reference_under_this_generators_front_matter() {
    let page = harness_docs::toolchains::page().unwrap();
    assert!(page.starts_with("---\n# generated by harness-docs, do not edit\n"));
    assert!(!page.contains("cargo xtask toolchain-docs"), "{page}");
    assert!(page.contains("## `rust`"), "{page}");
    assert_eq!(page.matches("\n---\n").count(), 1, "{page}");
}

fn capability(status: &'static str, evidence: Option<&'static str>) -> Capability {
    Capability {
        area: "Area",
        label: "Label",
        detail: "Detail.",
        status,
        evidence,
        href: None,
    }
}

#[test]
fn a_status_item_naming_a_missing_test_is_refused() {
    let root = root();
    let gone = capability(
        "shipped",
        Some("crates/harness-loop/src/budget.rs::no_such_test"),
    );
    let error = data_for(&root, &[gone]).unwrap_err().to_string();
    assert!(error.contains("no_such_test"), "{error}");
    let present = capability(
        "shipped",
        Some("crates/harness-loop/src/budget.rs::a_zero_bound_refuses_by_name"),
    );
    assert!(data_for(&root, &[present]).is_ok());
    assert!(data_for(&root, &[capability("shipped", None)]).is_err());
    let claimed = capability(
        "planned",
        Some("crates/harness-loop/src/budget.rs::a_zero_bound_refuses_by_name"),
    );
    assert!(data_for(&root, &[claimed]).is_err());
    assert!(data_for(&root, &[capability("released", None)]).is_err());
    assert!(data_for(&root, &[capability("planned", None)]).is_ok());
}

#[test]
fn provenance_writes_the_site_record_and_trailing_slash_routes() {
    let site = scratch("provenance");
    let commit = "e86f43d90b637fc2c1790f77d59efff2eb852386";
    let site_arg = site.to_str().unwrap();
    let refused = docs(
        &site,
        &["provenance", "--site", site_arg, "--commit", commit],
    );
    assert_eq!(refused.status.code(), Some(1), "{}", describe(&refused));

    fs::create_dir_all(site.join("docs/reference")).unwrap();
    fs::write(site.join("index.html"), r#"<main id="main">"#).unwrap();
    fs::write(site.join("404.html"), r#"<main id="lost">"#).unwrap();
    fs::write(site.join("docs.html"), r#"<h1 id="one-loop">"#).unwrap();
    fs::write(
        site.join("docs/reference/cli.html"),
        r#"<h2 id="b10x-harness-run">"#,
    )
    .unwrap();
    fs::write(
        site.join("docs/status.html"),
        r#"<meta http-equiv="refresh" content="0; url=/harness/docs/status/">"#,
    )
    .unwrap();
    let bad = docs(
        &site,
        &["provenance", "--site", site_arg, "--commit", "e86f43d"],
    );
    assert_eq!(bad.status.code(), Some(1), "{}", describe(&bad));
    let bound = docs(
        &site,
        &["provenance", "--site", site_arg, "--commit", commit],
    );
    assert!(bound.status.success(), "{}", describe(&bound));

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(site.join(".well-known/b10x-site.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["commit"], commit);
    assert_eq!(manifest["baseUrl"], "/harness/");
    assert_eq!(manifest["repository"], "harness");
    assert_eq!(manifest["schema"], "b10x-project-site/v1");
    let routes: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(site.join(".well-known/b10x-routes.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(routes["schema"], "b10x-project-routes/v1");
    assert_eq!(routes["commit"], commit);
    let paths: Vec<&str> = routes["routes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|route| route["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        [
            "/harness/",
            "/harness/docs/",
            "/harness/docs/reference/cli/"
        ]
    );
    assert_eq!(routes["routes"][2]["anchors"][0], "b10x-harness-run");
    assert!(site.join(".nojekyll").is_file());
    fs::remove_dir_all(site).unwrap();
}
