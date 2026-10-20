//! Whether a write may go through a path, under the run's scope and the step's.
//!
//! # A written directory is judged by what it holds, by name and on disk
//!
//! [`Scope`] judges spellings, and a directory by every name its rules can match beneath it
//! ([`Scope::refusal_of_directory`]). A link beneath the directory is a spelling of somewhere
//! else that no reading of the directory's name can see, and a program handed the directory
//! writes through it: `gofmt -w src` rewrote `protected/kept.go` through `src/alias.go` while
//! the scope refused that file by name. So a written directory the scope admits by name is
//! walked on disk, links not followed by the listing, and every link met is judged where it
//! leads, as the program would follow it:
//!
//! - The walk is made only when a layer refuses this write somewhere. A scope that refuses
//!   nothing has nothing a walk could find, so a run that restricts nothing pays nothing.
//! - A link that lands within a directory already walked, or waiting to be, is covered by that
//!   walk. That is what ends a cycle: every directory is walked once, by its canonical path.
//! - One that lands on a file is judged as a written file, where it lands.
//! - One that lands on another directory inside the workspace is judged by name as the written
//!   directory is, and then walked like it.
//! - One that leads nowhere is judged where a write through it would create its target, resolved
//!   against the link's own directory.
//! - One that leaves the workspace, or cannot be resolved, is refused.
//! - More than [`MAX_ENTRIES`] entries in all, or a directory more than [`MAX_DEPTH`] directories
//!   below the written one, is refused rather than judged in part, and so is an entry that cannot
//!   be read.
//!
//! Every refusal names the directory, the link and the rule (or the bound), and tells the caller
//! to name the subdirectories or files instead. A link is named by the route the program would
//! take to it, through the written directory and any link before it.
//!
//! Judged when the call is made: a link created between the judgement and the write is not seen.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use crate::Scope;
use crate::scope::{NAME_INSTEAD, normalise};

/// How many entries a walk lists, across every directory it walks, before the write is refused
/// instead.
///
/// A written directory may hold a dependency tree. The `node_modules` trees on the host this was
/// measured on hold 46,000 to 48,600 entries each, so this leaves four times the largest, and
/// listing this many takes about 0.15 s (0.7 µs an entry, measured with `find`).
pub(crate) const MAX_ENTRIES: usize = 200_000;

/// How many directories below the written one a walk descends before the write is refused
/// instead.
///
/// The deepest of those `node_modules` trees nests 12 directories below the directory that holds
/// it, and a package manager nests a conflicting version of a package two levels further each
/// time it has to; this leaves room for ten such levels.
pub(crate) const MAX_DEPTH: usize = 32;

/// How a walk beneath a written directory is bounded.
#[derive(Debug, Clone, Copy)]
struct Bounds {
    entries: usize,
    depth: usize,
}

/// Why `operation` may not write `path` under the run's layer or the step's, the run's answer
/// first, or [`None`] if both admit it. A file write, a file edit and every written path of a
/// toolchain call are asked here, as spelled and where they land.
///
/// A path that is a directory in `tree`, links followed, is judged by what it can hold
/// ([`Scope::refusal_of_directory`]) and then walked for links (the module's own description).
/// Any other path is judged as [`Scope::refusal`] judges it, which is exactly as before for a
/// file and still treats a spelling that can only name a directory as one. With no `tree` to look
/// in, a bare name is judged as a file and nothing is walked.
pub(crate) fn refusal(
    layers: [&Scope; 2],
    tree: Option<&Path>,
    operation: &str,
    path: &str,
) -> Option<String> {
    refusal_within(
        layers,
        tree,
        operation,
        path,
        Bounds {
            entries: MAX_ENTRIES,
            depth: MAX_DEPTH,
        },
    )
}

fn refusal_within(
    layers: [&Scope; 2],
    tree: Option<&Path>,
    operation: &str,
    path: &str,
    bounds: Bounds,
) -> Option<String> {
    // A read is never the scope's business and a scope nobody wrote restricts nothing; neither
    // needs the filesystem asked.
    if !matches!(operation, "file.write" | "file.edit")
        || layers.iter().all(|layer| layer.is_empty())
    {
        return None;
    }
    let tree = tree.filter(|tree| !path.starts_with('/') && tree.join(path).is_dir());
    let judged = layers.into_iter().find_map(|layer| {
        if tree.is_some() {
            layer.refusal_of_directory(operation, path)
        } else {
            layer.refusal(operation, path)
        }
    });
    if judged.is_some() {
        return judged;
    }
    let tree = tree?;
    let rule = layers
        .into_iter()
        .find_map(|layer| layer.first_refusing(operation))?
        .declared();
    match Walk::of(layers, operation, path, rule, bounds, tree) {
        Ok(walk) => walk.run().err(),
        Err(refusal) => Some(refusal),
    }
}

/// One directory a walk is still to list.
struct Listing {
    /// Where it is, canonical.
    at: PathBuf,
    /// The route a program handed the written directory takes to it.
    route: String,
    /// How many directories below the written one it is.
    depth: usize,
}

/// One walk beneath one written directory.
struct Walk<'a> {
    layers: [&'a Scope; 2],
    operation: &'a str,
    /// The written path as spelled, which every refusal names.
    path: &'a str,
    /// The first rule that refuses this write somewhere: the reason the walk is made.
    rule: String,
    bounds: Bounds,
    /// The workspace root, canonical.
    root: PathBuf,
    /// Every directory walked or waiting to be, canonical: the written one and each one a link
    /// led to. A landing within one of them is covered by its walk.
    walked: Vec<PathBuf>,
    /// The directories still to list.
    pending: Vec<Listing>,
    /// Entries listed so far, across every directory.
    seen: usize,
}

impl<'a> Walk<'a> {
    /// A walk of `path` under `tree`, or the refusal for a directory that cannot be resolved
    /// inside the workspace.
    fn of(
        layers: [&'a Scope; 2],
        operation: &'a str,
        path: &'a str,
        rule: String,
        bounds: Bounds,
        tree: &Path,
    ) -> Result<Self, String> {
        let mut walk = Self {
            layers,
            operation,
            path,
            rule,
            bounds,
            root: PathBuf::new(),
            walked: Vec::new(),
            pending: Vec::new(),
            seen: 0,
        };
        let resolved = tree
            .canonicalize()
            .and_then(|root| Ok((tree.join(path).canonicalize()?, root)));
        match resolved {
            Ok((directory, root)) if directory.starts_with(&root) => {
                walk.root = root;
                walk.walked.push(directory.clone());
                walk.pending.push(Listing {
                    at: directory,
                    route: normalise(path),
                    depth: 0,
                });
                Ok(walk)
            }
            Ok(_) => {
                Err(walk.whole("is a directory outside the workspace, where no rule can be judged"))
            }
            Err(error) => Err(walk.whole(&format!(
                "is a directory that cannot be resolved in the workspace ({error})"
            ))),
        }
    }

    /// Lists every directory until none is left, or refuses at the first thing refused.
    fn run(mut self) -> Result<(), String> {
        while let Some(listing) = self.pending.pop() {
            self.list(&listing)?;
        }
        Ok(())
    }

    /// Every entry of one directory: a link judged where it leads, a directory kept to list.
    fn list(&mut self, listing: &Listing) -> Result<(), String> {
        let entries =
            fs::read_dir(&listing.at).map_err(|error| self.unreadable(&listing.route, &error))?;
        let depth = listing.depth + 1;
        for entry in entries {
            let entry = entry.map_err(|error| self.unreadable(&listing.route, &error))?;
            self.seen += 1;
            if self.seen > self.bounds.entries {
                return Err(format!(
                    "`{}` is a directory holding more than {} entries, more than a write through a \
                     directory is judged over while `{}` refuses writes, so it is refused rather \
                     than judged in part. {NAME_INSTEAD}.",
                    self.path, self.bounds.entries, self.rule
                ));
            }
            let route = beneath(&listing.route, &entry.file_name());
            let kind = entry
                .file_type()
                .map_err(|error| self.unreadable(&route, &error))?;
            if kind.is_symlink() {
                self.link(&entry.path(), &route, depth)?;
            } else if kind.is_dir() {
                self.keep(entry.path(), route, depth)?;
            }
        }
        Ok(())
    }

    /// Keeps a directory `depth` below the written one to list, or refuses it past the bound.
    fn keep(&mut self, at: PathBuf, route: String, depth: usize) -> Result<(), String> {
        if depth > self.bounds.depth {
            return Err(format!(
                "`{}` is a directory nested more than {} directories deep, deeper than a write \
                 through a directory is judged while `{}` refuses writes, so it is refused rather \
                 than judged in part. {NAME_INSTEAD}.",
                self.path, self.bounds.depth, self.rule
            ));
        }
        self.pending.push(Listing { at, route, depth });
        Ok(())
    }

    /// Judges the link at `link`, reached by `route`, where it leads, or where a write through it
    /// would create its target when it leads nowhere.
    fn link(&mut self, link: &Path, route: &str, depth: usize) -> Result<(), String> {
        if let Ok(landing) = link.canonicalize() {
            return self.landed(route, &landing, depth);
        }
        let target = fs::read_link(link).map_err(|error| {
            self.unjudged(
                route,
                &format!("leads nowhere that can be judged ({error})"),
            )
        })?;
        let parent = link.parent().unwrap_or(&self.root);
        let landing = would_land(&parent.join(target)).map_err(|reason| {
            self.unjudged(
                route,
                &format!("leads nowhere that can be judged ({reason})"),
            )
        })?;
        self.landed(route, &landing, depth)
    }

    /// Judges `landing`, absolute, where the link reached by `route` leads.
    fn landed(&mut self, route: &str, landing: &Path, depth: usize) -> Result<(), String> {
        if self.walked.iter().any(|walked| landing.starts_with(walked)) {
            return Ok(());
        }
        if !landing.starts_with(&self.root) {
            return Err(self.unjudged(
                route,
                "leads outside the workspace, where no rule can be judged",
            ));
        }
        let shown = self.shown(landing);
        if landing.is_dir() {
            if let Some(rule) = self
                .layers
                .into_iter()
                .find_map(|layer| layer.refusing_rule_of_directory(self.operation, &shown))
            {
                return Err(format!(
                    "`{}` is a directory, and the link `{route}` beneath it leads to the directory \
                     `{shown}`, where `{}` refuses this write; a write through the directory \
                     reaches what that directory holds through the link. {NAME_INSTEAD}, none of \
                     which holds that link.",
                    self.path,
                    rule.declared()
                ));
            }
            self.walked.push(landing.to_path_buf());
            return self.keep(landing.to_path_buf(), route.to_owned(), depth);
        }
        match self
            .layers
            .into_iter()
            .find_map(|layer| layer.refusing_rule(self.operation, &shown))
        {
            None => Ok(()),
            Some(rule) => Err(format!(
                "`{}` is a directory, and the link `{route}` beneath it leads to `{shown}`, where \
                 `{}` refuses this write; a write through the directory reaches that file through \
                 the link. {NAME_INSTEAD}, none of which holds that link.",
                self.path,
                rule.declared()
            )),
        }
    }

    /// A refusal for the link reached by `route`, which `why` cannot be judged.
    fn unjudged(&self, route: &str, why: &str) -> String {
        format!(
            "`{}` is a directory, and the link `{route}` beneath it {why}; while `{}` refuses \
             writes, a write through a directory is admitted only when every link beneath it is \
             judged. {NAME_INSTEAD}, none of which holds that link.",
            self.path, self.rule
        )
    }

    /// A refusal for an entry, reached by `route`, that cannot be read.
    fn unreadable(&self, route: &str, error: &std::io::Error) -> String {
        format!(
            "`{}` is a directory, and `{route}` beneath it cannot be read ({error}); while `{}` \
             refuses writes, a write through a directory is admitted only when everything beneath \
             it is judged. {NAME_INSTEAD}.",
            self.path, self.rule
        )
    }

    /// A refusal for the written directory as a whole, which `why` describes.
    fn whole(&self, why: &str) -> String {
        format!(
            "`{}` {why}; while `{}` refuses writes, a write through a directory is admitted only \
             when everything beneath it is judged. {NAME_INSTEAD}.",
            self.path, self.rule
        )
    }

    /// `path`, absolute, as the workspace spells it, `.` for the root itself.
    fn shown(&self, path: &Path) -> String {
        match path.strip_prefix(&self.root) {
            Ok(relative) if relative.as_os_str().is_empty() => ".".to_owned(),
            Ok(relative) => relative.to_string_lossy().into_owned(),
            Err(_) => path.display().to_string(),
        }
    }
}

/// The route to `name` inside the directory reached by `route`.
fn beneath(route: &str, name: &OsStr) -> String {
    let name = name.to_string_lossy();
    if route.is_empty() {
        name.into_owned()
    } else {
        format!("{route}/{name}")
    }
}

/// Where a write to `path`, absolute, would land: the deepest part of it that exists resolved,
/// links followed, and the part that does not exist yet put back on it.
///
/// # Errors
///
/// A part that exists cannot be resolved — a link that leads nowhere or round in a circle — or
/// the path climbs out of a directory that does not exist, which no reading of it may decide.
fn would_land(path: &Path) -> Result<PathBuf, String> {
    let mut tail = Vec::new();
    let mut existing = path;
    while existing.symlink_metadata().is_err() {
        let (Some(name), Some(parent)) = (existing.file_name(), existing.parent()) else {
            return Err("it climbs out of a directory that does not exist".to_owned());
        };
        tail.push(name);
        existing = parent;
    }
    let mut landing = existing.canonicalize().map_err(|error| error.to_string())?;
    for name in tail.iter().rev() {
        landing.push(name);
    }
    Ok(landing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScopeRule;

    const INSTEAD: &str = "Name the subdirectories or files to format or write instead";

    fn scope(rule: &str) -> Scope {
        Scope::of(vec![ScopeRule::parse(rule).expect("a rule")])
    }

    /// `src` holding `files` files and nothing else.
    fn holding(files: usize) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a workspace");
        std::fs::create_dir(dir.path().join("src")).expect("src");
        for file in 0..files {
            std::fs::write(dir.path().join(format!("src/{file}.go")), "").expect("a file");
        }
        dir
    }

    const TIGHT: Bounds = Bounds {
        entries: 3,
        depth: 2,
    };

    #[test]
    fn a_directory_holding_more_entries_than_the_bound_is_refused_and_names_it() {
        let dir = holding(4);
        let refusal = refusal_within(
            [&scope("protected/**=denied"), &Scope::default()],
            Some(dir.path()),
            "file.write",
            "src",
            TIGHT,
        )
        .expect("refused by the bound");
        assert_eq!(
            refusal,
            format!(
                "`src` is a directory holding more than 3 entries, more than a write through a \
                 directory is judged over while `protected/**=denied` refuses writes, so it is \
                 refused rather than judged in part. {INSTEAD}."
            )
        );
        let within = holding(3);
        assert!(
            refusal_within(
                [&scope("protected/**=denied"), &Scope::default()],
                Some(within.path()),
                "file.write",
                "src",
                TIGHT,
            )
            .is_none(),
            "three entries are within a bound of three"
        );
    }

    #[test]
    fn a_directory_nested_deeper_than_the_bound_is_refused_and_names_it() {
        let dir = tempfile::tempdir().expect("a workspace");
        std::fs::create_dir_all(dir.path().join("src/a/b/c")).expect("three levels");
        let refusal = refusal_within(
            [&scope("protected/**=denied"), &Scope::default()],
            Some(dir.path()),
            "file.write",
            "src",
            Bounds {
                entries: 100,
                depth: 2,
            },
        )
        .expect("refused by the bound");
        assert_eq!(
            refusal,
            format!(
                "`src` is a directory nested more than 2 directories deep, deeper than a write \
                 through a directory is judged while `protected/**=denied` refuses writes, so it \
                 is refused rather than judged in part. {INSTEAD}."
            )
        );
        assert!(
            refusal_within(
                [&scope("protected/**=denied"), &Scope::default()],
                Some(dir.path()),
                "file.write",
                "src/a",
                Bounds {
                    entries: 100,
                    depth: 2,
                },
            )
            .is_none(),
            "two levels beneath `src/a` are within a bound of two"
        );
    }

    #[test]
    fn nothing_is_walked_unless_a_layer_refuses_this_write_somewhere() {
        // A bound of no entries refuses any walk at all, so an admission here is a walk that
        // never happened.
        let dir = holding(1);
        let none = Bounds {
            entries: 0,
            depth: 0,
        };
        let allowed = scope("**=allowed");
        assert!(
            refusal_within(
                [&allowed, &Scope::default()],
                Some(dir.path()),
                "file.write",
                "src",
                none,
            )
            .is_none()
        );
        // `partial-only` refuses a whole write and no edit, so it walks for the one only.
        let partial = scope("docs/**=partial-only");
        assert!(
            refusal_within(
                [&Scope::default(), &partial],
                Some(dir.path()),
                "file.edit",
                "src",
                none,
            )
            .is_none()
        );
        let refusal = refusal_within(
            [&allowed, &partial],
            Some(dir.path()),
            "file.write",
            "src",
            none,
        )
        .expect("the step's layer refuses a whole write somewhere");
        assert!(refusal.contains("more than 0 entries"), "{refusal}");
        assert!(
            refusal.contains("`docs/**=partial-only`"),
            "the rule that made the walk happen: {refusal}"
        );
    }

    #[test]
    fn a_link_beneath_a_written_directory_that_leaves_the_workspace_is_refused() {
        let outside = tempfile::tempdir().expect("outside the workspace");
        std::fs::write(outside.path().join("x.go"), "").expect("a file outside");
        let dir = holding(0);
        std::os::unix::fs::symlink(outside.path().join("x.go"), dir.path().join("src/x.go"))
            .expect("a link out");
        let refusal = refusal_within(
            [&scope("protected/**=denied"), &Scope::default()],
            Some(dir.path()),
            "file.write",
            "src",
            TIGHT,
        )
        .expect("refused");
        assert_eq!(
            refusal,
            format!(
                "`src` is a directory, and the link `src/x.go` beneath it leads outside the \
                 workspace, where no rule can be judged; while `protected/**=denied` refuses \
                 writes, a write through a directory is admitted only when every link beneath it \
                 is judged. {INSTEAD}, none of which holds that link."
            )
        );
    }

    #[test]
    fn a_directory_beneath_that_cannot_be_read_is_refused_rather_than_skipped() {
        use std::os::unix::fs::PermissionsExt;
        let dir = holding(0);
        let locked = dir.path().join("src/locked");
        std::fs::create_dir(&locked).expect("a directory");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("locked");
        // A process that reads it anyway (root) has nothing to show here.
        let readable = std::fs::read_dir(&locked).is_ok();
        let refusal = refusal_within(
            [&scope("protected/**=denied"), &Scope::default()],
            Some(dir.path()),
            "file.write",
            "src",
            TIGHT,
        );
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
            .expect("unlocked, so the workspace can be removed");
        if readable {
            return;
        }
        let refusal = refusal.expect("refused");
        assert!(
            refusal
                .starts_with("`src` is a directory, and `src/locked` beneath it cannot be read ("),
            "{refusal}"
        );
        assert!(refusal.ends_with(&format!("{INSTEAD}.")), "{refusal}");
    }

    #[test]
    fn a_link_that_leads_nowhere_judgeable_is_refused() {
        // `gone/..` climbs out of a directory that does not exist, so where the target would be
        // is not something a lexical reading may decide.
        let dir = holding(0);
        std::os::unix::fs::symlink("gone/../../protected/x.go", dir.path().join("src/x.go"))
            .expect("a link through a missing directory");
        let refusal = refusal_within(
            [&scope("protected/**=denied"), &Scope::default()],
            Some(dir.path()),
            "file.write",
            "src",
            TIGHT,
        )
        .expect("refused");
        assert!(
            refusal.starts_with(
                "`src` is a directory, and the link `src/x.go` beneath it leads nowhere that can \
                 be judged ("
            ),
            "{refusal}"
        );
        assert!(
            refusal.ends_with(&format!("{INSTEAD}, none of which holds that link.")),
            "{refusal}"
        );
    }
}
