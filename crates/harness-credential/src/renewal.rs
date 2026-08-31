//! Renewing a subscription token, and writing the new one back where its owner keeps it.
//!
//! # What changed, and what it costs
//!
//! [`crate::SubscriptionToken`]'s own doc used to say this component does not refresh, because
//! "renewing it means holding a refresh token and calling an authorization server — custody this
//! component has not been given". It has been given it, deliberately, and the price is stated
//! rather than waved away:
//!
//! - **The harness now reads a field of somebody else's credential file that it does not send.**
//!   A refresh token is a longer-lived secret than the access token beside it. It leaves this
//!   process exactly once, in the body of one POST to the authorization server the caller named,
//!   and reaches no log, no error message, no event and no session record.
//! - **The harness now writes to a vendor's credential store.** That is a side effect on a file
//!   another program owns, so it is atomic (§ *Writing it back*), it preserves every byte it did
//!   not have to change, and it is announced: the run's record carries a `credential-renewed`
//!   event naming the file and the new expiry.
//!
//! Nothing here looks for a credential. The document, every pointer into it, the token endpoint
//! and the client id all arrive from the caller — the same rule the rest of this crate is held to.
//!
//! # Deciding that a token is stale
//!
//! The access token is read as a JWT and its `exp` claim is decoded. **The signature is not
//! verified, and it does not need to be**: this is not authenticating anybody, it is asking the
//! credential when it expects to stop working, and the authority on that answer is the server that
//! will refuse it. A token whose `exp` cannot be read is left alone — guessing that an opaque
//! credential is stale would spend a refresh token to replace something that works.
//!
//! # Writing it back
//!
//! Two rules, and the second is why this is not four lines of `serde_json`:
//!
//! 1. **Atomic.** The new document is written to a temporary file beside the original, verified by
//!    being parsed back, and only then renamed over it. A crash between the two leaves the old
//!    document intact; a half-written credential file would lock its owner out of their own
//!    account.
//! 2. **Byte-preserving where it can be.** The replaced values are spliced into the original text,
//!    so every other byte — key order, indentation, keys this build has never heard of — survives
//!    exactly. Parsing and re-serialising would preserve the *content* and reorder the file, and
//!    this file belongs to another program. Where a splice cannot be proven safe the re-serialised
//!    form is used instead, and [`Renewed::byte_preserving`] says which happened.

use std::path::{Path, PathBuf};
use std::time::Duration;

use harness_http::{FailureBody, JsonExchange, JsonPost};
use serde_json::{Value, json};

/// Where a credential store keeps the things a renewal reads and writes.
///
/// Every field is a pointer the **caller** supplies. Which key a given store puts its refresh
/// token under is that store's business, and a built-in answer here would silently read the wrong
/// field the day it changed — the reason [`crate::SubscriptionToken`] takes its access pointer the
/// same way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthDocument {
    /// The JSON document holding the tokens.
    pub path: PathBuf,
    /// RFC 6901 pointer to the access token: the one presented to the model endpoint.
    pub access_pointer: String,
    /// RFC 6901 pointer to the refresh token: the one presented to the authorization server.
    pub refresh_pointer: String,
    /// Pointer to an id token, when the store keeps one. Rewritten with the rest so the document
    /// does not end up describing two different sessions.
    pub id_token_pointer: Option<String>,
    /// Pointer to the store's own "when was this last renewed" stamp, when it keeps one.
    pub renewed_at_pointer: Option<String>,
}

/// The authorization server a refresh token is presented to.
///
/// No secret of its own: a public OAuth client id is public, and this carries no client secret
/// because the flow these tokens come from does not use one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenEndpoint {
    /// The absolute URL of the token endpoint.
    pub url: String,
    /// The OAuth client the credential was issued to.
    pub client_id: String,
}

/// What one renewal did. **Never the token, and never anything derived from it.**
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renewed {
    /// When the new access token expires, from its own `exp`. [`None`] when it is not a JWT.
    pub expires_unix: Option<u64>,
    /// Whether the server issued a new refresh token, retiring the one on disk.
    ///
    /// **Worth recording.** When it is true, a backup of this file taken a minute ago no longer
    /// holds a credential that works, so restoring one is not the recovery it looks like.
    pub refresh_token_rotated: bool,
    /// Whether every byte outside the replaced values survived. See the module's § *Writing it
    /// back*: `false` is a correct document written in serde's key order rather than its owner's.
    pub byte_preserving: bool,
}

/// Renews the token in `document` when it is within `margin` of expiring, and writes it back.
///
/// `now_unix` and `renewed_at` are the caller's — this crate has no clock and does no date
/// arithmetic, so two callers cannot disagree about what time it is.
///
/// Returns [`None`] when the token is not stale, which is the ordinary case: nothing was sent,
/// nothing was written, and the run proceeds on the credential it already had.
///
/// # Errors
///
/// Names the document it could not read, the pointer that led nowhere, or what the authorization
/// server answered. **No error carries a token**: the refresh token appears in exactly one place,
/// the body of the POST, and the failure messages name the endpoint instead.
pub fn renew_if_stale(
    document: &AuthDocument,
    endpoint: &TokenEndpoint,
    now_unix: u64,
    margin: Duration,
    renewed_at: &str,
) -> Result<Option<Renewed>, String> {
    let text = std::fs::read_to_string(&document.path)
        .map_err(|error| format!("reading `{}`: {error}", document.path.display()))?;
    let parsed: Value = serde_json::from_str(&text)
        .map_err(|error| format!("`{}` is not JSON: {error}", document.path.display()))?;
    let access = string_at(&parsed, &document.access_pointer)
        .ok_or_else(|| pointed_nowhere(document, &document.access_pointer))?;
    if !is_stale(expiry_of(access), now_unix, margin) {
        return Ok(None);
    }
    let refresh = string_at(&parsed, &document.refresh_pointer)
        .ok_or_else(|| pointed_nowhere(document, &document.refresh_pointer))?;

    // The one place the refresh token leaves this process. `scope` is deliberately absent: RFC 6749
    // § 6 says an omitted scope means the scope originally granted, and naming one here would be
    // this build deciding what the operator's own credential is allowed to do.
    let request = json!({
        "client_id": endpoint.client_id,
        "grant_type": "refresh_token",
        "refresh_token": refresh,
    });
    let answer = JsonExchange::new()
        .map_err(|error| error.message)?
        .post(&JsonPost {
            who: &format!("the authorization server at {}", endpoint.url),
            url: &endpoint.url,
            body: &request,
            failure_body: FailureBody::Omit,
        })
        .map_err(|error| error.message)?;

    let new_access = answer
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "the authorization server at {} answered without an `access_token`",
                endpoint.url
            )
        })?;
    let new_refresh = answer
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    if answer.get("refresh_token").is_some() && new_refresh.is_none() {
        return Err(format!(
            "the authorization server at {} answered with an empty or non-string refresh token",
            endpoint.url
        ));
    }
    let new_id_token = answer
        .get("id_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    if answer.get("id_token").is_some() && new_id_token.is_none() {
        return Err(format!(
            "the authorization server at {} answered with an empty or non-string id token",
            endpoint.url
        ));
    }

    // Ordered so that the values most likely to be unique are spliced first; every one of them is
    // checked for uniqueness anyway, so the order is a readability choice and not a correctness
    // one.
    let mut edits: Vec<(&str, &str, &str)> = vec![(&document.access_pointer, access, new_access)];
    if let Some(new_refresh) = new_refresh
        && new_refresh != refresh
    {
        edits.push((&document.refresh_pointer, refresh, new_refresh));
    }
    if let (Some(pointer), Some(new_id_token)) = (&document.id_token_pointer, new_id_token)
        && let Some(old) = string_at(&parsed, pointer)
    {
        edits.push((pointer, old, new_id_token));
    }
    if let Some(pointer) = &document.renewed_at_pointer
        && let Some(old) = string_at(&parsed, pointer)
    {
        edits.push((pointer, old, renewed_at));
    }

    let (rewritten, byte_preserving) = rewrite(&text, &parsed, &edits)?;
    verify(&rewritten, &edits, document)?;
    write_atomically_if_unchanged(&document.path, &text, &rewritten)?;

    Ok(Some(Renewed {
        expires_unix: expiry_of(new_access),
        refresh_token_rotated: new_refresh.is_some_and(|new| new != refresh),
        byte_preserving,
    }))
}

/// Whether a token with this expiry should be renewed now.
///
/// A token whose expiry could not be read is **not** stale. Guessing would spend a refresh token
/// to replace a credential that works, and the one authority on whether an opaque token is still
/// good is the server that will refuse it.
#[must_use]
pub fn is_stale(expires_unix: Option<u64>, now_unix: u64, margin: Duration) -> bool {
    expires_unix.is_some_and(|expiry| expiry.saturating_sub(margin.as_secs()) <= now_unix)
}

/// The `exp` claim of a JWT, without verifying its signature.
///
/// **Not authentication.** This asks the credential when it expects to stop working so the run can
/// renew it beforehand; the far side remains the only thing that decides whether a token is good.
/// A value that is not a three-part JWT, or whose payload carries no numeric `exp`, answers
/// [`None`] — and [`is_stale`] treats that as *leave it alone*.
#[must_use]
pub fn expiry_of(token: &str) -> Option<u64> {
    let mut parts = token.split('.');
    let (_header, payload) = (parts.next()?, parts.next()?);
    parts.next()?;
    let claims: Value = serde_json::from_slice(&base64url(payload)?).ok()?;
    claims.get("exp")?.as_u64()
}

/// Decodes unpadded base64url, which is what a JWT's segments are.
///
/// Written out rather than taken from a crate, for the reason `environment::civil_from_days` is in
/// the CLI: it is twenty lines with a fixed alphabet, and a dependency added for it would be a
/// dependency in the tree of a binary that handles credentials.
fn base64url(segment: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(segment.len() * 3 / 4);
    let mut accumulator: u32 = 0;
    let mut bits = 0_u32;
    for byte in segment.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            // Padding is legal but optional in a JWT, and nothing follows it.
            b'=' => break,
            _ => return None,
        };
        accumulator = (accumulator << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the shift leaves exactly the low eight bits"
            )]
            out.push((accumulator >> bits) as u8);
        }
    }
    Some(out)
}

/// The string at `pointer`, or [`None`] when nothing string-shaped is there.
fn string_at<'a>(document: &'a Value, pointer: &str) -> Option<&'a str> {
    document.pointer(pointer).and_then(Value::as_str)
}

fn pointed_nowhere(document: &AuthDocument, pointer: &str) -> String {
    format!(
        "`{}` holds no string at `{pointer}`, so there is nothing here to renew",
        document.path.display()
    )
}

/// The document with each edit applied, and whether every other byte survived.
///
/// Tries the splice first — see the module's § *Writing it back* — and falls back to re-serialising
/// the parsed document when any one of them cannot be proven safe. The fallback is correct and
/// reorders keys; it is not silent, because the caller reports which happened.
fn rewrite(
    text: &str,
    parsed: &Value,
    edits: &[(&str, &str, &str)],
) -> Result<(String, bool), String> {
    let mut spliced = text.to_owned();
    let mut ok = true;
    for (_, old, new) in edits {
        let Some(next) = splice(&spliced, old, new) else {
            ok = false;
            break;
        };
        spliced = next;
    }
    if ok {
        return Ok((spliced, true));
    }
    let mut rebuilt = parsed.clone();
    for (pointer, _, new) in edits {
        let slot = rebuilt
            .pointer_mut(pointer)
            .ok_or_else(|| format!("`{pointer}` is no longer in the document being rewritten"))?;
        *slot = Value::String((*new).to_owned());
    }
    let mut rendered = serde_json::to_string_pretty(&rebuilt)
        .map_err(|error| format!("rewriting the credential document: {error}"))?;
    rendered.push('\n');
    Ok((rendered, false))
}

/// Replaces `old` with `new`, leaving every other byte exactly as it was.
///
/// [`None`] rather than a guess whenever the replacement cannot be proven to keep the document
/// valid JSON: either value needing an escape, or `old` appearing anywhere other than exactly once.
/// A token that occurs twice is a document this build does not understand well enough to edit
/// textually, and the caller has a correct fallback.
fn splice(document: &str, old: &str, new: &str) -> Option<String> {
    if old.is_empty() || !is_plain(old) || !is_plain(new) {
        return None;
    }
    let mut found = document.match_indices(old);
    let (at, _) = found.next()?;
    if found.next().is_some() {
        return None;
    }
    let mut out = String::with_capacity(document.len() - old.len() + new.len());
    out.push_str(&document[..at]);
    out.push_str(new);
    out.push_str(&document[at + old.len()..]);
    Some(out)
}

/// Whether a string appears in JSON exactly as it reads, with no escape sequence involved.
fn is_plain(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| !character.is_control() && character != '"' && character != '\\')
}

/// Parses the rewritten document and checks each edit landed where it was aimed.
///
/// The guard against a splice that produced something that reads as JSON but says the wrong thing.
/// It runs before the rename, so a document that fails it never reaches the path its owner reads.
fn verify(
    rewritten: &str,
    edits: &[(&str, &str, &str)],
    document: &AuthDocument,
) -> Result<(), String> {
    let parsed: Value = serde_json::from_str(rewritten).map_err(|error| {
        format!(
            "the rewritten `{}` would not have been JSON, so it was not written: {error}",
            document.path.display()
        )
    })?;
    for (pointer, _, new) in edits {
        if string_at(&parsed, pointer) != Some(*new) {
            return Err(format!(
                "the rewritten `{}` does not carry the new value at `{pointer}`, so it was not \
                 written",
                document.path.display()
            ));
        }
    }
    Ok(())
}

/// Writes `contents` over `path` without ever leaving a half-written document there.
///
/// A temporary file in the **same directory** — so the rename is within one filesystem and is
/// therefore atomic — carrying the original's permissions, flushed to disk before the rename. A
/// credential file caught half-written is its owner locked out of their own account.
#[cfg(test)]
fn write_atomically(path: &Path, contents: &str) -> Result<(), String> {
    write_atomically_if_unchanged(
        path,
        &std::fs::read_to_string(path).map_err(|error| {
            format!(
                "reading the current `{}` before writing it: {error}",
                path.display()
            )
        })?,
        contents,
    )
}

/// Atomically replaces `path` only when it still contains the bytes the caller read.
///
/// The network round trip occurs before this function. Re-reading immediately before the rename
/// prevents a concurrent owner or another renewal from being overwritten with a document derived
/// from stale bytes.
fn write_atomically_if_unchanged(
    path: &Path,
    expected: &str,
    contents: &str,
) -> Result<(), String> {
    use std::io::Write as _;

    let current = std::fs::read_to_string(path).map_err(|error| {
        format!(
            "re-reading `{}` before replacing it: {error}",
            path.display()
        )
    })?;
    if current != expected {
        return Err(format!(
            "`{}` changed while its renewal request was in flight, so the newer document was not overwritten",
            path.display()
        ));
    }

    let directory = path.parent().ok_or_else(|| {
        format!(
            "`{}` has no parent directory to write beside",
            path.display()
        )
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory).map_err(|error| {
        format!(
            "creating a temporary file beside `{}`: {error}",
            path.display()
        )
    })?;
    #[cfg(unix)]
    {
        // The original's mode, not a fresh one. `NamedTempFile` starts at 0600, which happens to be
        // right for the file this was written for — but *happens to be* is not a rule, and a store
        // that is group-readable on purpose must not be narrowed by being renewed.
        let mode = std::fs::metadata(path)
            .map_err(|error| format!("reading the mode of `{}`: {error}", path.display()))?
            .permissions();
        temporary.as_file().set_permissions(mode).map_err(|error| {
            format!("setting the mode of the new `{}`: {error}", path.display())
        })?;
    }
    temporary
        .write_all(contents.as_bytes())
        .and_then(|()| temporary.as_file_mut().sync_all())
        .map_err(|error| format!("writing the new `{}`: {error}", path.display()))?;
    temporary
        .persist(path)
        .map_err(|error| format!("replacing `{}`: {error}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A JWT with the given `exp`, signed by nothing. The signature is never read.
    fn jwt(exp: u64) -> String {
        fn segment(bytes: &[u8]) -> String {
            const ALPHABET: &[u8] =
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
            let mut out = String::new();
            for chunk in bytes.chunks(3) {
                let mut buffer = [0_u8; 3];
                buffer[..chunk.len()].copy_from_slice(chunk);
                let packed = (u32::from(buffer[0]) << 16)
                    | (u32::from(buffer[1]) << 8)
                    | u32::from(buffer[2]);
                for index in 0..=chunk.len() {
                    out.push(char::from(
                        ALPHABET[((packed >> (18 - 6 * index)) & 0x3f) as usize],
                    ));
                }
            }
            out
        }
        format!(
            "{}.{}.{}",
            segment(br#"{"alg":"none"}"#),
            segment(format!(r#"{{"exp":{exp},"sub":"synthetic"}}"#).as_bytes()),
            "synthetic-signature-never-read"
        )
    }

    #[test]
    fn an_expiry_is_read_out_of_the_token_without_verifying_anything() {
        assert_eq!(expiry_of(&jwt(1_788_871_151)), Some(1_788_871_151));
    }

    #[test]
    fn a_token_whose_expiry_cannot_be_read_is_left_alone() {
        // The important half. An opaque credential that this build cannot date is not evidence
        // that it is stale, and treating it as stale would spend a refresh token on a working one.
        assert_eq!(expiry_of("synthetic-not-a-jwt"), None);
        assert_eq!(expiry_of("a.b.c"), None);
        assert!(!is_stale(None, 1_788_000_000, Duration::from_mins(15)));
    }

    #[test]
    fn staleness_is_decided_against_the_margin_and_not_against_the_instant_of_expiry() {
        let margin = Duration::from_mins(15);
        let expiry = 1_788_000_000;
        assert!(
            !is_stale(Some(expiry), expiry - 901, margin),
            "a token with more than the margin left is not touched"
        );
        assert!(
            is_stale(Some(expiry), expiry - 899, margin),
            "a token inside the margin is renewed before a turn can fail on it"
        );
        assert!(
            is_stale(Some(expiry), expiry + 1, margin),
            "and an expired one certainly is"
        );
    }

    #[test]
    fn a_splice_replaces_one_value_and_leaves_every_other_byte_alone() {
        let document = "{\n  \"b\": \"old\",\n  \"a\": {\"keep\": 1}\n}\n";
        let spliced = splice(document, "old", "new").expect("a safe splice");
        assert_eq!(spliced, "{\n  \"b\": \"new\",\n  \"a\": {\"keep\": 1}\n}\n");
    }

    #[test]
    fn a_splice_refuses_rather_than_guessing_when_it_cannot_be_proven_safe() {
        // Both refusals have a correct fallback, so refusing costs a reordered file and nothing
        // else. Guessing costs somebody their credential store.
        assert_eq!(
            splice(r#"{"a":"x","b":"x"}"#, "x", "y"),
            None,
            "a value that occurs twice is not one this can aim at"
        );
        assert_eq!(
            splice(r#"{"a":"x"}"#, "x", "has a \" in it"),
            None,
            "a replacement needing an escape would break the document"
        );
        assert_eq!(splice(r#"{"a":"x"}"#, "absent", "y"), None);
    }

    #[test]
    fn a_rewrite_that_cannot_splice_still_produces_a_correct_document_and_says_so() {
        // The fallback path: every key survives, the values are right, and `byte_preserving` is
        // false so the caller can report which of the two happened.
        let text = r#"{"a":"dup","b":"dup","other":42}"#;
        let parsed: Value = serde_json::from_str(text).expect("JSON");
        let (rewritten, byte_preserving) =
            rewrite(text, &parsed, &[("/a", "dup", "fresh")]).expect("rewritten");
        assert!(!byte_preserving);
        let back: Value = serde_json::from_str(&rewritten).expect("still JSON");
        assert_eq!(back["a"], json!("fresh"));
        assert_eq!(back["b"], json!("dup"), "the untouched value survives");
        assert_eq!(back["other"], json!(42), "and so does a key nobody named");
    }

    #[test]
    fn an_atomic_write_keeps_the_mode_the_document_already_had() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("auth.json");
        std::fs::write(&path, "{}").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        }
        write_atomically(&path, "{\"a\":1}").expect("written");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "{\"a\":1}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).expect("stat").permissions().mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "a credential file must not widen because it was renewed"
            );
        }
    }

    #[test]
    fn a_document_that_is_not_stale_is_neither_sent_nor_written() {
        // No network is reachable in this test, so reaching the exchange at all would fail loudly.
        // That is the assertion: a fresh token means nothing is sent.
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("auth.json");
        let fresh = jwt(2_000_000_000);
        let body = json!({"tokens": {"access_token": fresh, "refresh_token": "synthetic"}});
        let text = serde_json::to_string_pretty(&body).expect("JSON");
        std::fs::write(&path, &text).expect("write");
        let document = AuthDocument {
            path: path.clone(),
            access_pointer: "/tokens/access_token".to_owned(),
            refresh_pointer: "/tokens/refresh_token".to_owned(),
            id_token_pointer: None,
            renewed_at_pointer: None,
        };
        let endpoint = TokenEndpoint {
            url: "http://127.0.0.1:1/oauth/token".to_owned(),
            client_id: "synthetic-client".to_owned(),
        };
        let outcome = renew_if_stale(
            &document,
            &endpoint,
            1_788_000_000,
            Duration::from_mins(15),
            "2026-08-30T00:00:00Z",
        )
        .expect("a fresh token is not an error");
        assert_eq!(outcome, None);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            text,
            "the document was not touched"
        );
    }

    /// A token endpoint that answers once with `answer`, and reports the request body it read.
    ///
    /// Real HTTP against a real socket, because the thing under test is the whole exchange — what
    /// is sent, what comes back, and what lands on disk. A hand-stubbed transport would prove the
    /// splice and nothing about the request.
    fn one_shot_endpoint(answer: &str) -> (String, std::sync::mpsc::Receiver<String>) {
        endpoint_after_request("200 OK", answer, || {})
    }

    /// A one-shot endpoint with a caller-controlled effect after it has received the request and
    /// before it sends the response.
    fn endpoint_after_request<F>(
        status: &str,
        answer: &str,
        after_request: F,
    ) -> (String, std::sync::mpsc::Receiver<String>)
    where
        F: FnOnce() + Send + 'static,
    {
        use std::io::{BufRead as _, Read as _, Write as _};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let url = format!(
            "http://{}/oauth/token",
            listener.local_addr().expect("bound")
        );
        let (sender, receiver) = std::sync::mpsc::channel();
        let status = status.to_owned();
        let answer = answer.to_owned();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("one connection");
            let mut reader = std::io::BufReader::new(stream);
            let mut length = 0_usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).expect("a header line") <= 2 {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().expect("a length");
                }
            }
            let mut body = vec![0_u8; length];
            reader.read_exact(&mut body).expect("the body");
            let _ = sender.send(String::from_utf8_lossy(&body).into_owned());
            after_request();
            let mut stream = reader.into_inner();
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{answer}",
                answer.len()
            );
            let _ = stream.flush();
        });
        (url, receiver)
    }

    fn stale_document(path: PathBuf) -> AuthDocument {
        AuthDocument {
            path,
            access_pointer: "/tokens/access_token".to_owned(),
            refresh_pointer: "/tokens/refresh_token".to_owned(),
            id_token_pointer: None,
            renewed_at_pointer: None,
        }
    }

    #[test]
    fn a_stale_document_is_renewed_written_back_byte_for_byte_and_reported() {
        // The whole act, end to end: what is sent, what comes back, what is on disk afterwards.
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("auth.json");
        let (stale, fresh) = (jwt(1_000), jwt(2_000_000_000));
        // Hand-written rather than serialised, with the key order and the extra keys a real store
        // has: what is being asserted below is that all of it survives untouched.
        let before = format!(
            "{{\n  \"auth_mode\": \"synthetic\",\n  \"OPENAI_API_KEY\": null,\n  \"tokens\": {{\n    \
             \"id_token\": \"synthetic-id\",\n    \"access_token\": \"{stale}\",\n    \
             \"refresh_token\": \"synthetic-refresh-one\",\n    \"account_id\": \"keep-me\"\n  }},\n  \
             \"last_refresh\": \"2026-01-01T00:00:00Z\"\n}}\n"
        );
        std::fs::write(&path, &before).expect("write");

        let (url, sent) = one_shot_endpoint(&format!(
            r#"{{"access_token":"{fresh}","refresh_token":"synthetic-refresh-two","id_token":"synthetic-id-two","token_type":"Bearer"}}"#
        ));
        let renewed = renew_if_stale(
            &AuthDocument {
                path: path.clone(),
                access_pointer: "/tokens/access_token".to_owned(),
                refresh_pointer: "/tokens/refresh_token".to_owned(),
                id_token_pointer: Some("/tokens/id_token".to_owned()),
                renewed_at_pointer: Some("/last_refresh".to_owned()),
            },
            &TokenEndpoint {
                url,
                client_id: "synthetic-client".to_owned(),
            },
            1_788_000_000,
            Duration::from_mins(15),
            "2026-08-30T12:00:00Z",
        )
        .expect("renewed")
        .expect("the token was stale, so something happened");

        // What went out: the refresh token and the grant, and no scope narrowing what the operator
        // was granted.
        let request: Value = serde_json::from_str(&sent.recv().expect("a request")).expect("JSON");
        assert_eq!(request["grant_type"], json!("refresh_token"));
        assert_eq!(request["refresh_token"], json!("synthetic-refresh-one"));
        assert_eq!(request["client_id"], json!("synthetic-client"));
        assert_eq!(
            request.get("scope"),
            None,
            "an omitted scope keeps the granted one"
        );

        // What came back, as the caller is told it.
        assert_eq!(renewed.expires_unix, Some(2_000_000_000));
        assert!(renewed.refresh_token_rotated, "the server issued a new one");
        assert!(renewed.byte_preserving);

        // What is on disk. Every byte that did not have to change is where it was — including the
        // key order, the two-space indent, `OPENAI_API_KEY` and a key nobody here reads.
        let after = std::fs::read_to_string(&path).expect("read");
        let expected = before
            .replace(&stale, &fresh)
            .replace("synthetic-refresh-one", "synthetic-refresh-two")
            .replace("\"synthetic-id\"", "\"synthetic-id-two\"")
            .replace("2026-01-01T00:00:00Z", "2026-08-30T12:00:00Z");
        assert_eq!(after, expected, "only the four values moved");
        assert!(after.contains("\"account_id\": \"keep-me\""));
    }

    #[test]
    fn a_server_that_keeps_the_refresh_token_is_reported_as_not_having_rotated_it() {
        // The distinction a backup depends on: when this is false, the copy of the file taken
        // before the run still holds a credential that works.
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("auth.json");
        let (stale, fresh) = (jwt(1_000), jwt(2_000_000_000));
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&json!({
                "tokens": {"access_token": stale, "refresh_token": "synthetic-refresh-one"}
            }))
            .expect("JSON"),
        )
        .expect("write");
        let (url, _sent) = one_shot_endpoint(&format!(
            r#"{{"access_token":"{fresh}","refresh_token":"synthetic-refresh-one"}}"#
        ));
        let renewed = renew_if_stale(
            &AuthDocument {
                path,
                access_pointer: "/tokens/access_token".to_owned(),
                refresh_pointer: "/tokens/refresh_token".to_owned(),
                id_token_pointer: None,
                renewed_at_pointer: None,
            },
            &TokenEndpoint {
                url,
                client_id: "synthetic-client".to_owned(),
            },
            1_788_000_000,
            Duration::from_mins(15),
            "2026-08-30T12:00:00Z",
        )
        .expect("renewed")
        .expect("stale");
        assert!(!renewed.refresh_token_rotated);
    }

    #[test]
    fn a_stale_document_whose_refresh_pointer_leads_nowhere_refuses_before_sending_anything() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("auth.json");
        let stale = jwt(1_000);
        std::fs::write(
            &path,
            serde_json::to_string(&json!({"tokens": {"access_token": stale}})).expect("JSON"),
        )
        .expect("write");
        let document = AuthDocument {
            path,
            access_pointer: "/tokens/access_token".to_owned(),
            refresh_pointer: "/tokens/refresh_token".to_owned(),
            id_token_pointer: None,
            renewed_at_pointer: None,
        };
        let endpoint = TokenEndpoint {
            url: "http://127.0.0.1:1/oauth/token".to_owned(),
            client_id: "synthetic-client".to_owned(),
        };
        let error = renew_if_stale(
            &document,
            &endpoint,
            1_788_000_000,
            Duration::from_mins(15),
            "2026-08-30T00:00:00Z",
        )
        .expect_err("there is nothing to present");
        assert!(error.contains("/tokens/refresh_token"), "{error}");
    }

    #[test]
    fn a_failed_exchange_never_quotes_its_response_body() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("auth.json");
        let stale = jwt(1_000);
        let before = serde_json::to_string(&json!({
            "tokens": {"access_token": stale, "refresh_token": "synthetic-request-secret"}
        }))
        .expect("JSON");
        std::fs::write(&path, &before).expect("write");
        let (url, _sent) = endpoint_after_request(
            "400 Bad Request",
            r#"{"detail":"synthetic-response-secret"}"#,
            || {},
        );
        let error = renew_if_stale(
            &stale_document(path.clone()),
            &TokenEndpoint {
                url,
                client_id: "synthetic-client".to_owned(),
            },
            1_788_000_000,
            Duration::from_mins(15),
            "2026-08-31T00:00:00Z",
        )
        .expect_err("the exchange refuses");
        assert!(error.contains("400"), "{error}");
        assert!(!error.contains("synthetic-response-secret"), "{error}");
        assert!(!error.contains("synthetic-request-secret"), "{error}");
        assert_eq!(std::fs::read_to_string(path).expect("read"), before);
    }

    #[test]
    fn an_empty_returned_credential_refuses_before_writing() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("auth.json");
        let stale = jwt(1_000);
        let before = serde_json::to_string(&json!({
            "tokens": {"access_token": stale, "refresh_token": "synthetic-refresh"}
        }))
        .expect("JSON");
        std::fs::write(&path, &before).expect("write");
        let (url, _sent) = one_shot_endpoint(r#"{"access_token":""}"#);
        let error = renew_if_stale(
            &stale_document(path.clone()),
            &TokenEndpoint {
                url,
                client_id: "synthetic-client".to_owned(),
            },
            1_788_000_000,
            Duration::from_mins(15),
            "2026-08-31T00:00:00Z",
        )
        .expect_err("empty access token refuses");
        assert!(error.contains("access_token"), "{error}");
        assert_eq!(std::fs::read_to_string(path).expect("read"), before);
    }

    #[test]
    fn a_document_changed_during_exchange_is_not_overwritten() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("auth.json");
        let stale = jwt(1_000);
        let fresh = jwt(2_000_000_000);
        let before = serde_json::to_string(&json!({
            "tokens": {"access_token": stale, "refresh_token": "synthetic-refresh"}
        }))
        .expect("JSON");
        let concurrent = serde_json::to_string(&json!({
            "tokens": {"access_token": "concurrent-owner-value", "refresh_token": "concurrent-refresh"}
        }))
        .expect("JSON");
        std::fs::write(&path, &before).expect("write");
        let changed_path = path.clone();
        let changed_bytes = concurrent.clone();
        let (url, _sent) = endpoint_after_request(
            "200 OK",
            &format!(r#"{{"access_token":"{fresh}"}}"#),
            move || std::fs::write(changed_path, changed_bytes).expect("concurrent write"),
        );
        let error = renew_if_stale(
            &stale_document(path.clone()),
            &TokenEndpoint {
                url,
                client_id: "synthetic-client".to_owned(),
            },
            1_788_000_000,
            Duration::from_mins(15),
            "2026-08-31T00:00:00Z",
        )
        .expect_err("a stale rewrite refuses");
        assert!(error.contains("changed while"), "{error}");
        assert_eq!(std::fs::read_to_string(path).expect("read"), concurrent);
    }
}
