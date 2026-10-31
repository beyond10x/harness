//! Fixtures shared by more than one integration test of this package.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The Rust-only controlled endpoint (`tests/fixtures/mcp_endpoint.rs`), compiled once per test
/// binary with the workspace toolchain.
pub fn controlled_fixture_binary() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            let source =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp_endpoint.rs");
            // Under the target directory, not `$TMPDIR`: a `TempDir` held by this static is never
            // dropped, so a directory under `$TMPDIR` would outlive every run
            // (story:end-to-end-suite-leaves-no-fixture-binary). `cargo clean` removes this one.
            let directory = tempfile::Builder::new()
                .prefix("end-to-end-mcp-endpoint-")
                .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
                .expect("a fixture build directory")
                .keep();
            let binary = directory.join("b10x-harness-mcp-endpoint");
            let output = Command::new("rustc")
                .args(["--edition=2024", "-C", "debuginfo=0", "-o"])
                .arg(&binary)
                .arg(&source)
                .output()
                .expect("the workspace Rust compiler runs");
            assert!(
                output.status.success(),
                "controlled endpoint compiles: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            binary
        })
        .as_path()
}
