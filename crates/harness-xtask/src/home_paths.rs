//! Index and worktree byte guard for literal POSIX home directories.
//!
//! Deliberately excludes history, Windows paths, shell expansion and assembled paths. Invalid
//! UTF-8 elsewhere in a blob never hides a match. A second view removes NULs to expose ASCII paths
//! embedded in UTF-16/32, preserving newlines. Unicode account names retain the legacy semantics.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

// Exact historical exemptions carried forward from the old checker. The journal and the story
// record already-published paths; rewriting either to satisfy this guard would forge history.
const HISTORICAL: [&str; 2] = [
    ".engineering/planning/journal.jsonl",
    ".engineering/planning/story/history-carries-a-home-directory.md",
];
// `aep plan store migrate git --verify` extracted this exact journal evidence into the revision-5
// store. Only these verified bytes inherit its historical exemption: neither a sibling record nor
// any alteration to this one is exempt. The digest is also the migration's content-addressed id.
const MIGRATED: &str = ".engineering/evidence/story/codex-live-refresh-measured/20260829T232745Z-000-792cc4befa2c.json";
const MIGRATED_SHA256: &str = "792cc4befa2c26b3534265d4d349995ee1239db61e4313929cbb56ab897953b2";

fn historical(path: &Path) -> bool {
    HISTORICAL.iter().any(|name| path == Path::new(name))
}

fn exempt(path: &Path, bytes: &[u8]) -> bool {
    historical(path) || (path == Path::new(MIGRATED) && sha256(bytes) == MIGRATED_SHA256)
}

fn sha256(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn account_of(candidate: &[u8]) -> String {
    let name: String = String::from_utf8_lossy(candidate)
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .collect();
    if name.chars().any(|c| c.is_alphanumeric() || c == '_') {
        name
    } else {
        String::new()
    }
}

fn paths_in(path: &Path, bytes: &[u8]) -> Vec<String> {
    if exempt(path, bytes) {
        return Vec::new();
    }
    let stripped: Vec<_> = bytes.iter().copied().filter(|b| *b != 0).collect();
    let mut findings = BTreeMap::new();
    for view in [bytes, stripped.as_slice()] {
        let mut line = 1;
        for (offset, byte) in view.iter().enumerate() {
            if *byte == b'\n' {
                line += 1;
            }
            if *byte != b'/' {
                continue;
            }
            for prefix in [b"/home/".as_slice(), b"/Users/".as_slice()] {
                if !view[offset..].starts_with(prefix) {
                    continue;
                }
                let candidate = &view[offset + prefix.len()..];
                let count = candidate
                    .iter()
                    .take_while(|b| {
                        b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | 128..=255)
                    })
                    .count();
                let account = account_of(&candidate[..count]);
                if account.is_empty() || account.trim_end_matches('.') == "you" {
                    continue;
                }
                let shown = format!("{}{account}", String::from_utf8_lossy(prefix));
                findings.insert(
                    (line, shown.clone()),
                    format!(
                        "{}:{line}: absolute home directory `{shown}`",
                        path.display()
                    ),
                );
            }
        }
    }
    findings.into_values().collect()
}

fn inspect(root: &Path, path: &Path) -> Vec<String> {
    if historical(path) {
        return Vec::new();
    }
    let full = root.join(path);
    let data = std::fs::symlink_metadata(&full).and_then(|metadata| {
        if metadata.is_symlink() {
            std::fs::read_link(&full).map(|target| target.as_os_str().as_encoded_bytes().to_vec())
        } else if metadata.is_dir() {
            Ok(Vec::new()) // submodule content belongs to its own repository
        } else {
            std::fs::read(&full)
        }
    });
    match data {
        Ok(bytes) => paths_in(path, &bytes),
        Err(error) => vec![format!(
            "{}: tracked but unreadable: {error}",
            path.display()
        )],
    }
}

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|error| format!("starting git {}: {error}", args.join(" ")))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

fn index_entries(root: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    let listing = git(root, &["ls-files", "-s", "-z"])?;
    let mut entries = Vec::new();
    for entry in listing.split(|byte| *byte == 0).filter(|e| !e.is_empty()) {
        let tab = entry
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or("git ls-files returned an entry without a path")?;
        let fields: Vec<_> = entry[..tab].split(|byte| *byte == b' ').collect();
        if fields.len() != 3 {
            return Err("git ls-files returned malformed index metadata".into());
        }
        if fields[0] == b"160000" {
            continue;
        }
        let blob = std::str::from_utf8(fields[1])
            .map_err(|e| e.to_string())?
            .to_owned();
        #[cfg(unix)]
        let name = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(entry[tab + 1..].to_vec())
        };
        #[cfg(not(unix))]
        let name = OsString::from(
            std::str::from_utf8(&entry[tab + 1..])
                .map_err(|_| "git returned a path not representable on this platform")?,
        );
        entries.push((PathBuf::from(name), blob));
    }
    Ok(entries)
}

fn staged_bytes(
    root: &Path,
    entries: &[(PathBuf, String)],
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let wanted: BTreeSet<_> = entries.iter().map(|(_, blob)| blob.as_str()).collect();
    if wanted.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut input = String::new();
    for blob in &wanted {
        let _ = writeln!(input, "{blob}");
    }
    let mut child = Command::new("git")
        .current_dir(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("starting git cat-file: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("git cat-file has no input pipe")?;
    // Drain output concurrently with requests: a large index must not deadlock on pipe capacity.
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let output = child.wait_with_output().map_err(|e| e.to_string());
    writer
        .join()
        .map_err(|_| "git cat-file input thread panicked")?
        .map_err(|e| format!("writing git cat-file input: {e}"))?;
    let output = output?;
    if !output.status.success() {
        return Err(format!(
            "git cat-file failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let mut rest = output.stdout.as_slice();
    let mut contents = BTreeMap::new();
    for blob in wanted {
        let end = rest
            .iter()
            .position(|b| *b == b'\n')
            .ok_or("truncated git cat-file header")?;
        let header = std::str::from_utf8(&rest[..end]).map_err(|e| e.to_string())?;
        let fields: Vec<_> = header.split(' ').collect();
        rest = &rest[end + 1..];
        if fields.len() == 2 && fields[1] == "missing" {
            continue;
        }
        if fields.len() != 3 || fields[0] != blob || fields[1] != "blob" {
            return Err("git cat-file returned an unexpected object".into());
        }
        let size: usize = fields[2]
            .parse()
            .map_err(|e| format!("invalid blob size: {e}"))?;
        if rest.get(size) != Some(&b'\n') {
            return Err("truncated git cat-file blob".into());
        }
        contents.insert(blob.to_owned(), rest[..size].to_vec());
        rest = &rest[size + 1..];
    }
    Ok(contents)
}

fn findings(root: &Path) -> Result<(usize, BTreeSet<String>), String> {
    let entries = index_entries(root)?;
    let contents = staged_bytes(root, &entries)?;
    let mut found = BTreeSet::new();
    for (path, blob) in &entries {
        if historical(path) {
            continue;
        }
        match contents.get(blob) {
            Some(bytes) => found.extend(paths_in(path, bytes)),
            None => {
                found.insert(format!(
                    "{}: staged as {blob}, which git cannot read",
                    path.display()
                ));
            }
        }
        match std::fs::symlink_metadata(root.join(path)) {
            // Deletions leave staged content authoritative; dangling symlinks are still inspected.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            _ => found.extend(inspect(root, path)),
        }
    }
    Ok((entries.len(), found))
}

pub fn check(root: &Path) -> Result<(), String> {
    let (count, found) = findings(root)?;
    if !found.is_empty() {
        return Err(format!(
            "{}\n{} absolute home director(ies) in tracked files",
            found.iter().cloned().collect::<Vec<_>>().join("\n"),
            found.len()
        ));
    }
    println!(
        "home paths: {count} tracked file(s) searched as bytes, staged and in the worktree, none absolute"
    );
    Ok(())
}

struct Cases(usize);
impl Cases {
    fn require(&mut self, label: &str, held: bool) -> Result<(), String> {
        self.0 += 1;
        if held {
            Ok(())
        } else {
            Err(format!("home paths self-test: {label}"))
        }
    }
}

fn put(root: &Path, name: &str, body: &[u8]) -> Result<(), String> {
    let path = root.join(name);
    std::fs::create_dir_all(path.parent().ok_or("fixture has no parent")?)
        .map_err(|e| e.to_string())?;
    std::fs::write(path, body).map_err(|e| e.to_string())
}

pub fn self_test() -> Result<(), String> {
    let mut cases = Cases(0);
    scanner_cases(&mut cases)?;
    repository_cases(&mut cases)?;
    println!("home paths: self-test green, {} case(s)", cases.0);
    Ok(())
}

fn scanner_cases(cases: &mut Cases) -> Result<(), String> {
    let path = Path::new("fixture.bin");
    // Assemble fixtures: the checker must not ship the very literal paths it prohibits.
    for root in ["home", "Users"] {
        let prefix = format!("/{root}/");
        for account in [
            "alice",
            "müller",
            "José",
            "张伟",
            "_account",
            "user",
            "username",
            "you-know-who",
            "username2",
            "users",
            "user2",
            "younger",
            "youtube",
        ] {
            let body = format!("one\ntwo\n{prefix}{account}");
            let found = paths_in(path, body.as_bytes());
            cases.require(
                "account named on its original line without trailing slash",
                found.len() == 1
                    && found[0]
                        == format!("fixture.bin:3: absolute home directory `{prefix}{account}`"),
            )?;
        }
        for suffix in [
            "you",
            "you/keys",
            "you.",
            "you’s machine",
            "…/keys",
            ".../keys",
            "“/keys",
            "—/keys",
        ] {
            cases.require(
                "placeholder and punctuation name nobody",
                paths_in(path, format!("{prefix}{suffix}").as_bytes()).is_empty(),
            )?;
        }
        for form in [
            format!("HOME={prefix}alice"),
            format!("home = \"{prefix}alice\""),
            format!("{prefix}alice”"),
        ] {
            cases.require(
                "assignment and prose still name the account",
                paths_in(path, form.as_bytes()).len() == 1,
            )?;
        }
        let literal = format!("{prefix}alice/work/key");
        let mut invalid = vec![0xed, 0xa0, 0x80, 0xff];
        invalid.extend(literal.as_bytes());
        invalid.extend([0, 0xfe]);
        cases.require(
            "undecodable binary fixture",
            std::str::from_utf8(&invalid).is_err(),
        )?;
        cases.require(
            "undecodable bytes do not hide paths",
            paths_in(path, &invalid).len() == 1,
        )?;
        for bytes in [
            literal
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
            literal.encode_utf16().flat_map(u16::to_be_bytes).collect(),
            literal
                .chars()
                .flat_map(|c| u32::from(c).to_le_bytes())
                .collect(),
            literal
                .chars()
                .flat_map(|c| u32::from(c).to_be_bytes())
                .collect(),
        ] {
            cases.require(
                "UTF-16/32 in both byte orders",
                paths_in(path, &bytes).len() == 1,
            )?;
        }
        let both = format!("{literal}\n{literal}\n");
        cases.require(
            "every line reported",
            paths_in(path, both.as_bytes()).len() == 2,
        )?;
        exemption_cases(cases, literal.as_bytes())?;
    }
    for body in [b"workspace = \".\"".as_slice(), b"/homeserver/alice/keys"] {
        cases.require("clean file", paths_in(path, body).is_empty())?;
    }
    Ok(())
}

fn exemption_cases(cases: &mut Cases, literal: &[u8]) -> Result<(), String> {
    for exempt_path in HISTORICAL {
        cases.require(
            "exact historical path exempt",
            paths_in(Path::new(exempt_path), literal).is_empty(),
        )?;
        for suffix in [".bak", ".orig", "2"] {
            cases.require(
                "historical sibling not exempt",
                paths_in(Path::new(&format!("{exempt_path}{suffix}")), literal).len() == 1,
            )?;
        }
    }
    cases.require(
        "migrated path alone grants no exemption",
        paths_in(Path::new(MIGRATED), literal).len() == 1,
    )
}

fn repository_cases(cases: &mut Cases) -> Result<(), String> {
    let scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
    let root = scratch.path();
    git(root, &["init", "--quiet"])?;
    let literal = format!("/{}/alice/work/key", "home");
    put(root, "f.txt", literal.as_bytes())?;
    put(root, "untracked.txt", literal.as_bytes())?;
    git(root, &["add", "f.txt"])?;
    let (count, found) = findings(root)?;
    cases.require(
        "index/worktree deduplicate and untracked excluded",
        count == 1 && found.len() == 1 && found.iter().all(|f| f.starts_with("f.txt:1:")),
    )?;
    cases.require(
        "checker entry point refuses and names file",
        check(root).is_err_and(|e| e.contains("f.txt:1:")),
    )?;
    put(root, "f.txt", b"clean\n")?;
    cases.require(
        "staged leak cannot be hidden by clean worktree",
        findings(root)?.1.len() == 1,
    )?;
    git(root, &["add", "f.txt"])?;
    cases.require(
        "clean staged and worktree content passes",
        check(root).is_ok(),
    )?;
    put(root, "f.txt", literal.as_bytes())?;
    cases.require(
        "worktree leak with clean index refused",
        findings(root)?.1.len() == 1,
    )?;
    git(root, &["add", "f.txt"])?;
    std::fs::remove_file(root.join("f.txt")).map_err(|e| e.to_string())?;
    cases.require(
        "deleted worktree judged by staged bytes",
        findings(root)?.1.len() == 1,
    )?;
    cases.require(
        "unreadable file refused by name",
        inspect(root, Path::new("missing.txt"))[0]
            .starts_with("missing.txt: tracked but unreadable:"),
    )?;
    #[cfg(unix)]
    for (index, target) in [
        format!("/{}/alice", "home"),
        format!("/{}/alice", "Users"),
        literal,
    ]
    .iter()
    .enumerate()
    {
        let name = format!("link-{index}");
        std::os::unix::fs::symlink(target, root.join(&name)).map_err(|e| e.to_string())?;
        let found = inspect(root, Path::new(&name));
        cases.require(
            "symlink target refused as path rather than unreadable",
            found.len() == 1 && found[0].contains("absolute home directory"),
        )?;
        git(root, &["add", &name])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn checker_rejects_planted_defects() {
        super::self_test().expect("home path guard self-test");
    }

    #[test]
    fn migrated_history_exemption_is_bound_to_exact_bytes() {
        let bytes = include_bytes!(
            "../../../.engineering/evidence/story/codex-live-refresh-measured/20260829T232745Z-000-792cc4befa2c.json"
        );
        assert!(super::exempt(std::path::Path::new(super::MIGRATED), bytes));
        let mut changed = bytes.to_vec();
        changed.push(b' ');
        assert!(!super::exempt(
            std::path::Path::new(super::MIGRATED),
            &changed
        ));
        assert!(!super::exempt(
            std::path::Path::new(&format!("{}.bak", super::MIGRATED)),
            bytes
        ));
    }
}
