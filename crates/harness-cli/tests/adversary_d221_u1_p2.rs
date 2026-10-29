//! Adversary pass 2 on `story:substrate-refusal-display-escapes-daemon-bytes` (wave d221).
//!
//! The correction's CHANGELOG line says no byte a substrate daemon answered reaches the terminal
//! raw any more. `Facts::withheld` quotes a daemon's fact value with `serde_json`'s Display
//! (`withheld.rs`, `describe`), which escapes only U+0000..U+001F, and `tools` prints that reason
//! on stderr through `render::withheld_line`. DEL, C1 CSI and U+202E reach the operator raw.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::Command;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

/// C1 CSI, DEL and the right-to-left override: what `serde_json` leaves raw.
const HOSTILE: [char; 3] = ['\u{9b}', '\u{7f}', '\u{202e}'];

fn answer(stream: &mut UnixStream) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut request_line = String::new();
    reader.read_line(&mut request_line).expect("request line");
    let mut length = 0_usize;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).expect("header");
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().expect("length");
        }
    }
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body).expect("body");
    let hostile: String = HOSTILE.iter().collect::<String>() + "x";
    let body = serde_json::json!({"result": {
        "driver": "host",
        "facts": {"exec.argv-only": hostile, "workspace.guarded-io": true},
        "snapshot": "snap_1"
    }})
    .to_string();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).expect("answer");
}

#[test]
fn a_hostile_fact_in_a_withheld_note_does_not_reach_stderr_raw() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("d.sock");
    let workspace = dir.path().join("project");
    std::fs::create_dir(&workspace).expect("workspace");
    let listener = UnixListener::bind(&socket).expect("bind");
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            answer(&mut stream);
        }
    });

    let output = Command::new(BINARY)
        .arg("tools")
        .arg("--workspace")
        .arg(&workspace)
        .arg("--substrate")
        .arg(&socket)
        .arg("--workspace-id")
        .arg("ws_a")
        .arg("--allow-program")
        .arg("cargo")
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("exec.argv-only"),
        "the withheld note was not printed: {stderr:?}"
    );
    let raw: Vec<char> = stderr.chars().filter(|c| HOSTILE.contains(c)).collect();
    assert!(raw.is_empty(), "raw {raw:?} in {stderr:?}");
}
