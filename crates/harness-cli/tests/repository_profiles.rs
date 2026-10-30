//! A repository's own profile, `<workspace>/.b10x/harness.toml`, through the shipped binary.
//!
//! The policy under test (decision-blocker `repository-profile-trust-policy`, 2026-10-23): a
//! repository profile is read only for a path the operator trusted; it may set `provider`, `model`
//! and `max-turns` and nothing else; and it sits above the operator's `[default]` and below every
//! `-p` and every typed flag.
//!
//! Every case runs on a machine of its own: `HOME`, `XDG_CONFIG_HOME` and `XDG_STATE_HOME` all
//! point into one temporary directory, so no case reads or writes the operator's real
//! configuration or trust record.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");
/// The environment variable the runs here read their key from.
const TEST_KEY_VAR: &str = "B10X_HARNESS_TEST_KEY";

struct Fixture {
    child: Child,
    base_url: String,
    record: Option<PathBuf>,
}

impl Fixture {
    fn start(scenario: &str) -> Self {
        Self::launch(scenario, None)
    }

    /// The same, writing one line per request it is sent to `record` (never a credential's value).
    fn recording(scenario: &str, record: PathBuf) -> Self {
        Self::launch(scenario, Some(record))
    }

    fn launch(scenario: &str, record: Option<PathBuf>) -> Self {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("fake_responses.py");
        let mut command = Command::new("python3");
        command.arg(&script).arg("--scenario").arg(scenario);
        if let Some(record) = &record {
            command.arg("--record").arg(record);
        }
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the fixture starts; python3 must be available");
        let mut line = String::new();
        BufReader::new(child.stdout.as_mut().expect("piped stdout"))
            .read_line(&mut line)
            .expect("the fixture announces its address");
        let ready: Value = serde_json::from_str(&line).expect("the announcement is JSON");
        Self {
            base_url: ready["base_url"].as_str().expect("a base url").to_owned(),
            child,
            record,
        }
    }

    /// How many requests reached it.
    fn requests(&self) -> usize {
        let record = self.record.as_ref().expect("a recording fixture");
        fs::read_to_string(record)
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Output {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Output {
    fn events(&self) -> Vec<Value> {
        self.stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("one JSON event per line"))
            .collect()
    }

    fn started(&self) -> Value {
        self.events()
            .into_iter()
            .find(|event| event["kind"] == "started")
            .unwrap_or_else(|| {
                panic!(
                    "a `started` record; status {:?}\nstdout: {}\nstderr: {}",
                    self.status, self.stdout, self.stderr
                )
            })
    }

    fn stop(&self) -> Value {
        self.events().last().expect("a terminal event")["stop"].clone()
    }

    /// The stderr lines that mention the repository profile's file.
    fn mentions_of_the_file(&self) -> Vec<&str> {
        self.stderr
            .lines()
            .filter(|line| line.contains(".b10x/harness.toml"))
            .collect()
    }
}

/// One operator's machine and one repository on it, both inside one temporary directory.
struct Machine {
    root: tempfile::TempDir,
}

impl Machine {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("a temporary directory");
        let workspace = root.path().join("work");
        fs::create_dir_all(&workspace).expect("a workspace");
        fs::write(workspace.join("README.md"), "hello harness\n").expect("a readable file");
        Self { root }
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn config_dir(&self) -> PathBuf {
        self.home().join("config").join("b10x")
    }

    fn operator_file(&self) -> PathBuf {
        self.config_dir().join("harness.toml")
    }

    fn trust_record(&self) -> PathBuf {
        self.config_dir().join("harness-trust.toml")
    }

    fn workspace(&self) -> PathBuf {
        self.root.path().join("work")
    }

    fn canonical_workspace(&self) -> PathBuf {
        self.workspace().canonicalize().expect("canonical")
    }

    fn repository_file(&self) -> PathBuf {
        self.canonical_workspace()
            .join(".b10x")
            .join("harness.toml")
    }

    fn operator(&self, text: &str) -> &Self {
        fs::create_dir_all(self.config_dir()).expect("a config directory");
        fs::write(self.operator_file(), text).expect("the operator's config");
        self
    }

    fn repository(&self, text: &str) -> &Self {
        fs::create_dir_all(self.workspace().join(".b10x")).expect("a .b10x directory");
        fs::write(self.workspace().join(".b10x").join("harness.toml"), text)
            .expect("the repository profile");
        self
    }

    fn command(&self) -> Command {
        let mut command = Command::new(BINARY);
        command
            .stdin(Stdio::null())
            .env("HOME", self.home())
            .env("XDG_CONFIG_HOME", self.home().join("config"))
            .env("XDG_STATE_HOME", self.home().join("state"))
            .env(TEST_KEY_VAR, "test-key")
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY");
        command
    }

    fn invoke(&self, arguments: &[&str], directory: Option<&Path>) -> Output {
        let mut command = self.command();
        command.args(arguments);
        if let Some(directory) = directory {
            command.current_dir(directory);
        }
        let output = command.output().expect("the binary runs");
        Output {
            status: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    fn trust(&self, path: &Path) -> Output {
        let path = path.to_str().expect("a UTF-8 path");
        self.invoke(&["profiles", "trust", path], None)
    }

    fn untrust(&self, path: &Path) -> Output {
        let path = path.to_str().expect("a UTF-8 path");
        self.invoke(&["profiles", "untrust", path], None)
    }

    fn trusted(&self) -> &Self {
        let output = self.trust(&self.workspace());
        assert_eq!(output.status, Some(0), "trusted: {}", output.stderr);
        self
    }

    /// A `--json` run over this machine's workspace, naming nothing about the endpoint.
    fn run(&self, extra: &[&str]) -> Output {
        self.run_over(&self.workspace(), extra)
    }

    fn run_over(&self, workspace: &Path, extra: &[&str]) -> Output {
        let workspace = workspace.to_str().expect("a UTF-8 path");
        let mut arguments = vec![
            "run",
            "--json",
            "--no-session",
            "--input",
            "read the readme and tell me what it says",
            "--workspace",
            workspace,
        ];
        arguments.extend_from_slice(extra);
        self.invoke(&arguments, None)
    }

    /// The same, against the fixture by a typed `--base-url`, so no provider is consulted.
    fn run_against(&self, fixture: &Fixture, extra: &[&str]) -> Output {
        let mut arguments = vec![
            "--base-url",
            fixture.base_url.as_str(),
            "--api-key-env",
            TEST_KEY_VAR,
        ];
        arguments.extend_from_slice(extra);
        self.run(&arguments)
    }
}

#[test]
fn an_untrusted_repository_profile_is_not_applied_and_says_how_to_trust_it() {
    let fixture = Fixture::start("text");
    let machine = Machine::new();
    machine
        .operator("[default]\nmodel = \"operator-model\"\n")
        .repository("[default]\nmodel = \"repo-model\"\n");

    let output = machine.run_against(&fixture, &[]);

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    let started = output.started();
    assert_eq!(
        started["model"], "operator-model",
        "the file was not applied"
    );
    let names: Vec<&str> = started["profiles"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|profile| profile["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(names, vec!["default"], "and is not in the record");

    let lines = output.mentions_of_the_file();
    assert_eq!(lines.len(), 1, "one stderr line: {}", output.stderr);
    let expected_file = machine.repository_file().display().to_string();
    assert!(lines[0].contains(&expected_file), "{}", lines[0]);
    let command = format!(
        "b10x-harness profiles trust {}",
        machine.canonical_workspace().display()
    );
    assert!(
        lines[0].contains(&command),
        "the exact command `{command}`: {}",
        lines[0]
    );
}

#[test]
fn a_workspace_without_a_repository_profile_says_nothing_about_one() {
    let fixture = Fixture::start("text");
    let machine = Machine::new();
    machine.operator("[default]\nmodel = \"operator-model\"\n");

    let output = machine.run_against(&fixture, &[]);

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert!(
        !output.stderr.contains("repository"),
        "no file, no message: {}",
        output.stderr
    );
}

/// The story's acceptance: the same run, with and without the trusted file.
#[test]
fn a_trusted_repository_profile_sets_model_and_max_turns_where_the_same_run_without_it_does_not() {
    // Two turns: the model reads a file, then answers.
    let fixture = Fixture::start("flat-tool");
    let machine = Machine::new();
    machine
        .operator("[default]\nmodel = \"operator-model\"\nmax-turns = 5\n")
        .repository("[default]\nmodel = \"repo-model\"\nmax-turns = 1\n")
        .trusted();

    let with_it = machine.run_against(&fixture, &[]);
    assert_eq!(with_it.started()["model"], "repo-model");
    assert_eq!(
        with_it.stop(),
        serde_json::json!({"kind": "max-turns", "limit": 1}),
        "stderr: {}",
        with_it.stderr
    );
    assert_eq!(with_it.status, Some(2), "a binding budget is exit 2");
    assert!(
        with_it.mentions_of_the_file().is_empty(),
        "a trusted file is applied without a notice: {}",
        with_it.stderr
    );

    fs::remove_file(machine.workspace().join(".b10x").join("harness.toml")).expect("removed");
    let without_it = machine.run_against(&fixture, &[]);
    assert_eq!(without_it.started()["model"], "operator-model");
    assert_eq!(without_it.stop(), serde_json::json!({"kind": "completed"}));
    assert_eq!(without_it.status, Some(0), "stderr: {}", without_it.stderr);

    // Permissions stay the operator's: everything the record says the run may reach is the same
    // with the repository's file and without it.
    let reach = |output: &Output| {
        let started = output.started();
        [
            "published_tools",
            "operations",
            "withheld",
            "toolchains",
            "mcp",
            "skills",
            "agents",
            "memories",
            "credential_source",
        ]
        .map(|field| (field, started[field].clone()))
    };
    assert_eq!(
        reach(&with_it),
        reach(&without_it),
        "the repository profile changed what the run may reach"
    );
    assert_eq!(
        with_it.started()["published_tools"],
        serde_json::json!(["file_read", "dir_list", "search", "find"]),
        "the operator's own configuration sets no `write`, and the repository cannot"
    );
}

/// `provider` selects one of the operator's providers, and the file's `model` goes through it.
#[test]
fn a_trusted_repository_profile_selects_the_provider_where_the_same_run_without_it_has_none() {
    let fixture = Fixture::start("text");
    let machine = Machine::new();
    machine
        .operator(&format!(
            "[providers.openai]\nbase-url = \"{}\"\napi-key-env = \"B10X_HARNESS_TEST_KEY\"\n",
            fixture.base_url
        ))
        .repository("[default]\nprovider = \"openai\"\nmodel = \"repo-model\"\n")
        .trusted();

    let with_it = machine.run(&[]);
    assert_eq!(with_it.status, Some(0), "stderr: {}", with_it.stderr);
    let started = with_it.started();
    assert_eq!(started["credential_source"], "provider:openai");
    assert_eq!(
        started["model"], "repo-model",
        "the file's model, not the provider's default"
    );

    fs::remove_file(machine.workspace().join(".b10x").join("harness.toml")).expect("removed");
    let without_it = machine.run(&[]);
    assert_eq!(without_it.status, Some(1), "stdout: {}", without_it.stdout);
    assert!(
        without_it.stderr.contains("no endpoint or model"),
        "{}",
        without_it.stderr
    );
}

#[test]
fn a_trusted_repository_profile_carrying_a_permission_key_refuses_the_run_before_any_model_call() {
    let machine = Machine::new();
    machine
        .repository("[default]\nmodel = \"repo-model\"\nwrite = true\napprove-up-to = \"high\"\n")
        .trusted();

    // An endpoint nothing listens on: a refusal that came from it would say so.
    let output = machine.run(&["--base-url", "http://127.0.0.1:1/v1"]);

    assert_eq!(output.status, Some(1), "stdout: {}", output.stdout);
    for named in ["`write`", "`approve-up-to`"] {
        assert!(output.stderr.contains(named), "{named}: {}", output.stderr);
    }
    assert!(
        output
            .stderr
            .contains(&machine.repository_file().display().to_string()),
        "{}",
        output.stderr
    );
    assert!(
        !output.stdout.contains("\"started\""),
        "the run never started: {}",
        output.stdout
    );
}

#[test]
fn a_trusted_repository_profile_carrying_another_table_refuses_the_run() {
    let machine = Machine::new();
    machine
        .repository(
            "[default]\nmodel = \"repo-model\"\n[providers.openai]\nbase-url = \"https://elsewhere.example/v1\"\n\
             [[profiles]]\nname = \"w\"\nwrite = true\n",
        )
        .trusted();

    let output = machine.run(&["--base-url", "http://127.0.0.1:1/v1"]);

    assert_eq!(output.status, Some(1), "stdout: {}", output.stdout);
    for named in ["[providers]", "[[profiles]]"] {
        assert!(output.stderr.contains(named), "{named}: {}", output.stderr);
    }
}

#[test]
fn the_operator_default_is_beneath_the_repository_profile() {
    let fixture = Fixture::start("text");
    let machine = Machine::new();
    machine
        .operator("[default]\nmodel = \"operator-model\"\n")
        .repository("[default]\nmodel = \"repo-model\"\n")
        .trusted();

    let output = machine.run_against(&fixture, &[]);

    assert_eq!(output.started()["model"], "repo-model");
}

#[test]
fn a_named_profile_is_above_the_repository_profile() {
    let fixture = Fixture::start("text");
    let machine = Machine::new();
    machine
        .operator("[[profiles]]\nname = \"pick\"\nmodel = \"profile-model\"\n")
        .repository("[default]\nmodel = \"repo-model\"\n")
        .trusted();

    let output = machine.run_against(&fixture, &["-p", "pick"]);

    assert_eq!(output.started()["model"], "profile-model");
}

#[test]
fn a_typed_flag_is_above_the_repository_profile_and_every_named_one() {
    let fixture = Fixture::start("text");
    let machine = Machine::new();
    machine
        .operator("[[profiles]]\nname = \"pick\"\nmodel = \"profile-model\"\n")
        .repository("[default]\nmodel = \"repo-model\"\n")
        .trusted();

    let typed = machine.run_against(&fixture, &["--model", "typed-model"]);
    assert_eq!(typed.started()["model"], "typed-model");

    let over_a_profile = machine.run_against(&fixture, &["-p", "pick", "--model", "typed-model"]);
    assert_eq!(over_a_profile.started()["model"], "typed-model");
}

#[test]
fn session_started_names_the_repository_profile_by_its_file_and_digest_in_precedence_order() {
    let fixture = Fixture::start("text");
    let machine = Machine::new();
    machine
        .operator(
            "[default]\nmodel = \"operator-model\"\n[[profiles]]\nname = \"pick\"\nmax-turns = 7\n",
        )
        .repository("[default]\nmodel = \"repo-model\"\n")
        .trusted();

    let started = machine.run_against(&fixture, &["-p", "pick"]).started();
    let profiles = started["profiles"].as_array().expect("a list").clone();
    let rows: Vec<(String, String)> = profiles
        .iter()
        .map(|profile| {
            (
                profile["name"].as_str().expect("a name").to_owned(),
                profile["source"].as_str().expect("a source").to_owned(),
            )
        })
        .collect();
    let operator = machine.operator_file().display().to_string();
    assert_eq!(
        rows,
        vec![
            ("default".to_owned(), operator.clone()),
            (
                "repository".to_owned(),
                machine.repository_file().display().to_string()
            ),
            ("pick".to_owned(), operator),
        ]
    );
    for profile in &profiles {
        let digest = profile["sha256"].as_str().expect("a digest");
        assert!(
            digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "{profile}"
        );
    }

    // The digest is over the table, so editing the file moves the repository's and nothing else.
    machine.repository("[default]\nmodel = \"repo-model-2\"\n");
    let again = machine.run_against(&fixture, &["-p", "pick"]).started();
    let digests = |started: &Value| -> Vec<String> {
        started["profiles"]
            .as_array()
            .expect("a list")
            .iter()
            .map(|profile| profile["sha256"].as_str().expect("a digest").to_owned())
            .collect()
    };
    let (before, after) = (digests(&started), digests(&again));
    assert_eq!(before[0], after[0]);
    assert_ne!(before[1], after[1], "the repository's own table changed");
    assert_eq!(before[2], after[2]);
}

#[cfg(unix)]
#[test]
fn trust_and_untrust_round_trip_through_the_binary_keyed_by_the_canonical_path() {
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = Fixture::start("text");
    let machine = Machine::new();
    let operator_bytes = "[default]\nmodel = \"operator-model\"\n";
    machine
        .operator(operator_bytes)
        .repository("[default]\nmodel = \"repo-model\"\n");
    let link = machine.root.path().join("link");
    std::os::unix::fs::symlink(machine.workspace(), &link).expect("a symlink");
    let canonical = machine.canonical_workspace().display().to_string();

    let trusted = machine.trust(&link);
    assert_eq!(trusted.status, Some(0), "stderr: {}", trusted.stderr);
    assert!(trusted.stdout.contains(&canonical), "{}", trusted.stdout);
    let record = fs::read_to_string(machine.trust_record()).expect("a trust record");
    assert!(record.contains(&canonical), "{record}");
    assert!(
        !record.contains(&link.display().to_string()),
        "the link is not what is recorded: {record}"
    );
    let mode = fs::metadata(machine.trust_record())
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    assert_eq!(
        fs::read_to_string(machine.operator_file()).expect("reads"),
        operator_bytes,
        "the operator's own configuration is never edited"
    );

    // Trusted by the link, applied when the run names the real path — and the other way round.
    assert_eq!(
        machine.run_against(&fixture, &[]).started()["model"],
        "repo-model"
    );
    let over_the_link = machine.run_over(
        &link,
        &[
            "--base-url",
            fixture.base_url.as_str(),
            "--api-key-env",
            TEST_KEY_VAR,
        ],
    );
    assert_eq!(over_the_link.started()["model"], "repo-model");

    let untrusted = machine.untrust(&machine.workspace());
    assert_eq!(untrusted.status, Some(0), "stderr: {}", untrusted.stderr);
    let record = fs::read_to_string(machine.trust_record()).expect("a trust record");
    assert!(!record.contains(&canonical), "{record}");
    let after = machine.run_against(&fixture, &[]);
    assert_eq!(after.started()["model"], "operator-model");
    assert_eq!(after.mentions_of_the_file().len(), 1, "{}", after.stderr);
}

#[test]
fn trust_refuses_a_path_that_does_not_exist() {
    let machine = Machine::new();
    let missing = machine.root.path().join("not-there");

    let output = machine.trust(&missing);

    assert_eq!(output.status, Some(1), "stdout: {}", output.stdout);
    assert!(output.stderr.contains("not-there"), "{}", output.stderr);
    assert!(!machine.trust_record().exists(), "nothing was written");
}

#[test]
fn trust_for_one_repository_does_not_admit_another() {
    let fixture = Fixture::start("text");
    let machine = Machine::new();
    machine
        .operator("[default]\nmodel = \"operator-model\"\n")
        .repository("[default]\nmodel = \"repo-model\"\n");
    let other = machine.root.path().join("other");
    fs::create_dir_all(&other).expect("another repository");
    let trusted = machine.trust(&other);
    assert_eq!(trusted.status, Some(0), "stderr: {}", trusted.stderr);

    let output = machine.run_against(&fixture, &[]);

    assert_eq!(output.started()["model"], "operator-model");
    assert_eq!(output.mentions_of_the_file().len(), 1, "{}", output.stderr);
}

#[test]
fn profiles_list_show_and_explain_name_the_repository_profile_and_whether_it_is_trusted() {
    let machine = Machine::new();
    machine
        .operator("[[profiles]]\nname = \"pick\"\nmax-turns = 7\n")
        .repository("[default]\nmodel = \"repo-model\"\n");
    let here = machine.workspace();
    let file = machine.repository_file().display().to_string();

    let listed = machine.invoke(&["profiles", "list"], Some(&here));
    assert_eq!(listed.status, Some(0), "stderr: {}", listed.stderr);
    let line = listed
        .stdout
        .lines()
        .find(|line| line.starts_with("repository"))
        .unwrap_or_else(|| panic!("a repository line: {}", listed.stdout));
    assert!(line.contains(&file), "{line}");
    assert!(line.contains("not trusted"), "{line}");

    machine.trusted();
    let listed = machine.invoke(&["profiles", "list"], Some(&here));
    let line = listed
        .stdout
        .lines()
        .find(|line| line.starts_with("repository"))
        .unwrap_or_else(|| panic!("a repository line: {}", listed.stdout));
    assert!(line.contains(&file), "{line}");
    assert!(
        line.contains("trusted") && !line.contains("not trusted"),
        "{line}"
    );

    let shown = machine.invoke(&["profiles", "show", "pick"], Some(&here));
    assert_eq!(shown.status, Some(0), "stderr: {}", shown.stderr);
    assert!(shown.stdout.contains("repo-model"), "{}", shown.stdout);

    let explained = machine.invoke(&["profiles", "explain", "-p", "pick"], Some(&here));
    assert_eq!(explained.status, Some(0), "stderr: {}", explained.stderr);
    assert!(
        explained
            .stdout
            .contains(&format!("profile repository <- {file}")),
        "{}",
        explained.stdout
    );
    assert!(
        explained.stdout.contains("--model repo-model"),
        "{}",
        explained.stdout
    );
    assert!(
        explained.stdout.contains("--max-turns 7"),
        "{}",
        explained.stdout
    );
}

/// The operator's own `[default]` chose `claude`; a trusted repository chose `openai`, which the
/// operator has pointed at the recording fixture.
fn a_repository_that_moves_the_provider(machine: &Machine, fixture: &Fixture) {
    machine
        .operator(&format!(
            "[default]\nprovider = \"claude\"\n\
             [providers.claude]\nbase-url = \"http://127.0.0.1:1/v1\"\n\
             [providers.openai]\nbase-url = \"{}\"\n",
            fixture.base_url
        ))
        .repository("[default]\nprovider = \"openai\"\nmodel = \"repo-model\"\n")
        .trusted();
}

/// Decision 1 of round 2: a credential the operator typed is meant for a provider the operator
/// chose. Where the repository's `provider` decided which one that is, the run is refused by name
/// before any request, for every flag that names a credential.
#[test]
fn a_typed_credential_is_refused_by_name_when_the_repository_chose_the_provider() {
    let machine = Machine::new();
    let key_file = machine.root.path().join("key");
    fs::write(&key_file, "typed-key\n").expect("a key file");
    let key_file = key_file.to_str().expect("UTF-8").to_owned();
    for typed in [
        vec!["--api-key-env", TEST_KEY_VAR],
        vec!["--api-key-file", key_file.as_str()],
        vec!["--api-key-secret", "operator/claude"],
        vec!["--oauth-token-file", key_file.as_str()],
        vec!["--oauth-token-env", TEST_KEY_VAR],
    ] {
        let flag = typed[0];
        let fixture = Fixture::recording("text", machine.root.path().join(format!("{flag}.jsonl")));
        a_repository_that_moves_the_provider(&machine, &fixture);

        let output = machine.run(&typed);

        assert_eq!(output.status, Some(1), "{flag}: {}", output.stderr);
        assert_eq!(fixture.requests(), 0, "{flag}: no request was sent");
        assert!(
            !output.stdout.contains("\"started\""),
            "{flag}: {}",
            output.stdout
        );
        for named in [
            machine.repository_file().display().to_string(),
            "provider = \"openai\"".to_owned(),
            format!("`{flag}`"),
            "-p".to_owned(),
        ] {
            assert!(
                output.stderr.contains(&named),
                "{flag}: names {named}: {}",
                output.stderr
            );
        }
    }
}

/// The other side of decision 1: where the operator chose the provider, a typed credential goes
/// through exactly as it did before repository profiles existed.
#[test]
fn a_typed_credential_goes_through_where_the_operator_chose_the_provider() {
    // A `-p` profile that names the provider pins it over the repository's.
    let machine = Machine::new();
    let fixture = Fixture::recording("text", machine.root.path().join("requests.jsonl"));
    machine
        .operator(&format!(
            "[providers.openai]\nbase-url = \"{}\"\n\
             [[profiles]]\nname = \"pin\"\nprovider = \"openai\"\n",
            fixture.base_url
        ))
        .repository("[default]\nprovider = \"openai\"\nmodel = \"repo-model\"\n")
        .trusted();
    let output = machine.run(&["-p", "pin", "--api-key-env", TEST_KEY_VAR]);
    assert_eq!(output.status, Some(0), "pinned by -p: {}", output.stderr);
    assert_eq!(output.started()["credential_source"], "api-key:environment");
    assert_eq!(fixture.requests(), 1);

    // The operator's `[default]` already chose the same provider, so the repository moved nothing.
    let machine = Machine::new();
    let fixture = Fixture::recording("text", machine.root.path().join("requests.jsonl"));
    machine
        .operator(&format!(
            "[default]\nprovider = \"openai\"\n[providers.openai]\nbase-url = \"{}\"\n",
            fixture.base_url
        ))
        .repository("[default]\nprovider = \"openai\"\nmodel = \"repo-model\"\n")
        .trusted();
    let output = machine.run(&["--api-key-env", TEST_KEY_VAR]);
    assert_eq!(output.status, Some(0), "same provider: {}", output.stderr);
    assert_eq!(fixture.requests(), 1);

    // A typed `--base-url` consults no provider at all.
    let machine = Machine::new();
    let fixture = Fixture::recording("text", machine.root.path().join("requests.jsonl"));
    a_repository_that_moves_the_provider(&machine, &fixture);
    let output = machine.run_against(&fixture, &[]);
    assert_eq!(
        output.status,
        Some(0),
        "typed --base-url: {}",
        output.stderr
    );
    assert_eq!(output.started()["model"], "repo-model");
    assert_eq!(fixture.requests(), 1);
}

/// Decision 5 of round 2, through the binary: a trust record another user could write is not
/// trusted, and the run says which file and why.
#[cfg(unix)]
#[test]
fn a_trust_record_others_can_write_refuses_the_run_by_name() {
    use std::os::unix::fs::PermissionsExt as _;

    let machine = Machine::new();
    machine
        .repository("[default]\nmodel = \"repo-model\"\n")
        .trusted();
    fs::set_permissions(machine.trust_record(), fs::Permissions::from_mode(0o666)).expect("chmod");

    let output = machine.run(&["--base-url", "http://127.0.0.1:1/v1"]);

    assert_eq!(output.status, Some(1), "stdout: {}", output.stdout);
    assert!(
        output
            .stderr
            .contains(&machine.trust_record().display().to_string()),
        "{}",
        output.stderr
    );
    assert!(output.stderr.contains("writable"), "{}", output.stderr);
}
