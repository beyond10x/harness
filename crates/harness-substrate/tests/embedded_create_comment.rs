//! The second statement in `embedded.rs` about substrate's name rule, which was left standing.
//!
//! `1acad51` corrected `workspace_adopt`'s rustdoc and pinned it
//! (`crates/harness-substrate/tests/embedded_live.rs:162`). Sixty lines below the corrected
//! sentence, `Backend::workspace_create`'s body comment still says the opposite, as fact, about the
//! same function of the same pinned substrate:
//!
//! > the driver has two checks that disagree about what is legal: `HostDriver::workspace_path`
//! > admits `[A-Za-z0-9_-]`, while `validate_root_name` inside the guarded filesystem requires the
//! > `ws_` prefix and refuses a hyphen.
//!
//! At the pinned tag `0.2.2` (`43c5a10`) `validate_root_name` requires neither: it takes any single
//! path component of ASCII alphanumerics, `_` and `-` that is not empty, `.`, `..` or leading `-`.
//! The two checks therefore agree, and the comment's conclusion — "Meeting the stricter of the two
//! is the only thing a caller can do about that" — rests on a difference that is not there.
//!
//! So one file now holds both answers, and a reader who lands on the second one is told the rule
//! `0c31438` and substrate `0.2.2` removed. That is the defect
//! `story:help-text-names-a-rule-the-code-dropped` was opened about, on the surface immediately
//! next to the one it was closed on.

use b10x_harness_substrate::Embedded;

/// The claim `workspace_create`'s comment makes, run against the driver it makes it about.
///
/// **Measured before it is read.** A name that begins with no prefix and holds a hyphen is handed
/// to the driver through the guarded path the comment names; it is accepted. So
/// `validate_root_name` neither requires `ws_` nor refuses a hyphen at the pinned tag, and the two
/// checks the comment says disagree do not.
///
/// Then the comment is read out of this crate's own source — the same way
/// `the_documentation_on_workspace_adopt_states_the_rule_its_body_enforces` reads the rustdoc, and
/// for the same reason: a comment is reachable from nowhere else, which is exactly how this one
/// survived the commit that falsified it and then survived the change that corrected its neighbour.
#[test]
fn the_comment_on_workspace_create_states_the_rule_the_pinned_substrate_enforces() {
    let root = tempfile::tempdir().expect("a temporary root");
    std::fs::create_dir(root.path().join("work-native")).expect("a project tree");
    let embedded = Embedded::open(root.path(), None).expect("the driver opens");
    assert_eq!(
        embedded
            .workspace_adopt("work-native")
            .expect("no prefix, one hyphen, and the driver represents it"),
        "work-native",
        "the driver's own name check admits a hyphenated, unprefixed name"
    );

    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("embedded.rs");
    let text = std::fs::read_to_string(&source)
        .unwrap_or_else(|error| panic!("reading `{}`: {error}", source.display()));

    let signature = "fn workspace_create(&self, lease_ttl_ms: u64)";
    let lines: Vec<&str> = text.lines().collect();
    let declared = lines
        .iter()
        .position(|line| line.contains(signature))
        .unwrap_or_else(|| panic!("`{signature}` is in `{}`", source.display()));
    let body: Vec<&str> = lines[declared + 1..]
        .iter()
        .take_while(|line| line.trim_start().starts_with("//"))
        .copied()
        .collect();
    assert!(
        !body.is_empty(),
        "`workspace_create` opens with a comment, or there is nothing here to check"
    );
    let body = body.join("\n");

    assert!(
        !body.contains("requires the `ws_` prefix"),
        "`workspace_create`'s own comment says `validate_root_name` requires the `ws_` prefix and \
         refuses a hyphen. The driver this test just handed `work-native` to does neither, and \
         `workspace_adopt`'s rustdoc sixty lines above now says so. One file, two \
         answers:\n{body}"
    );
    assert!(
        !body.contains("two checks that disagree"),
        "and the conclusion drawn from it — that the two checks disagree, so a caller must meet \
         the stricter — is what a reader carries away. At `0.2.2` they agree:\n{body}"
    );
}
