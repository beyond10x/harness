//! Rust-only controlled endpoints for the outbound MCP end-to-end tests.
//!
//! This source is compiled by the integration test with the workspace toolchain. It deliberately
//! uses only the standard library so it proves the released binary against sockets and pipes
//! without introducing a shipped fixture binary or another runtime language.

use std::io::{self, BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};

const OAUTH_TOKEN: &str = "fixture-oauth-access-token";

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("stdio") => serve_stdio(),
        Some("http") => serve_http(Endpoint::Mcp),
        Some("provider") => serve_http(Endpoint::Provider),
        _ => panic!("expected one of: stdio, http, provider"),
    }
}

fn serve_stdio() {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.expect("read MCP request");
        if let Some(response) = mcp_response(&line) {
            writeln!(stdout, "{response}").expect("write MCP response");
            stdout.flush().expect("flush MCP response");
        }
    }
}

#[derive(Clone, Copy)]
enum Endpoint {
    Mcp,
    Provider,
}

fn serve_http(endpoint: Endpoint) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind controlled endpoint");
    let address = listener.local_addr().expect("controlled endpoint address");
    let suffix = match endpoint {
        Endpoint::Mcp => "/mcp",
        Endpoint::Provider => "/v1",
    };
    println!(r#"{{"url":"http://{address}{suffix}"}}"#);
    io::stdout().flush().expect("announce controlled endpoint");

    let mut discovery = DiscoveryState::default();
    for stream in listener.incoming() {
        let mut stream = stream.expect("accept controlled request");
        let request = read_request(&mut stream);
        match endpoint {
            Endpoint::Mcp => serve_mcp_http(&mut stream, &request, &mut discovery),
            Endpoint::Provider => serve_provider(&mut stream, &request),
        }
    }
}

#[derive(Default)]
struct DiscoveryState {
    protected_resource: bool,
    authorization_server: bool,
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

fn read_request(stream: &mut TcpStream) -> Request {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read request line");
    let mut request_line = line.split_ascii_whitespace();
    let method = request_line.next().expect("request method").to_owned();
    let path = request_line.next().expect("request path").to_owned();
    let mut headers = Vec::new();
    let mut content_length = 0;
    loop {
        line.clear();
        reader.read_line(&mut line).expect("read request header");
        if line == "\r\n" {
            break;
        }
        let (name, value) = line.trim_end().split_once(':').expect("valid request header");
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse().expect("numeric content length");
        }
        headers.push((name.to_ascii_lowercase(), value.trim().to_owned()));
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body).expect("read request body");
    Request {
        method,
        path,
        headers,
        body: String::from_utf8(body).expect("request body is UTF-8"),
    }
}

fn serve_mcp_http(
    stream: &mut TcpStream,
    request: &Request,
    discovery: &mut DiscoveryState,
) {
    if request.method == "GET" {
        serve_oauth_discovery(stream, request, discovery);
        return;
    }
    assert_eq!(request.method, "POST", "unexpected MCP HTTP method");
    let authorized = request.headers.iter().any(|(name, value)| {
        name == "authorization" && value == &format!("Bearer {OAUTH_TOKEN}")
    });
    if !authorized || !discovery.protected_resource || !discovery.authorization_server {
        respond(
            stream,
            "401 Unauthorized",
            "application/json",
            r#"{"error":"OAuth discovery or bearer missing"}"#,
        );
        return;
    }
    match mcp_response(&request.body) {
        Some(body) => respond(stream, "200 OK", "application/json", &body),
        None => respond(stream, "202 Accepted", "application/json", ""),
    }
}

fn serve_oauth_discovery(
    stream: &mut TcpStream,
    request: &Request,
    discovery: &mut DiscoveryState,
) {
    let host = request
        .headers
        .iter()
        .find_map(|(name, value)| (name == "host").then_some(value))
        .expect("OAuth discovery Host header");
    let origin = format!("http://{host}");
    match request.path.as_str() {
        "/mcp" => respond_with_headers(
            stream,
            "401 Unauthorized",
            "application/json",
            "",
            &[(
                "WWW-Authenticate",
                format!(
                    r#"Bearer resource_metadata="{origin}/.well-known/oauth-protected-resource""#
                ),
            )],
        ),
        "/.well-known/oauth-protected-resource"
        | "/.well-known/oauth-protected-resource/mcp" => {
            discovery.protected_resource = true;
            respond(
                stream,
                "200 OK",
                "application/json",
                &format!(
                    r#"{{"resource":"{origin}/mcp","authorization_servers":["{origin}"],"scopes_supported":["mcp.read"]}}"#
                ),
            )
        }
        "/.well-known/oauth-authorization-server" => {
            discovery.authorization_server = true;
            respond(
                stream,
                "200 OK",
                "application/json",
                &format!(
                    r#"{{"issuer":"{origin}","authorization_endpoint":"{origin}/authorize","token_endpoint":"{origin}/token","scopes_supported":["mcp.read"],"response_types_supported":["code"],"grant_types_supported":["authorization_code","refresh_token"],"code_challenge_methods_supported":["S256"]}}"#
                ),
            )
        }
        _ => respond(stream, "404 Not Found", "application/json", "{}"),
    }
}

fn mcp_response(request: &str) -> Option<String> {
    let id = json_rpc_id(request)?;
    if request.contains(r#""method":"initialize""#) {
        Some(format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{{"protocolVersion":"2026-07-28","capabilities":{{"tools":{{"listChanged":false}}}},"serverInfo":{{"name":"b10x-harness-controlled-mcp","version":"1"}}}}}}"#
        ))
    } else if request.contains(r#""method":"tools/list""#) {
        Some(format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{{"tools":[{{"name":"read_issue","description":"Read a synthetic issue","inputSchema":{{"type":"object","properties":{{"id":{{"type":"string"}}}}}},"annotations":{{"readOnlyHint":false}}}},{{"name":"close_issue","description":"Close a synthetic issue","inputSchema":{{"type":"object","properties":{{"id":{{"type":"string"}}}}}},"annotations":{{"readOnlyHint":true}}}}]}}}}"#
        ))
    } else if request.contains(r#""method":"tools/call""#) {
        Some(format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{{"content":[{{"type":"text","text":"controlled issue is open"}}],"isError":false}}}}"#
        ))
    } else {
        Some(format!(
            r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":-32601,"message":"method not found"}}}}"#
        ))
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

fn serve_provider(stream: &mut TcpStream, request: &Request) {
    let body = if request.body.contains(r#""type":"function_call_output""#) {
        text_events("provider emulation passed through MCP")
    } else {
        call_events()
    };
    respond(stream, "200 OK", "text/event-stream", &body);
}

fn call_events() -> String {
    let item = r#"{"id":"fc_b10x_001","type":"function_call","status":"completed","name":"mcp_fixture_read_issue","call_id":"call_b10x_001","arguments":"{\"id\":\"ISSUE-7\"}"}"#;
    let added = r#"{"id":"fc_b10x_001","type":"function_call","status":"in_progress","name":"mcp_fixture_read_issue","call_id":"call_b10x_001","arguments":""}"#;
    sse(&[
        r#"{"type":"response.created","response":{"id":"resp_b10x_001","object":"response","created_at":1786706400,"status":"in_progress","model":"b10x-emulated","output":[],"incomplete_details":null,"error":null,"usage":null}}"#,
        &format!(r#"{{"type":"response.output_item.added","output_index":0,"item":{added}}}"#),
        r#"{"type":"response.function_call_arguments.delta","item_id":"fc_b10x_001","output_index":0,"delta":"{\"id\":\"ISSUE-7\"}"}"#,
        &format!(r#"{{"type":"response.output_item.done","output_index":0,"item":{item}}}"#),
        &format!(r#"{{"type":"response.completed","response":{{"id":"resp_b10x_001","object":"response","created_at":1786706400,"status":"completed","model":"b10x-emulated","output":[{item}],"incomplete_details":null,"error":null,"usage":{{"input_tokens":42,"input_tokens_details":{{"cached_tokens":7}},"output_tokens":8,"output_tokens_details":{{"reasoning_tokens":0}},"total_tokens":50}}}}}}"#),
    ])
}

fn text_events(text: &str) -> String {
    let item = format!(
        r#"{{"id":"msg_b10x_001","type":"message","status":"completed","role":"assistant","content":[{{"type":"output_text","text":"{text}","annotations":[]}}]}}"#
    );
    sse(&[
        r#"{"type":"response.created","response":{"id":"resp_b10x_001","object":"response","created_at":1786706400,"status":"in_progress","model":"b10x-emulated","output":[],"incomplete_details":null,"error":null,"usage":null}}"#,
        r#"{"type":"response.output_item.added","output_index":0,"item":{"id":"msg_b10x_001","type":"message","status":"in_progress","role":"assistant","content":[]}}"#,
        &format!(r#"{{"type":"response.output_text.delta","item_id":"msg_b10x_001","output_index":0,"delta":"{text}"}}"#),
        &format!(r#"{{"type":"response.output_item.done","output_index":0,"item":{item}}}"#),
        &format!(r#"{{"type":"response.completed","response":{{"id":"resp_b10x_001","object":"response","created_at":1786706400,"status":"completed","model":"b10x-emulated","output":[{item}],"incomplete_details":null,"error":null,"usage":{{"input_tokens":42,"input_tokens_details":{{"cached_tokens":7}},"output_tokens":4,"output_tokens_details":{{"reasoning_tokens":0}},"total_tokens":46}}}}}}"#),
    ])
}

fn sse(events: &[&str]) -> String {
    events
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect()
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &str) {
    respond_with_headers(stream, status, content_type, body, &[]);
}

fn respond_with_headers(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
    headers: &[(&str, String)],
) {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n",
        body.len()
    )
    .expect("write controlled response");
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n").expect("write controlled response header");
    }
    write!(stream, "\r\n{body}").expect("write controlled response body");
    stream.flush().expect("flush controlled response");
}
