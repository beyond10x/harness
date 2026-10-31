//! An MCP stdio endpoint that publishes the controlled endpoint's tools and never answers a call.
//!
//! Compiled by `tests/adversary_awaited_ports.rs` with the workspace toolchain. It answers
//! `initialize` and `tools/list` byte for byte as `mcp_endpoint.rs stdio` does, so the reviewed
//! snapshot digest matches, and on `tools/call` it creates the file named by its first argument and
//! then stays silent until its stdin closes: a tool that hangs.

use std::io::{self, BufRead as _, Write as _};

fn main() {
    let marker = std::env::args().nth(1).expect("a marker path");
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.expect("read MCP request");
        let Some(id) = json_rpc_id(&line) else {
            continue;
        };
        let response = if line.contains(r#""method":"initialize""#) {
            format!(
                r#"{{"jsonrpc":"2.0","id":{id},"result":{{"protocolVersion":"2026-07-28","capabilities":{{"tools":{{"listChanged":false}}}},"serverInfo":{{"name":"b10x-harness-controlled-mcp","version":"1"}}}}}}"#
            )
        } else if line.contains(r#""method":"tools/list""#) {
            format!(
                r#"{{"jsonrpc":"2.0","id":{id},"result":{{"tools":[{{"name":"read_issue","description":"Read a synthetic issue","inputSchema":{{"type":"object","properties":{{"id":{{"type":"string"}}}}}},"annotations":{{"readOnlyHint":false}}}},{{"name":"close_issue","description":"Close a synthetic issue","inputSchema":{{"type":"object","properties":{{"id":{{"type":"string"}}}}}},"annotations":{{"readOnlyHint":true}}}}]}}}}"#
            )
        } else if line.contains(r#""method":"tools/call""#) {
            std::fs::write(&marker, "called").expect("write the call marker");
            continue;
        } else {
            format!(
                r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":-32601,"message":"method not found"}}}}"#
            )
        };
        writeln!(stdout, "{response}").expect("write MCP response");
        stdout.flush().expect("flush MCP response");
    }
}

fn json_rpc_id(request: &str) -> Option<&str> {
    let rest = request.split_once(r#""id":"#)?.1.trim_start();
    if rest.starts_with('"') {
        let end = rest[1..].find('"')? + 2;
        Some(&rest[..end])
    } else {
        let end = rest.find([',', '}']).unwrap_or(rest.len());
        Some(rest[..end].trim())
    }
}
