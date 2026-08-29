//! Where a run may write, declared before it starts.
//!
//! # Why this is in the toolset and not at a seam
//!
//! Every other arm of the evaluation narrows a run by adjudicating each call at a decision seam.
//! This loop has no seam and deliberately never grows one: its claim is that **the published
//! toolset is the policy**, so a tool that must not act here should refuse here, in the same place
//! it refuses a program nobody declared.
//!
//! A live run against this repository's own corpus rewrote artifact files whole, under a directory
//! whose frontmatter is owned by a CLI. Nothing stopped it, because nothing had been told. The rule
//! had existed for a year in another arm's driver, written in that vendor's tool names.
//!
//! # Granularity, not identity
//!
//! A rule says `allowed`, `partial-only` or `denied` — never an operation name. Which entries
//! replace a whole file is *this* crate's fact and stays here: `file_write` replaces a file and
//! `file_edit` changes part of one, which is exactly the distinction the store's rule needs and
//! exactly the distinction no list of operations can make.
//!
//! # First match wins, and silence is not permission
//!
//! Rules are ordered and the first whose glob matches decides. A call whose path matches nothing is
//! **allowed**: this is a declaration of where writing is *restricted*, and a scope nobody wrote
//! restricts nothing. The document that produces these rules is where a catch-all is mandatory —
//! there a missing tail is an oversight, here an empty list is a deliberate absence.

use serde::{Deserialize, Serialize};

/// How much of a file may be changed under a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WriteScope {
    /// Whole-file replacement and partial edits are both fine.
    Allowed,
    /// Part of a file may change; a whole file may never be replaced.
    PartialOnly,
    /// Nothing may be written here.
    Denied,
}

impl WriteScope {
    /// Reads the word a declaration uses.
    ///
    /// # Errors
    ///
    /// Names what was written and what the three words are. A misspelling that silently became
    /// `allowed` would be a boundary that quietly is not one.
    pub fn parse(word: &str) -> Result<Self, String> {
        match word {
            "allowed" => Ok(Self::Allowed),
            "partial-only" => Ok(Self::PartialOnly),
            "denied" => Ok(Self::Denied),
            other => Err(format!(
                "`{other}` is not a write scope; there is `allowed`, `partial-only` and `denied`"
            )),
        }
    }
}

/// One ordered rule: these paths, this much writing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeRule {
    /// A glob over the path as the caller wrote it. `*` within a segment, `**` across them.
    pub paths: String,
    /// What writing is allowed there.
    pub write: WriteScope,
}

impl ScopeRule {
    /// Reads `<glob>=<allowed|partial-only|denied>`.
    ///
    /// # Errors
    ///
    /// Names the half that is wrong. The separator is the last `=`, so a glob may contain one.
    pub fn parse(declaration: &str) -> Result<Self, String> {
        let (paths, word) = declaration.rsplit_once('=').ok_or_else(|| {
            format!("`{declaration}` is not `<glob>=<allowed|partial-only|denied>`")
        })?;
        if paths.trim().is_empty() {
            return Err(format!("`{declaration}` names no path"));
        }
        Ok(Self {
            paths: paths.trim().to_owned(),
            write: WriteScope::parse(word.trim())?,
        })
    }
}

/// Every rule a run was given, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope(Vec<ScopeRule>);

impl Scope {
    /// A scope from ordered rules.
    #[must_use]
    pub fn of(rules: Vec<ScopeRule>) -> Self {
        Self(rules)
    }

    /// Whether nothing was declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The rules, in order.
    #[must_use]
    pub fn rules(&self) -> &[ScopeRule] {
        &self.0
    }

    /// Why this operation may not touch this path, or [`None`] if it may.
    ///
    /// The refusal names the path, what was refused and **what would work instead**. A message that
    /// said only "denied" would be retried until the run's turn budget ran out, which is money
    /// spent on a wall.
    ///
    /// The rule is matched against a lexically normalised path, not the spelling the caller used,
    /// because otherwise `./target/x` walks past a rule that refuses `target/x`. The message still
    /// names the path as it was written — that is what the caller has to change.
    #[must_use]
    pub fn refusal(&self, operation: &str, path: &str) -> Option<String> {
        if !matches!(operation, "file.write" | "file.edit") {
            return None;
        }
        // A scope nobody wrote restricts nothing, so nothing below it may turn silence into a
        // refusal — including the absolute-path rule.
        if self.0.is_empty() {
            return None;
        }
        if path.starts_with('/') {
            return Some(format!(
                "`{path}` is absolute, and a scoped run's paths are relative to the workspace \
                 root. This scope does not know where that root is, so it cannot tell whether the \
                 path falls under a rule — and a write nobody could judge is exactly how a \
                 declared scope gets stepped around. Spell the path relative to the workspace root."
            ));
        }
        let normalised = normalise(path);
        let rule = self
            .0
            .iter()
            .find(|rule| glob_matches(&rule.paths, &normalised))?;
        match rule.write {
            WriteScope::Allowed => None,
            WriteScope::PartialOnly if operation == "file.edit" => None,
            WriteScope::PartialOnly => Some(format!(
                "`{path}` may be changed in part but never replaced whole, so `file_write` is \
                 refused there. Use `file_edit`, which names the exact text to replace — a whole \
                 file rewritten by hand is indistinguishable from one silently altered."
            )),
            WriteScope::Denied => Some(format!(
                "`{path}` is outside what this run may change. Nothing here writes to it; if the \
                 work needs that path, the run was scoped wrongly and a person should say so."
            )),
        }
    }
}

/// The path as the rules see it: empty and `.` segments dropped, `..` resolved against what came
/// before.
///
/// A glob matched against the raw string is a boundary anyone can step around by spelling the path
/// differently. `target/x`, `./target/x` and `crates/../target/x` are one file, and a rule that
/// refuses `target/**` has to refuse all three.
///
/// Lexical rather than filesystem: this crate cannot resolve links from here and does not need to,
/// because the provider refuses by where a path *lands* as well. A `..` that would climb above the
/// start is kept rather than dropped, so such a path still matches no workspace-relative glob
/// instead of quietly turning into one that does.
fn normalise(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if matches!(parts.last(), None | Some(&"..")) => parts.push(".."),
            ".." => {
                parts.pop();
            }
            name => parts.push(name),
        }
    }
    parts.join("/")
}

/// `*` matches within a path segment, `**` across them, everything else is literal.
///
/// No regular expressions and no dependency. A scope is a boundary somebody has to be able to read
/// at a glance, and what a regex adds is the power to write one nobody can check.
fn glob_matches(pattern: &str, value: &str) -> bool {
    fn go(pattern: &[u8], value: &[u8]) -> bool {
        match pattern.first() {
            None => value.is_empty(),
            Some(b'*') => {
                let crosses = pattern.get(1) == Some(&b'*');
                let rest = if crosses {
                    &pattern[2..]
                } else {
                    &pattern[1..]
                };
                // `**/` names zero directories as readily as many, so `**/*.md` has to match
                // `README.md` at the root. Without this it needs a `/` somewhere in the value and
                // the rule silently skips the top level, which is where the files people mean by
                // "every markdown file" usually start.
                if crosses && rest.first() == Some(&b'/') && go(&rest[1..], value) {
                    return true;
                }
                for taken in 0..=value.len() {
                    if !crosses && value[..taken].contains(&b'/') {
                        break;
                    }
                    if go(rest, &value[taken..]) {
                        return true;
                    }
                }
                false
            }
            Some(&expected) => value.first() == Some(&expected) && go(&pattern[1..], &value[1..]),
        }
    }
    go(pattern.as_bytes(), value.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Scope {
        Scope::of(vec![
            ScopeRule::parse(".engineering/planning/**=partial-only").expect("reads"),
            ScopeRule::parse("target/**=denied").expect("reads"),
        ])
    }

    #[test]
    fn the_rule_no_list_of_operations_can_express() {
        // Both are writes. Under the store one is right and the other re-types frontmatter a CLI
        // owns, and that is the whole reason this is granularity rather than identity.
        let path = ".engineering/planning/story/a.md";
        assert!(store().refusal("file.write", path).is_some());
        assert!(store().refusal("file.edit", path).is_none());
    }

    #[test]
    fn a_refusal_names_the_path_and_the_way_in() {
        // A denial that says only "denied" is retried until the turn budget runs out.
        let refusal = store()
            .refusal("file.write", ".engineering/planning/story/a.md")
            .expect("refused");
        assert!(
            refusal.contains(".engineering/planning/story/a.md"),
            "{refusal}"
        );
        assert!(refusal.contains("file_edit"), "{refusal}");
    }

    #[test]
    fn a_denied_path_refuses_both_and_says_it_is_the_scope_that_is_wrong() {
        assert!(store().refusal("file.write", "target/debug/x").is_some());
        let refusal = store()
            .refusal("file.edit", "target/debug/x")
            .expect("refused");
        assert!(refusal.contains("scoped wrongly"), "{refusal}");
    }

    #[test]
    fn a_path_no_rule_mentions_is_allowed_and_a_scope_nobody_wrote_restricts_nothing() {
        // This is a declaration of where writing is *restricted*. The document that produces these
        // rules is where a catch-all is mandatory; here an empty list is a deliberate absence.
        assert!(
            store()
                .refusal("file.write", "crates/cli/src/main.rs")
                .is_none()
        );
        assert!(Scope::default().refusal("file.write", "anything").is_none());
    }

    #[test]
    fn reading_a_rule_names_the_half_that_is_wrong() {
        assert!(ScopeRule::parse("crates/**").is_err(), "no scope word");
        assert!(ScopeRule::parse("=denied").is_err(), "no path");
        let bad = ScopeRule::parse("crates/**=readonly").expect_err("refused");
        assert!(bad.contains("partial-only"), "{bad}");
    }

    #[test]
    fn a_denied_path_spelled_another_way_is_still_the_same_path() {
        // The glob used to be matched against the raw string, so every one of these but the first
        // was allowed under `target/**=denied`.
        for path in [
            "target/x",
            "./target/x",
            "././target/x",
            "crates/../target/x",
            "target/./x",
        ] {
            assert!(store().refusal("file.write", path).is_some(), "{path}");
        }
        assert!(store().refusal("file.write", "crates/x.rs").is_none());
    }

    #[test]
    fn an_absolute_path_is_refused_because_the_scope_cannot_judge_it() {
        // The scope does not know the workspace root, so it cannot tell what an absolute path is
        // under. Waving it through is the bypass; refusing it costs a turn and a rewrite.
        let refusal = store()
            .refusal("file.write", "/abs/target/x")
            .expect("refused");
        assert!(refusal.contains("absolute"), "{refusal}");
        assert!(
            Scope::default()
                .refusal("file.write", "/abs/target/x")
                .is_none(),
            "a scope nobody wrote restricts nothing, absolute or not"
        );
    }

    #[test]
    fn leaving_the_workspace_is_the_providers_refusal_and_not_the_scopes() {
        // A kept `..` matches no workspace-relative glob. The path is still refused, by the check
        // that can see the filesystem and knows where it lands.
        assert!(store().refusal("file.write", "../outside").is_none());
    }

    #[test]
    fn a_leading_double_star_names_zero_directories_as_well_as_many() {
        assert!(glob_matches("**/*.md", "README.md"));
        assert!(glob_matches("**/*.md", "docs/a.md"));
        assert!(glob_matches("**/*.md", "a/b/c.md"));
        assert!(!glob_matches("**/*.md", "README.txt"));
        // A trailing `**` is unchanged: `docs/**` is what is *under* `docs`, not `docs` itself.
        assert!(glob_matches("docs/**", "docs/x"));
        assert!(!glob_matches("docs/**", "docs"));
    }

    #[test]
    fn a_read_is_never_the_scopes_business() {
        // The scope is about writing. Narrowing what a run may *read* is a different decision with
        // a different argument, and taking it here by accident would be the wrong way to make it.
        assert!(store().refusal("file.read", "target/debug/x").is_none());
        assert!(store().refusal("shell", "target/debug/x").is_none());
    }
}
