//! Reading a memory vault off a disk, from a directory an operator **named**.
//!
//! `skills.rs` beside this file loads what `harness-loop`'s `skill.rs` defines; this loads what its
//! `memory.rs` defines, for the same reason. A loop that walked a directory would be a loop whose
//! instructions depend on something no caller declared — and for a memory vault that is not a
//! style preference, it is the whole difference between this and the ambient memory file
//! `metaharness` refuses by hermetic rule. There is **no default path**, no `$XDG` fallback, no
//! walk up the tree looking for a `memories/` beside the workspace, and no environment variable.
//! `--memory-dir` or nothing, which is the shape `AGENTS.md`'s credential rule states once and
//! this file obeys: *the harness reads nothing it was not pointed at*.
//!
//! # `<dir>/<id>.md`, and the id is the file's own name
//!
//! Flat, one file per record, frontmatter then body. The id is the file stem rather than a
//! frontmatter key, because two records claiming one id has to be impossible to write rather than
//! merely refused: a directory cannot hold two `substrate-pin.md`, so the filesystem enforces what
//! would otherwise be a check.
//!
//! # The parser refuses rather than guessing, and refuses the whole vault
//!
//! Same small `key: value` reader as `skills.rs`, taking no dependency for it, and understanding
//! nothing else — no nested maps, no block scalars, no anchors, no multi-line values. A document
//! using any of them is **refused by name**. And one bad record refuses the whole directory: a
//! vault silently missing the record that mattered reads to the model exactly like a complete
//! vault, which is `AGENTS.md` invariant 8 one layer up. The reference design this shape is taken
//! from states the same rule as *do not tell the caller the memory does not exist*.
//!
//! # There is no writer here, and that is a decision rather than an omission
//!
//! `AGENTS.md`: *the shipped toolset is read-only; adding a tool that writes or executes is its
//! own change, with its own gate and its own entry in `STATUS.md` — never a flag on an existing
//! one.* A memory vault is exactly the thing that tempts a writer to arrive as a flag on the
//! reader. It does not arrive here. This module opens files `O_RDONLY` through `fs::read_to_string`
//! and creates nothing.

use std::fs;
use std::path::{Path, PathBuf};

use harness_loop::{Memory, MemoryKind, MemoryStatus, MemoryTrust};

/// The delimiter a frontmatter block opens and closes with.
const FENCE: &str = "---";

/// The extension a memory record is written with.
const EXTENSION: &str = "md";

/// Every memory under one directory, in id order.
///
/// Id order rather than readdir order, for the reason `skills.rs` sorts: readdir order is a
/// filesystem's, and two machines would otherwise put a run's memories in the instruction in
/// different orders — changing the bytes of every request and defeating a prompt cache for no
/// reason anybody chose.
///
/// # Errors
///
/// Names the directory that cannot be read, or the first record that cannot be. A directory that
/// exists and holds no record is **not** an error here: `--memory-dir` pointing at an empty vault
/// is answered by the caller, which knows whether the run needed one. A record that is there and
/// cannot be read **is** an error, and it refuses every record beside it.
pub fn memories_in(directory: &Path) -> Result<Vec<Memory>, String> {
    let entries = fs::read_dir(directory).map_err(|error| {
        format!(
            "reading the memory directory `{}`: {error}",
            directory.display()
        )
    })?;
    let mut documents: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "reading the memory directory `{}`: {error}",
                directory.display()
            )
        })?;
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == EXTENSION)
        {
            documents.push(path);
        }
    }
    documents.sort();
    documents.iter().map(|path| memory_at(path)).collect()
}

/// One memory, from its own document, named by the file's stem.
///
/// # Errors
///
/// Names the file and what is wrong with it: a stem that is not a usable id, no frontmatter, an
/// unterminated block, a key this build does not read, a missing or illegal `kind`, `trust` or
/// `status`, or a body over the result bound.
pub fn memory_at(document: &Path) -> Result<Memory, String> {
    let named = |message: String| format!("the memory record `{}`: {message}", document.display());
    let id = document
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| named("has no usable file name to take its id from".to_owned()))?
        .to_owned();

    let bytes = fs::metadata(document)
        .map_err(|error| format!("reading metadata for `{}`: {error}", document.display()))?
        .len();
    if bytes > harness_wire::MAX_TOOL_RESULT_BYTES as u64 {
        return Err(named(format!(
            "is {bytes} bytes, over the {} byte result bound; it was refused before being read",
            harness_wire::MAX_TOOL_RESULT_BYTES
        )));
    }
    let text = fs::read_to_string(document)
        .map_err(|error| format!("reading `{}`: {error}", document.display()))?;

    let rest = text
        .strip_prefix(FENCE)
        .and_then(|rest| rest.strip_prefix('\n'))
        .ok_or_else(|| named(format!("does not open with a `{FENCE}` frontmatter fence")))?;
    let end = rest
        .find(&format!("\n{FENCE}"))
        .ok_or_else(|| named(format!("opens a `{FENCE}` fence and never closes it")))?;
    let (frontmatter, body) = rest.split_at(end);
    let body = body
        .strip_prefix(&format!("\n{FENCE}"))
        .unwrap_or(body)
        .trim_start_matches('\n');

    let mut kind = None;
    let mut summary = None;
    let mut trust = None;
    let mut status = None;
    let mut supersedes = None;
    for line in frontmatter.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| named(format!("`{line}` is not `key: value`")))?;
        let value = value.trim();
        match key.trim() {
            "kind" => kind = Some(parse_kind(value).map_err(&named)?),
            "summary" => summary = Some(value.to_owned()),
            "trust" => trust = Some(parse_trust(value).map_err(&named)?),
            "status" => status = Some(parse_status(value).map_err(&named)?),
            "supersedes" => supersedes = Some(value.to_owned()),
            // **Refused, not ignored**, exactly as an unread skill frontmatter key is. A
            // `confidence:` or a `scope:` this build skipped would be a claim its writer made and
            // the run did not apply, with nothing saying so — and an `id:` is refused here too,
            // because the id is the file's name and a second one in the document would be two
            // answers to one question.
            other => {
                return Err(named(format!(
                    "declares `{other}`, which this build does not read. It reads `kind`, \
                     `summary`, `trust`, `status` and `supersedes`; the id is the file's own \
                     name. A key skipped here is a claim its writer made and this run would not \
                     have applied."
                )));
            }
        }
    }

    let kind =
        kind.ok_or_else(|| named(format!("declares no `kind`. It is one of: {}.", kinds())))?;
    let summary = summary.ok_or_else(|| {
        named(
            "declares no `summary`. The summary is the whole of what a run that never recalls \
             this memory is told about it, so a record without one cannot be chosen."
                .to_owned(),
        )
    })?;
    // **Required rather than defaulted, both of them.** A `trust` this build filled in for an
    // absent key would be this build asserting something about a document it was only reading,
    // and a `status` defaulted to `active` would silently reactivate a record whose writer forgot
    // to say. The one legal `trust` value makes the key look redundant; writing it is the writer
    // stating, once per record, that this is unreviewed context.
    let trust = trust.ok_or_else(|| {
        named(
            "declares no `trust`. Every vault record is `unreviewed`, and writing it is how a \
             record states that rather than this build assuming it."
                .to_owned(),
        )
    })?;
    let status = status.ok_or_else(|| {
        named("declares no `status`. It is one of: active, rejected, superseded.".to_owned())
    })?;

    Ok(Memory {
        id,
        kind,
        summary,
        trust,
        status,
        supersedes: supersedes.filter(|value| !value.is_empty()),
        body: body.to_owned(),
    })
}

/// Every legal kind, for a refusal that says what was allowed.
fn kinds() -> String {
    MemoryKind::every()
        .iter()
        .map(|kind| kind.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The kind this word names, or a refusal saying which words are legal.
fn parse_kind(value: &str) -> Result<MemoryKind, String> {
    MemoryKind::every()
        .iter()
        .find(|kind| kind.as_str() == value)
        .copied()
        .ok_or_else(|| {
            format!(
                "declares `kind: {value}`, which is not one of: {}. The set is closed on \
                 purpose — a record that cannot pick a kind has to say so, and a catch-all \
                 member would make every refusable record legal instead.",
                kinds()
            )
        })
}

/// The one legal trust, or a refusal saying why there is only one.
fn parse_trust(value: &str) -> Result<MemoryTrust, String> {
    if value == "unreviewed" {
        return Ok(MemoryTrust::Unreviewed);
    }
    Err(format!(
        "declares `trust: {value}`. `unreviewed` is the only legal value: a vault record is \
         unreviewed context by construction, and governed truth is promoted **out** of the vault \
         into a canonical artifact with its own review — never marked trusted in place."
    ))
}

/// The status this word names, or a refusal saying which words are legal.
fn parse_status(value: &str) -> Result<MemoryStatus, String> {
    match value {
        "active" => Ok(MemoryStatus::Active),
        "rejected" => Ok(MemoryStatus::Rejected),
        "superseded" => Ok(MemoryStatus::Superseded),
        other => Err(format!(
            "declares `status: {other}`, which is not one of: active, rejected, superseded. \
             Nothing is deleted; a status is flipped and the record stays."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let document = dir.join(format!("{name}.{EXTENSION}"));
        fs::write(&document, text).expect("a record");
        document
    }

    const GOOD: &str = "---\nkind: fact\nsummary: The gate is `cargo xtask gate`.\ntrust: \
                        unreviewed\nstatus: active\n---\n\nIt runs python3 too.\n";

    #[test]
    fn a_memory_is_its_frontmatter_the_body_after_the_fence_and_the_files_own_name() {
        let root = tempfile::tempdir().expect("a root");
        let document = write(root.path(), "gate-is-xtask", GOOD);
        let memory = memory_at(&document).expect("reads");
        assert_eq!(
            memory.id, "gate-is-xtask",
            "the id is the file's name, so two records of one id cannot be written at all"
        );
        assert_eq!(memory.kind, MemoryKind::Fact);
        assert_eq!(memory.summary, "The gate is `cargo xtask gate`.");
        assert_eq!(memory.trust, MemoryTrust::Unreviewed);
        assert_eq!(memory.status, MemoryStatus::Active);
        assert_eq!(memory.supersedes, None);
        assert_eq!(memory.body, "It runs python3 too.\n");
    }

    #[test]
    fn a_key_this_build_does_not_read_refuses_the_record_rather_than_being_skipped() {
        let root = tempfile::tempdir().expect("a root");
        let document = write(
            root.path(),
            "over-declared",
            "---\nkind: fact\nsummary: s\ntrust: unreviewed\nstatus: active\nconfidence: 0.9\n---\nb\n",
        );
        let error = memory_at(&document).expect_err("refused");
        assert!(error.contains("confidence"), "{error}");
        assert!(error.contains("does not read"), "{error}");
    }

    #[test]
    fn a_trust_other_than_unreviewed_is_refused_and_the_refusal_says_why_there_is_only_one() {
        let root = tempfile::tempdir().expect("a root");
        for claimed in ["reviewed", "trusted", "verified", "true"] {
            let document = write(
                root.path(),
                &format!("claims-{claimed}"),
                &format!("---\nkind: fact\nsummary: s\ntrust: {claimed}\nstatus: active\n---\nb\n"),
            );
            let error = memory_at(&document).expect_err("refused");
            assert!(error.contains("only legal value"), "{error}");
            assert!(error.contains("promoted"), "{error}");
        }
    }

    #[test]
    fn a_kind_outside_the_closed_set_is_refused_and_the_refusal_lists_the_set() {
        let root = tempfile::tempdir().expect("a root");
        // `note` and `context` are the two the reference design carries and this one dropped.
        for claimed in ["note", "context", "anything"] {
            let document = write(
                root.path(),
                &format!("kind-{claimed}"),
                &format!(
                    "---\nkind: {claimed}\nsummary: s\ntrust: unreviewed\nstatus: active\n---\nb\n"
                ),
            );
            let error = memory_at(&document).expect_err("refused");
            assert!(error.contains("decision, fact, handoff"), "{error}");
        }
    }

    #[test]
    fn a_record_that_states_no_trust_or_no_status_is_refused_rather_than_defaulted() {
        let root = tempfile::tempdir().expect("a root");
        let no_trust = write(
            root.path(),
            "no-trust",
            "---\nkind: fact\nsummary: s\nstatus: active\n---\nb\n",
        );
        assert!(
            memory_at(&no_trust)
                .expect_err("refused")
                .contains("declares no `trust`")
        );
        let no_status = write(
            root.path(),
            "no-status",
            "---\nkind: fact\nsummary: s\ntrust: unreviewed\n---\nb\n",
        );
        assert!(
            memory_at(&no_status)
                .expect_err("refused")
                .contains("declares no `status`"),
            "defaulting to active would silently reactivate a record whose writer forgot to say"
        );
    }

    #[test]
    fn one_unreadable_record_refuses_the_whole_directory_rather_than_yielding_the_rest() {
        // **The named refusal, not a smaller set.** A vault missing the record that mattered reads
        // to the model exactly like a complete vault — invariant 8 one layer up — so the caller
        // is told which document, and gets nothing.
        let root = tempfile::tempdir().expect("a root");
        write(root.path(), "aaa-good", GOOD);
        write(
            root.path(),
            "mmm-truncated",
            "---\nkind: fact\nsummary: s\ntrust: unreviewed\n",
        );
        write(root.path(), "zzz-good", GOOD);
        let error = memories_in(root.path()).expect_err("the directory is refused");
        assert!(error.contains("mmm-truncated"), "{error}");
        assert!(error.contains("never closes"), "{error}");
    }

    #[test]
    fn a_directory_is_read_in_id_order_and_not_in_the_filesystems() {
        let root = tempfile::tempdir().expect("a root");
        for name in ["zulu", "alpha", "mike"] {
            write(root.path(), name, GOOD);
        }
        let ids: Vec<String> = memories_in(root.path())
            .expect("reads")
            .into_iter()
            .map(|memory| memory.id)
            .collect();
        assert_eq!(ids, vec!["alpha", "mike", "zulu"]);
    }

    #[test]
    fn a_directory_holding_no_record_reads_as_none_rather_than_failing() {
        let root = tempfile::tempdir().expect("a root");
        fs::write(root.path().join("README.txt"), "not a record").expect("a file");
        assert!(memories_in(root.path()).expect("reads").is_empty());
    }

    #[test]
    fn a_superseding_record_names_what_it_replaces_and_the_replaced_one_stays() {
        let root = tempfile::tempdir().expect("a root");
        write(
            root.path(),
            "substrate-pin",
            "---\nkind: fact\nsummary: old\ntrust: unreviewed\nstatus: superseded\n---\nold\n",
        );
        write(
            root.path(),
            "substrate-pin-2",
            "---\nkind: fact\nsummary: new\ntrust: unreviewed\nstatus: active\nsupersedes: \
             substrate-pin\n---\nnew\n",
        );
        let loaded = memories_in(root.path()).expect("reads");
        assert_eq!(loaded.len(), 2, "nothing is deleted");
        // By id and not by position: `substrate-pin-2.md` sorts *before* `substrate-pin.md`,
        // because `-` is 0x2D and `.` is 0x2E. The order is still the stable one this function
        // promises; it is simply not the one a reader guesses.
        let replacement = loaded
            .iter()
            .find(|memory| memory.id == "substrate-pin-2")
            .expect("the replacement is there");
        assert_eq!(
            replacement.supersedes.as_deref(),
            Some("substrate-pin"),
            "the new record names the old, so writing a correction never touches the old file"
        );
        let replaced = loaded
            .iter()
            .find(|memory| memory.id == "substrate-pin")
            .expect("and so is what it replaced");
        assert_eq!(replaced.status, MemoryStatus::Superseded);
    }

    #[test]
    fn an_oversized_record_is_refused_before_its_body_is_loaded() {
        let root = tempfile::tempdir().expect("a root");
        let document = write(
            root.path(),
            "large",
            &format!(
                "---\nkind: fact\nsummary: s\ntrust: unreviewed\nstatus: active\n---\n{}",
                "x".repeat(harness_wire::MAX_TOOL_RESULT_BYTES)
            ),
        );
        let error = memory_at(&document).expect_err("oversized refuses");
        assert!(error.contains("result bound"), "{error}");
        assert!(error.contains("before being read"), "{error}");
    }
}
