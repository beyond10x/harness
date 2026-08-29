//! What a run leaves behind, so a second run can pick it up.
//!
//! Today a run's whole record is its stdout. `LoopOutcome.items` is documented as *"the complete
//! conversation, ready to be replayed into a following run"* and nothing replayed it, so a stream
//! that failed on turn 20 of a paid run lost all twenty turns and no follow-up question could be
//! asked without paying for the whole conversation again.
//!
//! # Where a session lives, and why not here
//!
//! Under `$XDG_STATE_HOME`, never in the workspace. This repository is private and **never commits
//! a transcript** (`AGENTS.md` § Safety envelope); a session file written next to the code is one
//! `git add -A` away from being committed, and a transcript carries whatever the model read. State
//! rather than cache or config: it survives a reboot, it is not reproducible, and nobody edits it
//! by hand.
//!
//! # What is deliberately not in the file
//!
//! **No credential**, of any kind. The credential is fetched per call from a source the caller
//! names, and a session file is a plain-text file on disk with a long life.
//!
//! **No instructions text.** The standing instruction is a *function* of this run's catalogue, its
//! write scope and the project files it discovered — all of which can differ by the time a session
//! is resumed. Storing the old text would replay a conversation under an instruction that no
//! longer describes what the run can do, and a run whose instruction is not derivable from its
//! flags is one nobody can reproduce. The caller re-derives it, and the difference is visible in
//! the flags rather than hidden in the file.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use harness_loop::{LoopOutcome, RunLedger};
use harness_wire::{Item, Usage, WireId};
use serde::{Deserialize, Serialize};

/// The shape this module writes and the only one it reads.
///
/// A file that says anything else is refused **by name** rather than parsed hopefully: a session
/// is replayed into a model at the caller's expense, and a field this version does not understand
/// is a difference in what the conversation means.
pub const SESSION_VERSION: u32 = 1;

/// A finished run, in the form a following run can replay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    /// The file format, always [`SESSION_VERSION`] when this code wrote it.
    pub version: u32,
    /// Sortable, unique, and the file's own name. See [`Session::new_id`].
    pub id: String,
    /// The wire the conversation was produced on.
    ///
    /// Carried because an opaque item may not cross wires (`AGENTS.md` invariant 5): resuming a
    /// session on a different wire is a refusal somebody has to be able to make, and it cannot be
    /// made without knowing which wire the items came from.
    pub wire: WireId,
    pub model: String,
    pub base_url: String,
    pub workspace: PathBuf,
    pub created_unix: u64,
    pub updated_unix: u64,
    pub turns: u64,
    /// The conversation, verbatim.
    ///
    /// Stored exactly as the loop produced it, opaque items included — payload never read, `wire`
    /// never rewritten. Anything else would be this module reinterpreting a provider item it was
    /// told not to understand.
    pub items: Vec<Item>,
    /// One entry per turn the provider reported for, across every run folded in.
    pub usage: Vec<Usage>,
    /// What the session cost so far, or [`None`] when no run in it was priced.
    ///
    /// Absent rather than zero, and absence survives folding: an unpriced run added to an unpriced
    /// session stays unpriced (`AGENTS.md` invariant 7).
    pub cost_micro_usd: Option<u64>,
    /// The structured answer of the **last** run folded in, when it gave one.
    ///
    /// Optional in the file as well as here, so a session written before `--output-schema` existed
    /// still loads. It replaces rather than accumulates, exactly as `items` does: a session's
    /// answer is its latest run's answer, and a run that answered in prose leaves none — carrying
    /// an older run's answer forward would say this conversation ended in a shape it did not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured: Option<serde_json::Value>,
}

impl Session {
    /// An empty session for a run that is about to start.
    pub fn new(
        wire: WireId,
        model: impl Into<String>,
        base_url: impl Into<String>,
        workspace: PathBuf,
    ) -> Self {
        let now = unix_now();
        Self {
            version: SESSION_VERSION,
            id: Self::new_id(),
            wire,
            model: model.into(),
            base_url: base_url.into(),
            workspace,
            created_unix: now,
            updated_unix: now,
            turns: 0,
            items: Vec::new(),
            usage: Vec::new(),
            cost_micro_usd: None,
            structured: None,
        }
    }

    /// A sortable, unique identifier, without reaching for a crate to get one.
    ///
    /// `{nanoseconds since the epoch:016x}-{process id:08x}`. Fixed-width hex so a lexical sort of
    /// the identifiers is a chronological sort of the sessions — which is what makes a directory
    /// listing meaningful and gives [`Session::latest`] a tie-break that means something. The pid
    /// is what separates two runs started in the same nanosecond by different processes; a
    /// nanosecond clock separates two runs of the same process. Sixteen hex digits hold the
    /// nanosecond count until the year 2554.
    pub fn new_id() -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_nanos()).unwrap_or(u64::MAX)
            });
        format!("{nanos:016x}-{:08x}", std::process::id())
    }

    /// Folds a finished run into this session.
    ///
    /// The outcome's items **replace** rather than append: the loop replays the whole conversation
    /// every turn and hands back all of it, so appending would store every earlier turn twice. The
    /// usage list appends, because each entry is one turn that was really billed.
    pub fn extend(&mut self, outcome: &LoopOutcome) {
        self.items.clone_from(&outcome.items);
        self.fold_spend(&outcome.usage, outcome.turns, outcome.cost_micro_usd);
        self.structured.clone_from(&outcome.structured);
    }

    /// Folds what a run **spent** into this session, for a run that never produced an outcome.
    ///
    /// A run that broke on the wire hands its caller a conversation and a [`RunLedger`] and never
    /// builds a [`LoopOutcome`] — but the turns it bought before it broke were billed like any
    /// other, and their usage and cost have already gone past the reader on stderr. A session that
    /// filed the conversation and not the figures would answer *what did that failed run cost* with
    /// nothing, which reads as *nothing*.
    ///
    /// Items are not touched here: the caller saves the vector the loop wrote back, which is the
    /// same conversation arriving by the other half of the same hand-back. No structured answer is
    /// touched either — a run that failed gave none, and clearing an earlier run's would lose it.
    pub fn spent(&mut self, ledger: &RunLedger) {
        self.fold_spend(&ledger.usage, ledger.turns, ledger.cost_micro_usd);
    }

    /// The arithmetic both of those share, so a failed run is folded exactly as an answered one is.
    ///
    /// Absence survives it: an unpriced run added to an unpriced session leaves the session
    /// unpriced rather than zeroed (`AGENTS.md` invariant 7).
    fn fold_spend(&mut self, usage: &[Usage], turns: u64, cost_micro_usd: Option<u64>) {
        self.usage.extend(usage.iter().cloned());
        self.turns = self.turns.saturating_add(turns);
        self.cost_micro_usd = match (self.cost_micro_usd, cost_micro_usd) {
            (None, None) => None,
            (spent, added) => Some(spent.unwrap_or(0).saturating_add(added.unwrap_or(0))),
        };
        self.updated_unix = unix_now();
    }

    /// Writes the session into `dir`, atomically, and answers where it went.
    ///
    /// The file appears whole or not at all: written to `<id>.json.tmp` and renamed over
    /// `<id>.json`, because a run interrupted mid-write would otherwise leave a half-session that
    /// parses as far as it goes and resumes into a conversation missing its end.
    ///
    /// A directory this call creates is created `0700` on unix — a transcript is whatever the
    /// model read. An existing directory's mode is left as the operator set it.
    ///
    /// # Errors
    ///
    /// Every failure is named with the path it happened on: an unusable identifier, a directory
    /// that cannot be created, a file that cannot be written, a rename that did not happen.
    pub fn save(&self, dir: &Path) -> Result<PathBuf, String> {
        check_id(&self.id)?;
        if !dir.exists() {
            fs::create_dir_all(dir).map_err(|error| {
                format!(
                    "creating the session directory `{}`: {error}",
                    dir.display()
                )
            })?;
            restrict(dir)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| format!("encoding session `{}`: {error}", self.id))?;
        let temporary = dir.join(format!("{}.json.tmp", self.id));
        let path = dir.join(format!("{}.json", self.id));
        fs::write(&temporary, text)
            .map_err(|error| format!("writing `{}`: {error}", temporary.display()))?;
        if let Err(error) = fs::rename(&temporary, &path) {
            let _ = fs::remove_file(&temporary);
            return Err(format!(
                "renaming `{}` onto `{}`: {error}",
                temporary.display(),
                path.display()
            ));
        }
        Ok(path)
    }

    /// Reads one session by identifier.
    ///
    /// # Errors
    ///
    /// Names the file when it cannot be read, when it is not the JSON this module writes, and when
    /// it says a version this build does not know — the last of these is a refusal rather than a
    /// best effort, because a shape this code does not understand is a conversation it would
    /// replay wrongly.
    pub fn load(dir: &Path, id: &str) -> Result<Self, String> {
        check_id(id)?;
        let path = dir.join(format!("{id}.json"));
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("reading the session `{}`: {error}", path.display()))?;
        Self::parse(&text, &path)
    }

    /// The most recently updated session in `dir`, or [`None`] when there is none.
    ///
    /// Newest by `updated_unix`, ties broken by identifier — which, the identifiers being
    /// time-ordered, breaks them by which session started later.
    ///
    /// # Errors
    ///
    /// Names the directory it could not read, and names any file in it that is not a session this
    /// build understands. A corrupt file is not skipped: a resume that silently picked an older
    /// session than the one asked for would replay the wrong conversation.
    pub fn latest(dir: &Path) -> Result<Option<Self>, String> {
        let Some(newest) = Self::list(dir)?.into_iter().next() else {
            return Ok(None);
        };
        Self::load(dir, &newest.id).map(Some)
    }

    /// Every session in `dir`, newest first.
    ///
    /// A directory that does not exist is an empty list, not an error: nothing has been saved yet.
    ///
    /// # Errors
    ///
    /// Names the directory it could not read, and names any file that is not a session this build
    /// understands.
    pub fn list(dir: &Path) -> Result<Vec<SessionRow>, String> {
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let entries =
            fs::read_dir(dir).map_err(|error| format!("reading `{}`: {error}", dir.display()))?;
        let mut rows = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| format!("reading `{}`: {error}", dir.display()))?;
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let text = fs::read_to_string(&path)
                .map_err(|error| format!("reading `{}`: {error}", path.display()))?;
            let session = Self::parse(&text, &path)?;
            rows.push(SessionRow {
                id: session.id,
                updated_unix: session.updated_unix,
                model: session.model,
                turns: session.turns,
            });
        }
        rows.sort_by(|left, right| {
            right
                .updated_unix
                .cmp(&left.updated_unix)
                .then_with(|| right.id.cmp(&left.id))
        });
        Ok(rows)
    }

    /// One session's JSON, with the version checked before the shape is trusted.
    fn parse(text: &str, path: &Path) -> Result<Self, String> {
        let value: serde_json::Value = serde_json::from_str(text)
            .map_err(|error| format!("`{}` is not a session file: {error}", path.display()))?;
        let version = value
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                format!(
                    "`{}` does not say which session version it is, so it cannot be replayed",
                    path.display()
                )
            })?;
        if version != u64::from(SESSION_VERSION) {
            return Err(format!(
                "`{}` is a version {version} session and this build reads version \
                 {SESSION_VERSION}; it is refused rather than replayed under the wrong shape",
                path.display()
            ));
        }
        serde_json::from_value(value)
            .map_err(|error| format!("`{}` is not a session file: {error}", path.display()))
    }
}

/// One line of a session listing, for a caller that wants the inventory and not the conversations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub id: String,
    pub updated_unix: u64,
    pub model: String,
    pub turns: u64,
}

/// Where sessions live when the caller names no directory.
///
/// `$XDG_STATE_HOME/b10x-harness/sessions`, or `$HOME/.local/state/…` — the XDG default, spelled
/// out rather than assumed, because `$HOME` is what exists on a machine with no XDG environment.
///
/// # Errors
///
/// Refuses by name when neither variable is set. A harness that invented a directory in that case
/// would write a transcript somewhere nobody knew to look — possibly inside the workspace, which
/// is the one place it must never be.
pub fn default_dir() -> Result<PathBuf, String> {
    if let Some(state) = std::env::var_os("XDG_STATE_HOME")
        && !state.is_empty()
    {
        return Ok(PathBuf::from(state).join("b10x-harness").join("sessions"));
    }
    if let Some(home) = std::env::var_os("HOME")
        && !home.is_empty()
    {
        return Ok(PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("b10x-harness")
            .join("sessions"));
    }
    Err(
        "neither `XDG_STATE_HOME` nor `HOME` is set, so there is no state directory to keep \
         sessions in; name one explicitly"
            .to_owned(),
    )
}

/// Seconds since the epoch, or zero on a clock set before it.
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Refuses an identifier that is not exactly one file name.
///
/// An identifier reaches this module from a command line, and `<dir>/<id>.json` with an `id` of
/// `../../.ssh/config` is a write outside the session directory. Narrow rather than escaped: a
/// run mints hex and one dash, and a walk adds the section it belongs to and every attempt on the
/// way down to it — `<flow-run>.root.1.shape.1` (design 0003 § 4), which is why the dot is a letter
/// here.
///
/// **`..` is not**, nor a leading one: those are the two spellings that leave the directory, and
/// admitting the character without refusing the pair would have re-opened exactly the hole this
/// function exists to close.
fn check_id(id: &str) -> Result<(), String> {
    let legal =
        |byte: u8| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' || byte == b'.';
    if id.is_empty() || !id.bytes().all(legal) || id.contains("..") || id.starts_with('.') {
        return Err(format!(
            "`{id}` is not a session identifier: identifiers are letters, digits, `-`, `_` and \
             `.`, and never `..` or a leading `.`"
        ));
    }
    Ok(())
}

/// Makes a session directory this process created readable only by its owner.
#[cfg(unix)]
fn restrict(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|error| {
        format!(
            "restricting the session directory `{}` to its owner: {error}",
            dir.display()
        )
    })
}

#[cfg(not(unix))]
fn restrict(_dir: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_loop::LoopStop;
    use harness_wire::{CallId, ToolOutcome};
    use serde_json::json;

    fn wire() -> WireId {
        WireId::new("openai-responses").expect("a valid wire id")
    }

    fn session() -> Session {
        Session::new(
            wire(),
            "gpt-5.6-sol",
            "https://gw.example/v1",
            PathBuf::from("/w"),
        )
    }

    fn outcome(turns: u64, cost: Option<u64>, usage: Vec<Usage>) -> LoopOutcome {
        LoopOutcome {
            stop: LoopStop::Completed,
            text: "done".to_owned(),
            items: vec![Item::user("hi"), Item::assistant("hello")],
            turns,
            usage,
            cost_micro_usd: cost,
            structured: None,
        }
    }

    fn usage(input: u64) -> Usage {
        Usage {
            model: "gpt-5.6-sol".to_owned(),
            input_tokens: input,
            output_tokens: 1,
            cached_input_tokens: 0,
            cache_creation_input_tokens: None,
        }
    }

    #[test]
    fn a_saved_session_round_trips_including_an_opaque_item() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let mut saved = session();
        saved.items = vec![
            Item::user("read the readme"),
            Item::Opaque {
                wire: wire(),
                payload: json!({"type": "reasoning", "id": "rs_1"}),
            },
            Item::result(
                CallId::new("call-1").expect("a valid call id"),
                ToolOutcome::ok(json!({"bytes": 12})),
            ),
        ];
        let path = saved.save(directory.path()).expect("the session saves");
        assert_eq!(path, directory.path().join(format!("{}.json", saved.id)));

        let loaded = Session::load(directory.path(), &saved.id).expect("the session loads");
        assert_eq!(loaded, saved, "every field survives the round trip");
        assert_eq!(
            loaded.items[1].opaque_wire(),
            Some(&wire()),
            "an opaque item keeps the wire that produced it"
        );
    }

    #[test]
    fn a_successful_save_leaves_no_temporary_file_behind() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let saved = session();
        saved.save(directory.path()).expect("the session saves");
        let names: Vec<String> = fs::read_dir(directory.path())
            .expect("the directory lists")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, vec![format!("{}.json", saved.id)]);
    }

    #[test]
    fn the_newest_session_is_the_one_resumed() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let mut older = session();
        older.id = "0000000000000001-00000001".to_owned();
        older.updated_unix = 1_000;
        older
            .save(directory.path())
            .expect("the older session saves");

        let mut newer = session();
        newer.id = "0000000000000002-00000001".to_owned();
        newer.updated_unix = 2_000;
        newer.model = "newest".to_owned();
        newer
            .save(directory.path())
            .expect("the newer session saves");

        let latest = Session::latest(directory.path())
            .expect("the directory lists")
            .expect("there is a session");
        assert_eq!(latest.model, "newest");

        let rows = Session::list(directory.path()).expect("the directory lists");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, newer.id, "the listing is newest first");
        assert_eq!(rows[0].turns, 0);
    }

    #[test]
    fn an_empty_directory_has_no_session_to_resume() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        assert_eq!(
            Session::latest(&directory.path().join("never-written")).expect("no error"),
            None
        );
    }

    #[test]
    fn a_session_that_does_not_parse_is_refused_by_name() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("broken.json");
        fs::write(&path, "{ not json").expect("the fixture writes");
        let error = Session::load(directory.path(), "broken").expect_err("a corrupt file refuses");
        assert!(error.contains("broken.json"), "{error}");
        let error = Session::list(directory.path()).expect_err("a corrupt file refuses");
        assert!(error.contains("broken.json"), "{error}");
    }

    #[test]
    fn a_session_from_a_later_version_is_refused_by_name() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let mut value = serde_json::to_value(session()).expect("a session encodes");
        value["version"] = json!(2);
        fs::write(
            directory.path().join("future.json"),
            serde_json::to_string(&value).expect("the fixture encodes"),
        )
        .expect("the fixture writes");
        let error = Session::load(directory.path(), "future").expect_err("version 2 refuses");
        assert!(error.contains("future.json"), "{error}");
        assert!(error.contains("version 2"), "{error}");
    }

    #[test]
    fn an_identifier_that_is_not_a_file_name_is_refused() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        for traversing in ["../../etc/passwd", "..", ".ssh", "a/b"] {
            let error = Session::load(directory.path(), traversing)
                .expect_err("a traversing identifier refuses");
            assert!(error.contains("not a session identifier"), "{error}");
        }
    }

    #[test]
    fn a_section_of_a_walk_names_its_own_session_and_still_stays_one_file_name() {
        // What design 0003 § 4 mints: the walk's identifier, then every open scope on the way down
        // as its own name and the attempt it is on.
        check_id(&format!(
            "{}.root.1.implement-to-review.2.implement.1",
            Session::new_id()
        ))
        .expect("a section identifier is a file name");
    }

    #[test]
    fn folding_a_run_in_sums_the_usage_and_replaces_the_conversation() {
        let mut folded = session();
        folded.usage = vec![usage(10)];
        folded.turns = 1;
        folded.items = vec![Item::user("old")];

        folded.extend(&outcome(3, None, vec![usage(20), usage(30)]));

        assert_eq!(folded.turns, 4);
        assert_eq!(
            folded.usage.len(),
            3,
            "each billed turn keeps its own entry"
        );
        assert_eq!(folded.usage[2].input_tokens, 30);
        assert_eq!(
            folded.items,
            vec![Item::user("hi"), Item::assistant("hello")],
            "the outcome carries the whole conversation, so it replaces rather than appends"
        );
    }

    #[test]
    fn a_session_extended_from_a_failed_runs_ledger_carries_what_that_run_spent() {
        // A run that broke on the wire never builds a `LoopOutcome`, so there is nothing to
        // `extend` with — only the ledger the loop wrote back. What it bought before it broke is
        // billed, and this is where it has to land or it is lost with the process.
        let mut folded = session();
        folded.usage = vec![usage(10)];
        folded.turns = 1;
        folded.cost_micro_usd = Some(400);
        folded.items = vec![Item::user("old")];

        folded.spent(&RunLedger {
            usage: vec![usage(20), usage(30)],
            cost_micro_usd: Some(100),
            turns: 2,
        });

        assert_eq!(
            folded.turns, 3,
            "the failed run's turns join the ones the session already had"
        );
        assert_eq!(
            folded.usage.len(),
            3,
            "each billed turn keeps its own entry, the failed run's included"
        );
        assert_eq!(folded.usage[2].input_tokens, 30);
        assert_eq!(folded.cost_micro_usd, Some(500));
        assert_eq!(
            folded.items,
            vec![Item::user("old")],
            "the conversation arrives by the other half of the hand-back, not this one"
        );
    }

    #[test]
    fn a_failed_run_nobody_could_price_leaves_the_session_unpriced_rather_than_zero() {
        let mut folded = session();
        folded.spent(&RunLedger {
            usage: vec![usage(20)],
            cost_micro_usd: None,
            turns: 1,
        });
        assert_eq!(folded.cost_micro_usd, None, "absence stays absence");
        assert_eq!(
            folded.usage.len(),
            1,
            "unpriced is not unreported: the tokens are still known"
        );
    }

    #[test]
    fn a_structured_answer_is_stored_beside_the_conversation_and_replaced_by_the_next_run() {
        let mut folded = session();
        assert_eq!(folded.structured, None, "a run under no schema stores none");

        let mut answered = outcome(1, None, Vec::new());
        answered.structured = Some(json!({"verdict": "ok"}));
        folded.extend(&answered);
        assert_eq!(folded.structured, Some(json!({"verdict": "ok"})));

        // A session's answer is its latest run's answer: carrying an older one forward would say
        // this conversation ended in a shape it did not.
        folded.extend(&outcome(1, None, Vec::new()));
        assert_eq!(folded.structured, None);
    }

    #[test]
    fn a_session_written_before_there_were_answers_still_loads() {
        // The field is optional in the file as well as in the type: a v1 session recorded by the
        // build before `--output-schema` existed carries no `structured` key at all.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let mut saved = session();
        saved.structured = Some(json!({"verdict": "ok"}));
        let path = saved.save(directory.path()).expect("the session saves");
        let text = fs::read_to_string(&path).expect("readable");
        assert!(text.contains("\"structured\""), "{text}");

        let older: Session = serde_json::from_str(&without_structured(&text))
            .unwrap_or_else(|error| panic!("a v1 file without the field loads: {error}"));
        assert_eq!(older.structured, None);
        assert_eq!(older.id, saved.id);
    }

    /// The same session file with the field deleted, as an older build would have written it.
    fn without_structured(text: &str) -> String {
        let mut value: serde_json::Value = serde_json::from_str(text).expect("a session file");
        value
            .as_object_mut()
            .expect("an object")
            .remove("structured");
        value.to_string()
    }

    #[test]
    fn an_unpriced_run_folded_into_an_unpriced_session_stays_unpriced() {
        let mut folded = session();
        folded.extend(&outcome(1, None, Vec::new()));
        assert_eq!(folded.cost_micro_usd, None, "absence stays absence");

        folded.extend(&outcome(1, Some(400), Vec::new()));
        assert_eq!(folded.cost_micro_usd, Some(400));

        folded.extend(&outcome(1, None, Vec::new()));
        assert_eq!(
            folded.cost_micro_usd,
            Some(400),
            "a priced session that runs unpriced keeps what it knows it spent"
        );

        folded.extend(&outcome(1, Some(100), Vec::new()));
        assert_eq!(folded.cost_micro_usd, Some(500));
    }

    #[test]
    fn identifiers_sort_in_the_order_they_were_minted() {
        let first = Session::new_id();
        let second = Session::new_id();
        assert!(first < second, "{first} then {second}");
        assert_eq!(
            first.len(),
            second.len(),
            "fixed width, so a lexical sort is a time sort"
        );
        check_id(&first).expect("a minted identifier is a file name");
    }

    #[test]
    fn the_default_directory_follows_the_state_variable_and_refuses_without_one() {
        // Reading the process environment, not writing it: these tests run in parallel and a
        // `set_var` here would change what another test sees.
        let directory = default_dir();
        match (std::env::var_os("XDG_STATE_HOME"), std::env::var_os("HOME")) {
            (Some(state), _) if !state.is_empty() => {
                assert_eq!(
                    directory.expect("a state directory"),
                    PathBuf::from(state).join("b10x-harness").join("sessions")
                );
            }
            (_, Some(home)) if !home.is_empty() => {
                assert_eq!(
                    directory.expect("a state directory"),
                    PathBuf::from(home)
                        .join(".local/state")
                        .join("b10x-harness")
                        .join("sessions")
                );
            }
            _ => {
                let error = directory.expect_err("without either variable there is no default");
                assert!(error.contains("XDG_STATE_HOME"), "{error}");
            }
        }
    }
}
