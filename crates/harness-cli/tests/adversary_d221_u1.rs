//! Adversary pass 1 on `story:substrate-refusal-display-escapes-daemon-bytes` (wave d221).
//!
//! `SubstrateError::Refused`'s Display now escapes the daemon's body, but the CLI's adopt error
//! mapping prints `body` itself for `status: 0`, the arm meant for the client's own refusals. The
//! client parses the status line with `parse::<u16>()`, so a program on the socket that answers
//! `HTTP/1.1 0` lands in that arm, and its body reaches stderr raw.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

/// ESC, C1 CSI, DEL, a line break and the right-to-left override.
const HOSTILE: [char; 5] = ['\u{1b}', '\u{9b}', '\u{7f}', '\n', '\u{202e}'];

fn answer(stream: &mut std::os::unix::net::UnixStream) {
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
    let (status_line, body) = if request_line.contains("/v1/machine") {
        ("HTTP/1.1 200 OK".to_owned(), "{}".to_owned())
    } else {
        (
            "HTTP/1.1 0 X".to_owned(),
            format!(
                "\u{1b}]0;owned\u{7}{}x",
                HOSTILE.iter().collect::<String>()
            ),
        )
    };
    let response = format!(
        "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).expect("answer");
}

#[test]
fn a_daemon_answering_status_zero_to_an_adopt_does_not_reach_stderr_raw() {
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
        .arg("--substrate-adopt")
        .arg("project")
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("--substrate-adopt project"),
        "the adopt was not the refusal: {stderr:?}"
    );
    let raw: Vec<char> = stderr
        .trim_end_matches('\n')
        .chars()
        .filter(|c| HOSTILE.contains(c) || *c == '\u{7}')
        .collect();
    assert!(raw.is_empty(), "raw {raw:?} in {stderr:?}");
}
