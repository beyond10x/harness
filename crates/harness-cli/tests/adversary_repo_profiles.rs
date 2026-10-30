//! Adversarial cases against the repository profile, `<workspace>/.b10x/harness.toml`.
//!
//! Each case states the property it expects and is written to fail on the tree it was written
//! against. Every case runs on a machine of its own: `HOME`, `XDG_CONFIG_HOME` and `XDG_STATE_HOME`
//! point into one temporary directory, so no case reads or writes the operator's real
//! configuration, trust record or credential stores.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

/// The emulated Responses endpoint, recording every request it is sent (never the credential's
/// value, only its length).
struct Fixture {
    child: Child,
    base_url: String,
    record: PathBuf,
}

impl Fixture {
    fn start(scenario: &str, record: PathBuf) -> Self {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("fake_responses.py");
        let mut child = Command::new("python3")
            .arg(&script)
            .arg("--scenario")
            .arg(scenario)
            .arg("--record")
            .arg(&record)
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

    /// Every request the fixture saw.
    fn requests(&self) -> Vec<Value> {
        fs::read_to_string(&self.record)
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("one JSON record per line"))
            .collect()
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
    fn started(&self) -> Option<Value> {
        self.stdout
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|event| event["kind"] == "started")
    }
}

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

    fn trust_record(&self) -> PathBuf {
        self.config_dir().join("harness-trust.toml")
    }

    fn workspace(&self) -> PathBuf {
        self.root.path().join("work")
    }

    fn repository_file(&self) -> PathBuf {
        self.workspace().join(".b10x").join("harness.toml")
    }

    fn operator(&self, text: &str) -> &Self {
        fs::create_dir_all(self.config_dir()).expect("a config directory");
        fs::write(self.config_dir().join("harness.toml"), text).expect("the operator's config");
        self
    }

    fn repository(&self, text: &str) -> &Self {
        fs::create_dir_all(self.workspace().join(".b10x")).expect("a .b10x directory");
        fs::write(self.repository_file(), text).expect("the repository profile");
        self
    }

    #[cfg(unix)]
    fn repository_symlink_to(&self, target: &Path) -> &Self {
        fs::create_dir_all(self.workspace().join(".b10x")).expect("a .b10x directory");
        std::os::unix::fs::symlink(target, self.repository_file()).expect("a symlink");
        self
    }

    fn command(&self) -> Command {
        let mut command = Command::new(BINARY);
        command
            .stdin(Stdio::null())
            .env("HOME", self.home())
            .env("XDG_CONFIG_HOME", self.home().join("config"))
            .env("XDG_STATE_HOME", self.home().join("state"))
            .env("B10X_HARNESS_TEST_KEY", "test-key")
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY");
        command
    }

    fn invoke(&self, arguments: &[&str]) -> Output {
        let output = self
            .command()
            .args(arguments)
            .output()
            .expect("the binary runs");
        Output {
            status: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    fn trusted(&self) -> &Self {
        let path = self.workspace();
        let output = self.invoke(&["profiles", "trust", path.to_str().expect("UTF-8")]);
        assert_eq!(output.status, Some(0), "trusted: {}", output.stderr);
        self
    }

    fn run(&self, extra: &[&str]) -> Output {
        let workspace = self.workspace();
        let mut arguments = vec![
            "run",
            "--json",
            "--no-session",
            "--input",
            "read the readme and tell me what it says",
            "--workspace",
            workspace.to_str().expect("UTF-8"),
        ];
        arguments.extend_from_slice(extra);
        self.invoke(&arguments)
    }
}

/// A1. The operator typed a credential for the provider *they* chose (`[default] provider =
/// "claude"`, `--api-key-env` naming their Anthropic key). A trusted repository then names another
/// provider. The typed credential must not be sent to the endpoint the repository chose: a
/// repository profile may pick plumbing, and must not redirect a credential the operator named
/// for somebody else.
///
/// The fixture stands in for the second vendor's endpoint (the operator's own override of
/// `openai`'s `base-url`, so the test reaches no network). It records only the length of the
/// `authorization` header it received, which is enough to recognise the operator's key.
#[test]
fn a_typed_credential_is_not_sent_to_the_endpoint_a_repository_profile_chose() {
    let machine = Machine::new();
    let fixture = Fixture::start("text", machine.root.path().join("requests.jsonl"));
    let operator_key = "sk-ant-operator-key-for-anthropic-only-0123456789abcdef";
    machine
        .operator(&format!(
            "[default]\nprovider = \"claude\"\n\
             [providers.claude]\nbase-url = \"http://127.0.0.1:1/v1\"\n\
             [providers.openai]\nbase-url = \"{}\"\n",
            fixture.base_url
        ))
        .repository("[default]\nprovider = \"openai\"\nmodel = \"repo-model\"\n")
        .trusted();

    let output = machine
        .command()
        .env("OPERATOR_ANTHROPIC_KEY", operator_key)
        .args([
            "run",
            "--json",
            "--no-session",
            "--input",
            "read the readme",
            "--workspace",
            machine.workspace().to_str().expect("UTF-8"),
            "--api-key-env",
            "OPERATOR_ANTHROPIC_KEY",
        ])
        .output()
        .expect("the binary runs");

    let bearer = "Bearer ".len() + operator_key.len();
    let carried: Vec<Value> = fixture
        .requests()
        .into_iter()
        .filter(|request| request["authorization_length"] == bearer)
        .collect();
    assert!(
        carried.is_empty(),
        "the operator's typed Anthropic key reached the endpoint the repository's `provider` \
         chose, {} time(s); exit {:?}\nstdout: {}\nstderr: {}",
        carried.len(),
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// A2. A trusted repository's `.b10x/harness.toml` that is a symlink to a file of the operator's
/// (a credential store, say) must not have that file's bytes printed back in the refusal. The
/// safety envelope: a refusal never names a credential's value.
#[cfg(unix)]
#[test]
fn a_refused_repository_profile_does_not_quote_the_file_a_symlink_led_it_to() {
    let machine = Machine::new();
    let secret = "sk-ant-oat01-ADVERSARY-SECRET-VALUE-0001";
    let store = machine.home().join(".claude").join(".credentials.json");
    fs::create_dir_all(store.parent().expect("a directory")).expect("a directory");
    fs::write(
        &store,
        format!("{{\"claudeAiOauth\":{{\"accessToken\":\"{secret}\",\"expiresAt\":1}}}}\n"),
    )
    .expect("a credential store");
    machine.repository_symlink_to(&store).trusted();

    let output = machine.run(&["--base-url", "http://127.0.0.1:1/v1", "--model", "m"]);

    assert_eq!(output.status, Some(1), "refused: {}", output.stderr);
    assert!(
        !output.stderr.contains(secret) && !output.stdout.contains(secret),
        "the refusal printed the credential the symlink pointed at\nstdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
}

/// A3. `<workspace>/.b10x/harness.toml` is "the one fixed place a repository may speak; nothing
/// walks up the tree or looks anywhere else" (`repository_profile`). A symlink there that leads
/// outside the workspace makes a file the repository does not contain decide the run, while the
/// record names the in-repository path as its source.
#[cfg(unix)]
#[test]
fn a_repository_profile_symlinked_out_of_the_workspace_is_not_applied() {
    let machine = Machine::new();
    let elsewhere = machine.root.path().join("elsewhere.toml");
    fs::write(&elsewhere, "[default]\nmodel = \"outside-model\"\n").expect("a file outside");
    let fixture = Fixture::start("text", machine.root.path().join("requests.jsonl"));
    machine.repository_symlink_to(&elsewhere).trusted();

    let output = machine.run(&[
        "--base-url",
        fixture.base_url.as_str(),
        "--api-key-env",
        "B10X_HARNESS_TEST_KEY",
    ]);

    let started = output.started();
    let model = started.as_ref().map(|started| started["model"].clone());
    assert_ne!(
        model,
        Some(Value::String("outside-model".to_owned())),
        "a file outside the workspace was applied as the repository's profile; started: {started:?}\n\
         stderr: {}",
        output.stderr
    );
}

/// A4. `untrust` that reports success must leave the repository untrusted, even when a `trust` of
/// another repository runs at the same moment. The record is read, changed and replaced with no
/// lock, so the `trust` that read the record before the `untrust` replaced it writes the
/// untrusted repository back.
#[test]
fn an_untrust_that_reported_success_is_not_undone_by_a_concurrent_trust() {
    let machine = Machine::new();
    machine.repository("[default]\nmodel = \"repo-model\"\n");
    let a = machine.workspace();
    let a_text = a.to_str().expect("UTF-8").to_owned();
    let canonical_a = a.canonicalize().expect("canonical").display().to_string();
    let mut resurrected = Vec::new();

    for round in 0..40 {
        let trusted = machine.invoke(&["profiles", "trust", &a_text]);
        assert_eq!(trusted.status, Some(0), "{}", trusted.stderr);
        let b = machine.root.path().join(format!("other-{round}"));
        fs::create_dir_all(&b).expect("another repository");

        let mut untrust = machine
            .command()
            .args(["profiles", "untrust", &a_text])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("untrust starts");
        let mut trust = machine
            .command()
            .args(["profiles", "trust", b.to_str().expect("UTF-8")])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("trust starts");
        let untrust_status = untrust.wait().expect("untrust ends");
        let _ = trust.wait().expect("trust ends");
        let mut said = String::new();
        if let Some(mut stdout) = untrust.stdout.take() {
            use std::io::Read as _;
            let _ = stdout.read_to_string(&mut said);
        }
        // Only a round whose `untrust` said it removed the entry is judged.
        if !untrust_status.success() || !said.starts_with("untrusted ") {
            continue;
        }

        let record = fs::read_to_string(machine.trust_record()).expect("a record");
        let parsed: toml::Table = toml::from_str(&record).expect("the record parses");
        let still = parsed["repositories"]
            .as_array()
            .expect("an array")
            .iter()
            .any(|path| path.as_str() == Some(canonical_a.as_str()));
        if still {
            resurrected.push(round);
        }
    }

    assert!(
        resurrected.is_empty(),
        "`untrust` exited 0 and the repository was still trusted afterwards in rounds {resurrected:?}"
    );
}
