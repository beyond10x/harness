//! Telling the model where it is.
//!
//! The standing instruction described the tools and nothing else, so a run began not knowing which
//! directory it was in, what machine it was on, what day it was, or which branch it was about to
//! edit. The measured consequence is a model that asks for `pwd`, guesses a repository layout, or
//! writes a dated note with the wrong date — each of them a billed round trip spent discovering
//! something the process already knew.
//!
//! Two things go in. **Where the run is**: workspace, platform, date, git state. And **what the
//! project asks of anyone working in it**: `AGENTS.md`, else `CLAUDE.md`.
//!
//! # What this deliberately does not do
//!
//! It **runs no `git`**. Spawning a subprocess to learn a branch name would put an unconfined
//! process on a path that is otherwise entirely file reads, on every run, before the model has
//! asked for anything. `.git/HEAD` is a text file with a documented shape and reading it is the
//! whole job.
//!
//! It **reads only the workspace root**. No walk upward to a parent repository, no home-directory
//! instruction file: the workspace is the boundary the tools enforce, and an instruction picked up
//! from outside it would be one the operator never pointed the run at.
//!
//! It takes `now` as an argument rather than reading the clock, so what it produces is testable.

use std::fmt::Write as _;
use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// How much of a project instruction file is carried into the standing instruction.
///
/// The instruction is re-sent on **every** turn of a stateless loop, so its size is paid once per
/// turn for the length of the run. 32 KiB is roughly 8k tokens: large enough for every `AGENTS.md`
/// in this organisation, small enough that a repository with a 500 KiB generated instruction file
/// cannot quietly double the cost of a run.
pub const MAX_INSTRUCTIONS_BYTES: u64 = 32 * 1024;

/// The instruction files looked for, in the order they win.
///
/// `AGENTS.md` first because it is the harness-neutral one: it is the file this organisation
/// writes for *any* agent, and `CLAUDE.md` is one vendor's name for the same thing. A repository
/// carrying both means the neutral file is the maintained one, and a harness that preferred the
/// vendor file would read the copy.
pub const INSTRUCTION_FILES: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];

/// Seconds in a day, for turning a timestamp into a date.
const SECONDS_PER_DAY: i64 = 86_400;

/// Where a run is, and what the project it is in asks of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    /// A fenced block naming the workspace, the platform, the date and the git state.
    pub block: String,
    /// The project instruction file this workspace carries, if it carries one.
    pub instructions: Option<ProjectInstructions>,
}

/// A project's own instruction file, as much of it as is carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectInstructions {
    /// The absolute path it was read from, so the model can re-read the whole file if it needs to.
    pub path: PathBuf,
    /// The text carried, whole unless `truncated_at` says otherwise.
    pub text: String,
    /// The file's **full** size in bytes, present only when `text` is shorter than that.
    ///
    /// Both figures are needed to say "first N of M bytes", and `text.len()` is already N — so
    /// what this carries is M, the size the text was cut down *from*. [`Environment::render`]
    /// states it in words: a bound that is not stated reads to the model exactly like a complete
    /// file (`AGENTS.md` invariant 8).
    pub truncated_at: Option<usize>,
}

impl Environment {
    /// The text a caller appends to the standing instruction.
    ///
    /// Markdown sections rather than prose, because this sits next to a catalogue rendered the
    /// same way and the model reads the whole thing as one document.
    pub fn render(&self) -> String {
        let mut text = format!("## Environment\n\n{}\n", self.block);
        if let Some(instructions) = &self.instructions {
            let _ = write!(
                text,
                "\n## Project instructions ({})\n\n",
                instructions.path.display()
            );
            if let Some(total) = instructions.truncated_at {
                let _ = writeln!(
                    text,
                    "Only the first {} of {total} bytes of this file are shown, cut at a line \
                     boundary. The rest is not part of this instruction; read the file if you \
                     need it.\n",
                    instructions.text.len()
                );
            }
            text.push_str(&instructions.text);
        }
        text
    }
}

/// Reads the workspace's surroundings.
///
/// Never fails: a workspace that cannot be canonicalised is reported by the path the caller gave,
/// and a file that exists but cannot be read is passed over. A run must not be refused because the
/// harness could not work out what day it is.
pub fn discover(workspace: &Path, now: SystemTime) -> Environment {
    let root = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    let mut block = String::from("```\n");
    let _ = writeln!(block, "workspace: {}", root.display());
    let _ = writeln!(
        block,
        "os: {} ({})",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let _ = writeln!(block, "date: {} (UTC)", utc_date(now));
    if let Some(line) = git_line(&root) {
        let _ = writeln!(block, "{line}");
    }
    block.push_str("```");
    Environment {
        block,
        instructions: project_instructions(&root),
    }
}

/// Today's date as `YYYY-MM-DD`, in UTC.
///
/// UTC and said so, rather than a local time this process cannot know is right: the container a
/// run happens in is usually on UTC while the person reading its output is not, and a date that is
/// silently one of the two is worse than a date that names its zone.
pub fn utc_date(now: SystemTime) -> String {
    let seconds = match now.duration_since(UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_secs()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_secs()).unwrap_or(i64::MAX),
    };
    let (year, month, day) = civil_from_days(seconds.div_euclid(SECONDS_PER_DAY));
    format!("{year:04}-{month:02}-{day:02}")
}

/// The civil date for a count of days since 1970-01-01, by Howard Hinnant's algorithm.
///
/// Written out rather than taken from a date crate: this is the only date arithmetic in the
/// repository, the algorithm is a dozen lines with a published proof, and a dependency added for
/// it would be a dependency in the dependency tree of a harness that handles credentials. It
/// shifts the year to start in March so that the leap day is the last day of the year and no month
/// table is needed.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    // Days from 0000-03-01 to 1970-01-01, so era arithmetic starts at a March.
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    // 146_097 days is exactly 400 years; the rest is an offset inside that era.
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    // The March-based month, 0 for March through 11 for February.
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// What `HEAD` says, when it says something this code understands.
enum Head {
    Branch(String),
    Detached(String),
}

/// The git line of the block, or nothing when the workspace is not a repository.
fn git_line(root: &Path) -> Option<String> {
    let marker = root.join(".git");
    let git_dir = if marker.is_dir() {
        Some(marker)
    } else if marker.is_file() {
        // A `.git` **file** is a linked worktree or a submodule: it points at the real git
        // directory instead of being one. Followed rather than ignored, because a worktree is
        // exactly where an agent-run change is most likely to be happening.
        gitdir_of(&marker, root)
    } else {
        return None;
    };
    Some(match git_dir.as_deref().and_then(head_of) {
        Some(Head::Branch(name)) => format!("git: branch {name}"),
        Some(Head::Detached(commit)) => format!("git: detached head at {commit}"),
        // A repository whose `HEAD` could not be read is still a repository, and saying so is what
        // stops the model concluding this is a plain directory it may edit freely.
        None => "git: repository, head unreadable".to_owned(),
    })
}

/// Follows the `gitdir:` line of a `.git` file to the directory it names.
fn gitdir_of(marker: &Path, root: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(marker).ok()?;
    let target = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("gitdir:"))?
        .trim();
    let path = PathBuf::from(target);
    // A worktree's pointer is usually absolute, but git accepts a relative one and resolves it
    // against the directory holding the `.git` file.
    Some(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}

/// Reads `HEAD` out of a git directory.
fn head_of(git_dir: &Path) -> Option<Head> {
    let text = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = text.trim();
    if let Some(reference) = head.strip_prefix("ref:") {
        let reference = reference.trim();
        let name = reference.strip_prefix("refs/heads/").unwrap_or(reference);
        if name.is_empty() {
            return None;
        }
        return Some(Head::Branch(name.to_owned()));
    }
    // A detached head is the object id itself. Twelve digits is what a person recognises and
    // enough to identify a commit in any repository this harness will see.
    if head.len() >= 12 && head.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Some(Head::Detached(head[..12].to_owned()));
    }
    None
}

/// The project instruction file this workspace carries, bounded and honest about the bound.
fn project_instructions(root: &Path) -> Option<ProjectInstructions> {
    for name in INSTRUCTION_FILES {
        let path = root.join(name);
        if !path.is_file() {
            continue;
        }
        let Ok(file) = File::open(&path) else {
            continue;
        };
        let Ok(metadata) = file.metadata() else {
            continue;
        };
        let total = metadata.len();
        let mut bytes = Vec::new();
        if file
            .take(MAX_INSTRUCTIONS_BYTES)
            .read_to_end(&mut bytes)
            .is_err()
        {
            continue;
        }
        let truncated_at = if total > MAX_INSTRUCTIONS_BYTES {
            // Cut at the last line break inside the bound: half a sentence of somebody's
            // instruction reads as an instruction, and a half-line of a code fence reads as a
            // fence that never closes.
            let cut = bytes
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map_or(bytes.len(), |index| index + 1);
            bytes.truncate(cut);
            Some(usize::try_from(total).unwrap_or(usize::MAX))
        } else {
            None
        };
        return Some(ProjectInstructions {
            path,
            // Lossy only for a file that is not UTF-8 at all: the cut above lands on a newline,
            // which is never inside a multi-byte character.
            text: String::from_utf8_lossy(&bytes).into_owned(),
            truncated_at,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// 2026-08-29T00:00:00Z, checked against `date -u -d @1787961600`.
    const A_DAY_IN_2026: u64 = 1_787_961_600;
    /// 2024-02-29T00:00:00Z, a leap day, checked against `date -u -d @1709164800`.
    const THE_LEAP_DAY_IN_2024: u64 = 1_709_164_800;

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("a temporary directory")
    }

    #[test]
    fn the_date_is_the_utc_civil_date_including_on_a_leap_day() {
        assert_eq!(utc_date(at(A_DAY_IN_2026)), "2026-08-29");
        assert_eq!(utc_date(at(THE_LEAP_DAY_IN_2024)), "2024-02-29");
        assert_eq!(utc_date(UNIX_EPOCH), "1970-01-01");
        // The last second of the day is still that day.
        assert_eq!(utc_date(at(A_DAY_IN_2026 + 86_399)), "2026-08-29");
        assert_eq!(utc_date(at(A_DAY_IN_2026 + 86_400)), "2026-08-30");
    }

    #[test]
    fn a_plain_directory_has_no_git_line_and_no_project_instructions() {
        let directory = tempdir();
        let environment = discover(directory.path(), at(A_DAY_IN_2026));
        assert!(!environment.block.contains("git:"), "{}", environment.block);
        assert_eq!(environment.instructions, None);
        assert!(environment.block.contains("date: 2026-08-29 (UTC)"));
        assert!(environment.block.contains(std::env::consts::OS));
        assert!(environment.render().contains("## Environment"));
        assert!(!environment.render().contains("## Project instructions"));
    }

    #[test]
    fn the_workspace_is_named_by_its_absolute_path() {
        let directory = tempdir();
        let root = directory.path().canonicalize().expect("a real path");
        let environment = discover(&directory.path().join("."), at(A_DAY_IN_2026));
        assert!(
            environment
                .block
                .contains(&format!("workspace: {}", root.display())),
            "{}",
            environment.block
        );
    }

    #[test]
    fn a_git_directory_reports_the_branch_its_head_points_at() {
        let directory = tempdir();
        let git = directory.path().join(".git");
        std::fs::create_dir(&git).expect("the fixture writes");
        std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").expect("the fixture writes");
        let environment = discover(directory.path(), at(A_DAY_IN_2026));
        assert!(
            environment.block.contains("git: branch main"),
            "{}",
            environment.block
        );
    }

    #[test]
    fn a_detached_head_reports_the_commit_rather_than_a_branch() {
        let directory = tempdir();
        let git = directory.path().join(".git");
        std::fs::create_dir(&git).expect("the fixture writes");
        std::fs::write(
            git.join("HEAD"),
            "0123456789abcdef0123456789abcdef01234567\n",
        )
        .expect("the fixture writes");
        let environment = discover(directory.path(), at(A_DAY_IN_2026));
        assert!(
            environment
                .block
                .contains("git: detached head at 0123456789ab"),
            "{}",
            environment.block
        );
    }

    #[test]
    fn a_git_file_is_followed_to_the_directory_it_names() {
        let directory = tempdir();
        let elsewhere = directory.path().join("real-git-dir");
        std::fs::create_dir(&elsewhere).expect("the fixture writes");
        std::fs::write(elsewhere.join("HEAD"), "ref: refs/heads/work\n")
            .expect("the fixture writes");
        let worktree = directory.path().join("worktree");
        std::fs::create_dir(&worktree).expect("the fixture writes");
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", elsewhere.display()),
        )
        .expect("the fixture writes");

        let environment = discover(&worktree, at(A_DAY_IN_2026));
        assert!(
            environment.block.contains("git: branch work"),
            "{}",
            environment.block
        );
    }

    #[test]
    fn a_repository_whose_head_cannot_be_read_still_says_it_is_a_repository() {
        let directory = tempdir();
        std::fs::create_dir(directory.path().join(".git")).expect("the fixture writes");
        let environment = discover(directory.path(), at(A_DAY_IN_2026));
        assert!(
            environment
                .block
                .contains("git: repository, head unreadable"),
            "{}",
            environment.block
        );
    }

    #[test]
    fn the_harness_neutral_instruction_file_wins_over_the_vendor_one() {
        let directory = tempdir();
        std::fs::write(directory.path().join("AGENTS.md"), "the neutral one\n")
            .expect("the fixture writes");
        std::fs::write(directory.path().join("CLAUDE.md"), "the vendor one\n")
            .expect("the fixture writes");
        let environment = discover(directory.path(), at(A_DAY_IN_2026));
        let instructions = environment
            .instructions
            .as_ref()
            .expect("a project instruction file is found");
        assert_eq!(instructions.text, "the neutral one\n");
        assert_eq!(instructions.truncated_at, None);
        let rendered = environment.render();
        assert!(rendered.contains("## Project instructions ("), "{rendered}");
        assert!(rendered.contains("AGENTS.md"), "{rendered}");
        assert!(rendered.ends_with("the neutral one\n"), "{rendered}");
    }

    #[test]
    fn the_vendor_instruction_file_is_read_when_it_is_the_only_one() {
        let directory = tempdir();
        std::fs::write(directory.path().join("CLAUDE.md"), "only this\n")
            .expect("the fixture writes");
        let instructions = discover(directory.path(), at(A_DAY_IN_2026))
            .instructions
            .expect("a project instruction file is found");
        assert_eq!(instructions.text, "only this\n");
        assert!(instructions.path.ends_with("CLAUDE.md"));
    }

    #[test]
    fn an_oversized_instruction_file_is_cut_at_a_line_and_says_so_in_words() {
        let directory = tempdir();
        let line = "x".repeat(63);
        let big: String = std::iter::repeat_n(format!("{line}\n"), 640).collect();
        assert_eq!(big.len(), 40 * 1024, "the fixture is 40 KiB");
        std::fs::write(directory.path().join("CLAUDE.md"), &big).expect("the fixture writes");

        let environment = discover(directory.path(), at(A_DAY_IN_2026));
        let instructions = environment
            .instructions
            .as_ref()
            .expect("a project instruction file is found");
        assert_eq!(instructions.truncated_at, Some(40 * 1024));
        assert!(
            u64::try_from(instructions.text.len()).expect("a length fits")
                <= MAX_INSTRUCTIONS_BYTES
        );
        assert!(
            instructions.text.ends_with('\n'),
            "the cut lands on a line boundary"
        );
        let rendered = environment.render();
        assert!(
            rendered.contains(&format!(
                "Only the first {} of 40960 bytes",
                instructions.text.len()
            )),
            "the cut is stated: {}",
            &rendered[..400.min(rendered.len())]
        );
    }
}
