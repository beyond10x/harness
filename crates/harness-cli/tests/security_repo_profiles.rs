//! Integrity conformance checks for repository profiles (story `repo-local-profiles`), written by
//! the wave's security review against the policy the story implements:
//!
//! > a repository profile needs explicit operator trust per repository path; it may carry plumbing
//! > keys only, never permission keys; precedence -p > repository profile > operator defaults for
//! > the keys it may carry.
//!
//! and against the coordinator's reason for refusing `[providers]` in such a file: an endpoint, and
//! with it the operator's credential, must not be pointed somewhere the operator did not choose.
//!
//! Every case runs on a machine of its own: `HOME`, `XDG_CONFIG_HOME` and `XDG_STATE_HOME` point
//! into one temporary directory, so no case reads or writes the operator's real configuration or
//! trust record. Endpoints are local listeners in this file; nothing leaves the machine.

use std::fs;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

/// A local endpoint that records the head of every request it is sent and answers `400`.
struct Capture {
    base_url: String,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Capture {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
        let port = listener.local_addr().expect("an address").port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut head = Vec::new();
                let mut byte = [0_u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    match stream.read(&mut byte) {
                        Ok(1) => head.push(byte[0]),
                        _ => break,
                    }
                }
                sink.lock()
                    .expect("not poisoned")
                    .push(String::from_utf8_lossy(&head).into_owned());
                let _ = stream.write_all(
                    b"HTTP/1.1 400 Bad Request\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                );
            }
        });
        Self {
            base_url: format!("http://127.0.0.1:{port}/v1"),
            seen,
        }
    }

    fn received(&self, needle: &str) -> bool {
        self.seen
            .lock()
            .expect("not poisoned")
            .iter()
            .any(|head| head.contains(needle))
    }

    fn requests(&self) -> usize {
        self.seen.lock().expect("not poisoned").len()
    }
}

struct Output {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

/// One operator's machine and one repository on it, inside one temporary directory.
struct Machine {
    root: tempfile::TempDir,
}

impl Machine {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("a temporary directory");
        fs::create_dir_all(root.path().join("work")).expect("a workspace");
        fs::write(root.path().join("work").join("README.md"), "hello\n").expect("a file");
        Self { root }
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn config_dir(&self) -> PathBuf {
        self.home().join("config").join("b10x")
    }

    fn workspace(&self) -> PathBuf {
        self.root.path().join("work")
    }

    fn profile_path(&self) -> PathBuf {
        self.workspace().join(".b10x").join("harness.toml")
    }

    fn operator(&self, text: &str) -> &Self {
        fs::create_dir_all(self.config_dir()).expect("a config directory");
        fs::write(self.config_dir().join("harness.toml"), text).expect("the operator's config");
        self
    }

    fn repository(&self, text: &str) -> &Self {
        fs::create_dir_all(self.workspace().join(".b10x")).expect("a .b10x directory");
        fs::write(self.profile_path(), text).expect("the repository profile");
        self
    }

    #[cfg(unix)]
    fn repository_link_to(&self, target: &Path) -> &Self {
        fs::create_dir_all(self.workspace().join(".b10x")).expect("a .b10x directory");
        std::os::unix::fs::symlink(target, self.profile_path()).expect("a symlink");
        self
    }

    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command
            .stdin(Stdio::null())
            .env("HOME", self.home())
            .env("XDG_CONFIG_HOME", self.home().join("config"))
            .env("XDG_STATE_HOME", self.home().join("state"))
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY");
        command
    }

    fn invoke(&self, arguments: &[&str]) -> Output {
        let output = self
            .command(BINARY)
            .args(arguments)
            .env("OPERATOR_TYPED_KEY", "typed-key-meant-for-claude-7f3a")
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
            "read the readme",
            "--workspace",
            workspace.to_str().expect("UTF-8"),
        ];
        arguments.extend_from_slice(extra);
        self.invoke(&arguments)
    }
}

/// The operator chose `claude` in their own `[default]` and typed the credential for it. A trusted
/// repository naming `provider = "openai"` must not be how that typed credential reaches the other
/// vendor's endpoint: the repository chose the endpoint, the operator chose the key for another.
#[test]
fn a_credential_the_operator_typed_never_reaches_the_endpoint_of_a_provider_the_repository_chose() {
    let claude = Capture::start();
    let openai = Capture::start();
    let operator = format!(
        "[default]\nprovider = \"claude\"\n\
         [providers.claude]\nbase-url = \"{}\"\n\
         [providers.openai]\nbase-url = \"{}\"\n",
        claude.base_url, openai.base_url
    );

    // Control: with no repository profile the typed key goes where the operator pointed it, so
    // the capture below is measuring something.
    let control = Machine::new();
    control.operator(&operator);
    let output = control.run(&["--api-key-env", "OPERATOR_TYPED_KEY"]);
    assert!(
        claude.received("typed-key-meant-for-claude-7f3a"),
        "control: the operator's own provider received the typed key; status {:?}\nstderr: {}",
        output.status,
        output.stderr
    );

    let machine = Machine::new();
    machine
        .operator(&operator)
        .repository("[default]\nprovider = \"openai\"\nmodel = \"repo-model\"\n")
        .trusted();
    let output = machine.run(&["--api-key-env", "OPERATOR_TYPED_KEY"]);

    assert!(
        !openai.received("typed-key-meant-for-claude-7f3a"),
        "the credential typed for the operator's `claude` was presented to the `openai` endpoint \
         the repository profile selected ({} request(s) there); status {:?}\nstdout: {}\nstderr: {}",
        openai.requests(),
        output.status,
        output.stdout,
        output.stderr
    );
}

/// Trust is per path, and a trusted repository's later commit can make its profile a symlink. A
/// file outside the workspace, here shaped like a credentials file, must not be read through it,
/// and what it holds must not be printed into a run's stderr (a CI log, for one).
#[cfg(unix)]
#[test]
fn a_trusted_repository_profile_linking_out_of_the_workspace_does_not_echo_what_it_links_to() {
    let machine = Machine::new();
    let outside = machine.root.path().join("outside");
    fs::create_dir_all(&outside).expect("a directory outside the workspace");
    let secret = outside.join("credentials");
    fs::write(
        &secret,
        "[default]\naws_access_key_id = AKIAFIXTURENOTAKEY0001\n",
    )
    .expect("a credentials-shaped file");
    machine.repository_link_to(&secret).trusted();

    let output = machine.run(&["--base-url", "http://127.0.0.1:1/v1", "--model", "m"]);

    assert!(
        !output.stderr.contains("AKIAFIXTURENOTAKEY0001")
            && !output.stdout.contains("AKIAFIXTURENOTAKEY0001"),
        "a file outside the workspace was read through the repository profile's symlink and \
         its contents printed; status {:?}\nstderr: {}",
        output.status,
        output.stderr
    );
}

/// A trusted profile that is a symlink to an endless device (storable in Git as a symlink) is
/// read with no bound. The run must refuse it by name, not grow until the allocator aborts.
#[cfg(target_os = "linux")]
#[test]
fn a_trusted_repository_profile_linked_to_an_endless_device_is_refused_by_name() {
    let machine = Machine::new();
    machine.repository_link_to(Path::new("/dev/zero")).trusted();
    let workspace = machine.workspace();

    // Address space capped at 2 GiB, so an unbounded read ends in an abort rather than in the
    // machine's memory; a wall clock of 60 s, so a read that never ends cannot hang the suite.
    let mut child = machine
        .command("sh")
        .arg("-c")
        .arg("ulimit -v 2097152 && exec \"$0\" \"$@\"")
        .arg(BINARY)
        .args([
            "run",
            "--json",
            "--no-session",
            "--input",
            "hello",
            "--workspace",
            workspace.to_str().expect("UTF-8"),
            "--base-url",
            "http://127.0.0.1:1/v1",
            "--model",
            "m",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("sh runs");
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("waits") {
            break Some(status);
        }
        if started.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }

    let status = status.unwrap_or_else(|| panic!("the run was still reading after 60 s: {stderr}"));
    assert_eq!(
        status.code(),
        Some(1),
        "a refusal by name, not an abort ({status:?}); stderr: {stderr}"
    );
    assert!(stderr.contains(".b10x/harness.toml"), "{stderr}");
}

/// `untrust` must stick. A `trust` of another repository running at the same moment loads the
/// record before the removal and writes it back after, which re-trusts what was just untrusted.
#[test]
fn untrusting_a_repository_is_not_undone_by_a_concurrent_trust_of_another() {
    use b10x_harness_cli::trust;

    let root = tempfile::tempdir().expect("a temporary directory");
    let record = root
        .path()
        .join("config")
        .join("b10x")
        .join(trust::TRUST_FILE);
    let target = root.path().join("target-repo");
    fs::create_dir_all(&target).expect("a repository");
    let canonical = target.canonicalize().expect("canonical");

    for round in 0..200 {
        trust::trust(&target, &record).expect("trusted");
        let others: Vec<PathBuf> = (0..4)
            .map(|index| {
                let other = root.path().join(format!("other-{round}-{index}"));
                fs::create_dir_all(&other).expect("another repository");
                other
            })
            .collect();
        let barrier = Arc::new(std::sync::Barrier::new(others.len() + 1));
        let mut handles = Vec::new();
        for other in others {
            let (barrier, record) = (Arc::clone(&barrier), record.clone());
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                let _ = trust::trust(&other, &record);
            }));
        }
        barrier.wait();
        let removed = trust::untrust(&target, &record).expect("untrusted");
        assert!(removed.changed, "round {round}: it was trusted");
        for handle in handles {
            handle.join().expect("joins");
        }
        assert!(
            !trust::load(&record).expect("reads").admits(&canonical),
            "round {round}: `untrust` reported success and the repository is trusted again"
        );
    }
}
