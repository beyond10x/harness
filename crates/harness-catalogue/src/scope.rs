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

    /// The word a declaration uses, which [`parse`](Self::parse) reads back.
    fn word(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::PartialOnly => "partial-only",
            Self::Denied => "denied",
        }
    }

    /// Whether `operation` is refused on a path this decides: `partial-only` refuses a whole
    /// write and admits an edit.
    fn refuses(self, operation: &str) -> bool {
        match self {
            Self::Allowed => false,
            Self::PartialOnly => operation != "file.edit",
            Self::Denied => true,
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
    /// The rule as a declaration spells it, `<glob>=<word>`, which a refusal names.
    pub(crate) fn declared(&self) -> String {
        format!("{}={}", self.paths, self.write.word())
    }

    /// Reads `<glob>=<allowed|partial-only|denied>`.
    ///
    /// # Errors
    ///
    /// Names the half that is wrong. The separator is the last `=`, so a glob may contain one.
    ///
    /// A glob that is absolute, or that climbs above the workspace root, is refused here: the
    /// paths a rule is matched against are workspace-relative, so such a rule could never fire,
    /// and a rule that silently matches nothing is a boundary nobody knows is missing.
    pub fn parse(declaration: &str) -> Result<Self, String> {
        let (paths, word) = declaration.rsplit_once('=').ok_or_else(|| {
            format!("`{declaration}` is not `<glob>=<allowed|partial-only|denied>`")
        })?;
        let paths = paths.trim();
        if paths.is_empty() {
            return Err(format!("`{declaration}` names no path"));
        }
        if paths.starts_with('/') {
            return Err(format!(
                "`{declaration}` is absolute, and a scope's paths are relative to the workspace \
                 root; spell the rule relative to it"
            ));
        }
        let normalised = normalise(paths);
        if normalised.is_empty() || normalised == ".." || normalised.starts_with("../") {
            return Err(format!(
                "`{declaration}` names nothing inside the workspace, so it could never match a \
                 path this run may write"
            ));
        }
        Ok(Self {
            paths: paths.to_owned(),
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
    /// because otherwise `./target/x` walks past a rule that refuses `target/x`. The rule's own
    /// glob is normalised the same way, so `./target/**` refuses what `target/**` refuses — a rule
    /// spelled with a `./` used to match nothing at all, silently. The message still names the
    /// path as it was written — that is what the caller has to change.
    ///
    /// Lexical only. A link inside the workspace is a second spelling this cannot see; the
    /// catalogue asks the provider where the path lands and puts that spelling through here too.
    ///
    /// A spelling that can only name a directory — the workspace root however it is written
    /// (`.`, `src/..`), or one that ends in `/`, `.` or `..` — is judged as
    /// [`refusal_of_directory`](Self::refusal_of_directory) judges a directory. Any other spelling
    /// is judged as a file, by its own name: whether a bare name like `protected` is a directory
    /// is a question about the filesystem this cannot ask, so a caller that can see one asks
    /// [`refusal_of_directory`](Self::refusal_of_directory) instead.
    #[must_use]
    pub fn refusal(&self, operation: &str, path: &str) -> Option<String> {
        self.judged(operation, path, names_a_directory(path))
    }

    /// Why this operation may not write through `path`, a directory, or [`None`] if it may.
    ///
    /// # A directory is judged by what it can hold
    ///
    /// A program handed a directory may change any file beneath it — a formatter given one
    /// rewrites every source it holds — and a rule like `protected/**=denied` refuses each of those
    /// files by name while matching the directory's own name not at all. So a directory is refused
    /// when a rule that refuses this operation could match a path beneath it, and the refusal
    /// names the directory and that rule.
    ///
    /// "Could match" follows the rule's glob, `*` within a segment and `**` across them:
    /// `docs/*/generated/**` reaches beneath `docs/a` and never beneath `docs/a/other`, and
    /// `**/*.pem` reaches beneath every directory. A directory that climbs out of the workspace
    /// may hold all of it, and the scope cannot tell, so every rule reaches beneath it.
    ///
    /// # An `allowed` rule re-admits only what it decides whole
    ///
    /// First match wins here as it does for a file. A rule ahead of the refusing one that matches
    /// **every** path beneath the directory — `**`, or `<the directory or one above it>/**` —
    /// decides all of them, and the refusing rule is never reached from there: `public/**=allowed`
    /// ahead of `**/*.pem=denied` admits `public` and still refuses `src`. A rule that decides
    /// only some of what the directory holds, such as `protected/*.txt=allowed` ahead of
    /// `protected/**=denied`, re-admits nothing, because the rest still reaches the deny.
    /// That is conservative where several partial rules together would decide the whole
    /// directory: refusing a write that would have been admitted costs a turn, and the reverse
    /// costs a file.
    ///
    /// The directory's own name is judged first, by the rules a file's is. A directory none of
    /// whose possible contents a rule refuses, and whose own name no rule refuses, is admitted.
    ///
    /// # Every refusal says what to name instead
    ///
    /// Whichever rule refused, by the directory's own name or by what it can hold, the refusal
    /// names the directory and that rule and tells the caller to name the subdirectories or files
    /// to format or write instead. A directory refused for one corner of it is usually most of a
    /// legitimate write, and a refusal that only said no would be retried whole.
    ///
    /// Lexical, like [`refusal`](Self::refusal): a link beneath the directory is a second
    /// spelling this cannot see, and the catalogue walks the directory for those.
    #[must_use]
    pub fn refusal_of_directory(&self, operation: &str, path: &str) -> Option<String> {
        self.judged(operation, path, true)
    }

    /// The first rule that refuses `operation` somewhere, or [`None`] where every rule admits it:
    /// a scope that refuses this write nowhere has nothing a walk beneath a directory could find.
    pub(crate) fn first_refusing(&self, operation: &str) -> Option<&ScopeRule> {
        self.0.iter().find(|rule| rule.write.refuses(operation))
    }

    /// The rule that refuses `operation` on `path`, a workspace-relative file judged by its own
    /// name, or [`None`] where the scope admits it.
    pub(crate) fn refusing_rule(&self, operation: &str, path: &str) -> Option<&ScopeRule> {
        let normalised = normalise(path);
        self.0
            .iter()
            .find(|rule| glob_matches(&normalise(&rule.paths), &normalised))
            .filter(|rule| rule.write.refuses(operation))
    }

    /// [`refusal`](Self::refusal), and for a `directory` what it can hold as well.
    fn judged(&self, operation: &str, path: &str, directory: bool) -> Option<String> {
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
        if directory {
            return self
                .directory_rule(operation, &normalised)
                .map(|(rule, reach)| through(path, rule, reach));
        }
        let named = self
            .0
            .iter()
            .find(|rule| glob_matches(&normalise(&rule.paths), &normalised));
        named.and_then(|rule| match rule.write {
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
        })
    }

    /// The rule that refuses `operation` through `path`, a workspace-relative directory judged as
    /// [`refusal_of_directory`](Self::refusal_of_directory) judges one, or [`None`] where the
    /// scope admits it. What a walk asks of a directory a link leads to.
    pub(crate) fn refusing_rule_of_directory(
        &self,
        operation: &str,
        path: &str,
    ) -> Option<&ScopeRule> {
        self.directory_rule(operation, &normalise(path))
            .map(|(rule, _)| rule)
    }

    /// The rule that refuses `operation` through `directory`, normalised, and how it reaches it:
    /// the first rule matching the directory's own name when that one refuses, and otherwise the
    /// first refusing rule that can match beneath it before a rule ahead decides everything there.
    fn directory_rule(&self, operation: &str, directory: &str) -> Option<(&ScopeRule, Reach)> {
        let named = self
            .0
            .iter()
            .find(|rule| glob_matches(&normalise(&rule.paths), directory));
        if let Some(rule) = named.filter(|rule| rule.write.refuses(operation)) {
            return Some((rule, Reach::Itself));
        }
        for rule in &self.0 {
            let glob = normalise(&rule.paths);
            if !reaches_beneath(&glob, directory) {
                continue;
            }
            if rule.write.refuses(operation) {
                return Some((rule, Reach::Beneath));
            }
            if covers_beneath(&glob, directory) {
                return None;
            }
        }
        None
    }
}

/// What every refusal of a write through a directory tells the caller to do instead.
pub(crate) const NAME_INSTEAD: &str = "Name the subdirectories or files to format or write instead";

/// How a refusing rule reaches a written directory.
#[derive(Debug, Clone, Copy)]
enum Reach {
    /// It matches the directory's own name.
    Itself,
    /// It can match something the directory holds.
    Beneath,
}

/// The refusal of a write through `path`, a directory, that `rule` refuses: the directory, the
/// rule, and what to name instead.
fn through(path: &str, rule: &ScopeRule, reach: Reach) -> String {
    let declared = rule.declared();
    if rule.write == WriteScope::PartialOnly {
        let what = match reach {
            Reach::Itself => {
                format!("`{path}` is a directory that `{declared}` lets change only in part")
            }
            Reach::Beneath => format!(
                "`{path}` is a directory that can hold what `{declared}` lets change only in part"
            ),
        };
        format!(
            "{what}, and a write through a directory may replace any file beneath it whole. \
             {NAME_INSTEAD}, outside what that rule covers, or change those in part with \
             `file_edit`."
        )
    } else {
        let what = match reach {
            Reach::Itself => {
                format!("`{path}` is a directory that `{declared}` refuses by its own name")
            }
            Reach::Beneath => format!(
                "`{path}` is a directory that can hold what `{declared}` refuses, and a write \
                 through a directory may change any file beneath it"
            ),
        };
        format!(
            "{what}. {NAME_INSTEAD}, outside what that rule covers; if the work needs what it \
             covers, the run was scoped wrongly and a person should say so."
        )
    }
}

/// Whether `path` can only name a directory: the workspace root however it is spelled, or a
/// spelling that ends in `/`, `.` or `..`, none of which names a file.
fn names_a_directory(path: &str) -> bool {
    path.ends_with('/')
        || matches!(path.rsplit('/').next(), Some("." | ".."))
        || normalise(path).is_empty()
}

/// Whether a normalised path climbs out of the workspace root, where the scope cannot see.
fn climbs_out(path: &str) -> bool {
    path == ".." || path.starts_with("../")
}

/// Whether `pattern` matches some path beneath `directory`, both normalised: whether the rule can
/// decide a file that a write through the directory may change.
///
/// The glob is followed over `<directory>/` as [`glob_matches`] follows it over a whole path.
/// What is left of it once the directory is spent matches some name beneath, because every glob
/// matches the text its wildcards leave behind; nothing left matches only the directory itself.
/// A `**` reached before the directory is spent takes the rest of it and goes on beneath. Every
/// pattern reaches beneath the root, and beneath a directory that climbs out of the workspace.
fn reaches_beneath(pattern: &str, directory: &str) -> bool {
    /// `prefix` is what is left of `<directory>/`, so it ends at a `/` that `*` cannot cross.
    fn go(pattern: &[u8], prefix: &[u8]) -> bool {
        if prefix.is_empty() {
            return !pattern.is_empty();
        }
        match pattern.first() {
            None => false,
            Some(b'*') if pattern.get(1) == Some(&b'*') => true,
            Some(b'*') => (0..prefix.len())
                .take_while(|&taken| !prefix[..taken].contains(&b'/'))
                .any(|taken| go(&pattern[1..], &prefix[taken..])),
            Some(&expected) => prefix[0] == expected && go(&pattern[1..], &prefix[1..]),
        }
    }
    if climbs_out(directory) {
        return true;
    }
    if directory.is_empty() {
        return !pattern.is_empty();
    }
    go(pattern.as_bytes(), format!("{directory}/").as_bytes())
}

/// Whether `pattern` matches **every** path beneath `directory`, both normalised, so that no rule
/// after it is reached from there.
///
/// Two shapes are recognised: `**`, and `<head>/**` where `head` matches the directory or one
/// above it. Sound and not complete: a pattern of any other shape is taken to leave something
/// beneath undecided, which can only refuse more.
fn covers_beneath(pattern: &str, directory: &str) -> bool {
    if pattern == "**" {
        return true;
    }
    let Some(head) = pattern.strip_suffix("/**") else {
        return false;
    };
    if directory.is_empty() || climbs_out(directory) {
        return false;
    }
    directory
        .match_indices('/')
        .map(|(at, _)| at)
        .chain([directory.len()])
        .any(|at| glob_matches(head, &directory[..at]))
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
pub(crate) fn normalise(path: &str) -> String {
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
    fn a_rule_spelled_with_a_dot_slash_refuses_what_the_bare_spelling_refuses() {
        // Only the caller's path used to be normalised. `./target/**=denied` then matched nothing
        // — not `target/x`, not even `./target/x` — and nothing said so.
        let scope = Scope::of(vec![
            ScopeRule::parse("./target/**=denied").expect("reads"),
            ScopeRule::parse("docs//./generated/**=denied").expect("reads"),
        ]);
        assert!(scope.refusal("file.write", "target/x").is_some());
        assert!(scope.refusal("file.write", "./target/x").is_some());
        assert!(scope.refusal("file.write", "docs/generated/a.md").is_some());
        assert!(scope.refusal("file.write", "docs/other.md").is_none());
    }

    #[test]
    fn a_rule_that_could_never_match_a_workspace_path_is_refused_when_it_is_read() {
        // A rule that silently matches nothing is a boundary nobody knows is missing.
        let absolute = ScopeRule::parse("/abs/target/**=denied").expect_err("refused");
        assert!(absolute.contains("relative"), "{absolute}");
        assert!(ScopeRule::parse("../outside/**=denied").is_err());
        assert!(
            ScopeRule::parse("./=denied").is_err(),
            "normalises to nothing"
        );
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
        assert!(
            store()
                .refusal_of_directory("file.read", "target")
                .is_none()
        );
        assert!(
            Scope::default()
                .refusal_of_directory("file.write", ".")
                .is_none(),
            "a scope nobody wrote restricts nothing, a directory included"
        );
    }

    // --- a written directory -----------------------------------------------------------------

    fn protected() -> Scope {
        Scope::of(vec![
            ScopeRule::parse("protected/**=denied").expect("reads"),
        ])
    }

    #[test]
    fn a_directory_none_of_whose_contents_a_rule_refuses_is_admitted_as_before() {
        let scope = protected();
        assert!(scope.refusal_of_directory("file.write", "src").is_none());
        assert!(scope.refusal("file.write", "src/.").is_none());
        assert!(scope.refusal("file.write", "src/").is_none());
        // A sibling whose name only starts like the refused one holds nothing it names.
        assert!(
            scope
                .refusal_of_directory("file.write", "protected2")
                .is_none()
        );
        // A single `*` stays within its segment, so a rule over the root's own files reaches
        // beneath no directory but the root.
        let top = Scope::of(vec![ScopeRule::parse("*.md=denied").expect("reads")]);
        assert!(top.refusal_of_directory("file.write", "docs").is_none());
        assert!(top.refusal_of_directory("file.write", ".").is_some());
        // A wildcard inside a rule is followed, not widened: `docs/*/generated/**` reaches beneath
        // `docs/a` and never beneath `docs/a/other`.
        let generated = Scope::of(vec![
            ScopeRule::parse("docs/*/generated/**=denied").expect("reads"),
        ]);
        assert!(
            generated
                .refusal_of_directory("file.write", "docs/a/other")
                .is_none()
        );
        assert!(
            generated
                .refusal_of_directory("file.write", "docs/a")
                .is_some()
        );
    }

    #[test]
    fn a_directory_a_deny_rule_reaches_beneath_is_refused_naming_the_directory_and_the_rule() {
        // A program handed a directory may change every file beneath it, and `protected/**`
        // refuses each of those by name while matching the directory's own name not at all.
        let refusal = protected()
            .refusal_of_directory("file.write", "protected")
            .expect("refused");
        assert_eq!(
            refusal,
            "`protected` is a directory that can hold what `protected/**=denied` refuses, and a \
             write through a directory may change any file beneath it. Name the subdirectories or \
             files to format or write instead, outside what that rule covers; if the work needs \
             what it covers, the run was scoped wrongly and a person should say so."
        );
        assert!(
            protected()
                .refusal_of_directory("file.edit", "protected")
                .is_some()
        );
        assert!(
            protected()
                .refusal_of_directory("file.write", "protected/sub")
                .is_some()
        );
        // A rule over one file reaches past every directory above it.
        let one_file = Scope::of(vec![
            ScopeRule::parse("docs/generated/index.md=denied").expect("reads"),
        ]);
        let refusal = one_file
            .refusal_of_directory("file.write", "docs")
            .expect("refused");
        assert!(
            refusal.contains("`docs/generated/index.md=denied`"),
            "{refusal}"
        );
        // A rule that starts with `**/` reaches beneath every directory.
        let keys = Scope::of(vec![ScopeRule::parse("**/*.pem=denied").expect("reads")]);
        assert!(keys.refusal_of_directory("file.write", "src").is_some());
    }

    #[test]
    fn the_workspace_root_in_any_spelling_is_a_directory_holding_every_nested_rule() {
        // `.`, `protected/.` and their kin can only name a directory, so they are judged as one
        // without the caller having to say so.
        let nested = Scope::of(vec![ScopeRule::parse("a/b/c/**=denied").expect("reads")]);
        for root in [".", "", "./", "src/..", "./src/../."] {
            let refusal = nested.refusal("file.write", root).expect("refused");
            assert!(refusal.contains("`a/b/c/**=denied`"), "{root}: {refusal}");
        }
        assert!(nested.refusal_of_directory("file.write", ".").is_some());
        for spelling in ["protected/.", "protected/", "./protected/."] {
            let refusal = protected()
                .refusal("file.write", spelling)
                .expect("refused");
            assert!(refusal.contains(&format!("`{spelling}`")), "{refusal}");
        }
        // A directory above the workspace may hold all of it, and the scope cannot tell.
        assert!(nested.refusal("file.write", "..").is_some());
        assert!(
            nested
                .refusal_of_directory("file.write", "../elsewhere")
                .is_some()
        );
    }

    #[test]
    fn an_allowed_rule_ahead_of_a_deny_readmits_only_a_directory_it_covers_whole() {
        // First match wins, so `public/**=allowed` decides everything beneath `public` before
        // `**/*.pem=denied` is reached from there, and nothing else.
        let readmitted = Scope::of(vec![
            ScopeRule::parse("public/**=allowed").expect("reads"),
            ScopeRule::parse("**/*.pem=denied").expect("reads"),
        ]);
        assert!(
            readmitted
                .refusal_of_directory("file.write", "public")
                .is_none()
        );
        assert!(
            readmitted
                .refusal_of_directory("file.write", "public/sub")
                .is_none()
        );
        let refusal = readmitted
            .refusal_of_directory("file.write", "src")
            .expect("`src` can hold a key");
        assert!(refusal.contains("`**/*.pem=denied`"), "{refusal}");

        // Beneath a deny, the re-admitted directory's contents are admitted; its own name is still
        // a path the deny matches first, and is refused by it as before.
        let nested = Scope::of(vec![
            ScopeRule::parse("protected/public/**=allowed").expect("reads"),
            ScopeRule::parse("protected/**=denied").expect("reads"),
        ]);
        assert!(
            nested
                .refusal_of_directory("file.write", "protected/public/sub")
                .is_none()
        );
        assert!(nested.refusal("file.write", "protected/public").is_some());
        let refusal = nested
            .refusal_of_directory("file.write", "protected/public")
            .expect("refused by its own name");
        assert!(
            refusal.starts_with(
                "`protected/public` is a directory that `protected/**=denied` refuses by its own \
                 name. Name the subdirectories or files to format or write instead"
            ),
            "{refusal}"
        );
        let refusal = nested
            .refusal_of_directory("file.write", "protected")
            .expect("`protected` holds more than `public`");
        assert!(refusal.contains("`protected/**=denied`"), "{refusal}");

        // An `allowed` after the deny is never reached for a key: those files are the deny's.
        let too_late = Scope::of(vec![
            ScopeRule::parse("**/*.pem=denied").expect("reads"),
            ScopeRule::parse("public/**=allowed").expect("reads"),
        ]);
        assert!(too_late.refusal("file.write", "public").is_none());
        assert!(
            too_late
                .refusal_of_directory("file.write", "public")
                .is_some()
        );

        // One that admits only some of what the directory holds re-admits nothing.
        let some = Scope::of(vec![
            ScopeRule::parse("protected/*.txt=allowed").expect("reads"),
            ScopeRule::parse("protected/**=denied").expect("reads"),
        ]);
        assert!(
            some.refusal_of_directory("file.write", "protected")
                .is_some()
        );

        // A catch-all `allowed` first leaves no path for the deny to decide.
        let shadowed = Scope::of(vec![
            ScopeRule::parse("**=allowed").expect("reads"),
            ScopeRule::parse("protected/**=denied").expect("reads"),
        ]);
        assert!(
            shadowed
                .refusal_of_directory("file.write", "protected")
                .is_none()
        );
    }

    #[test]
    fn a_partial_only_rule_beneath_a_directory_refuses_a_whole_write_through_it_and_no_edit() {
        // A program handed a directory may replace any file beneath it whole, which is exactly
        // what `partial-only` refuses.
        let refusal = store()
            .refusal_of_directory("file.write", ".engineering")
            .expect("refused");
        assert_eq!(
            refusal,
            "`.engineering` is a directory that can hold what \
             `.engineering/planning/**=partial-only` lets change only in part, and a write through \
             a directory may replace any file beneath it whole. Name the subdirectories or files \
             to format or write instead, outside what that rule covers, or change those in part \
             with `file_edit`."
        );
        assert!(
            store()
                .refusal_of_directory("file.edit", ".engineering")
                .is_none()
        );
    }

    #[test]
    fn a_written_file_is_judged_by_its_own_spelling_exactly_as_before() {
        // A bare name is a file unless the caller knows it is a directory: `protected/**` names
        // what is beneath `protected`, not `protected` itself.
        assert!(protected().refusal("file.write", "protected").is_none());
        let refusal = protected()
            .refusal("file.write", "protected/kept.go")
            .expect("refused");
        assert_eq!(
            refusal,
            "`protected/kept.go` is outside what this run may change. Nothing here writes to it; \
             if the work needs that path, the run was scoped wrongly and a person should say so."
        );
        assert!(store().refusal("file.write", "target").is_none());
        assert!(store().refusal("file.write", "crates/x.rs").is_none());
        // The same name refused as a file keeps a file's words; refused as a directory, it says
        // what to name instead.
        let named = Scope::of(vec![ScopeRule::parse("protected=denied").expect("reads")]);
        assert_eq!(
            named.refusal("file.write", "protected").as_deref(),
            Some(
                "`protected` is outside what this run may change. Nothing here writes to it; if \
                 the work needs that path, the run was scoped wrongly and a person should say so."
            )
        );
        assert_eq!(
            named
                .refusal_of_directory("file.write", "protected")
                .as_deref(),
            Some(
                "`protected` is a directory that `protected=denied` refuses by its own name. Name \
                 the subdirectories or files to format or write instead, outside what that rule \
                 covers; if the work needs what it covers, the run was scoped wrongly and a person \
                 should say so."
            )
        );
    }
}
