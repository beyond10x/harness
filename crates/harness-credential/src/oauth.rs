//! A credential source for a subscription token somebody else already obtained.
//!
//! # What this deliberately does not do
//!
//! It **does not look for a credential.** There is no default path, no vendor directory, no
//! environment variable it knows the name of, and no fallback if the named source is missing — the
//! harness reads nothing it was not pointed at (AGENTS.md *Safety envelope*). A source that
//! searched `~/.claude` would be an ambient fallback whichever way it was spelled, and a run whose
//! credential came from wherever the process happened to start is a run nobody can explain
//! afterwards.
//!
//! It also **does not refresh**. A subscription token expires, and renewing it means holding a
//! refresh token and calling an authorization server — custody this component has not been given.
//! What it does instead is re-read the named source **on every call**, so a token an owner outside
//! this process renews is picked up on the next turn without the run being restarted. That is the
//! whole of the renewal story here, and the run fails by name when the source has gone stale
//! rather than pretending otherwise.

use std::path::{Path, PathBuf};

use harness_wire::{Bearer, BearerSource, CredentialKind, WireError};

/// Where the caller pointed this source. Never a default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamedSource {
    /// A file whose contents are the token, or a JSON document holding it.
    File(PathBuf),
    /// An environment variable, named by the caller and not by this crate.
    Environment(String),
}

impl NamedSource {
    pub fn file(path: impl AsRef<Path>) -> Self {
        Self::File(path.as_ref().to_path_buf())
    }

    pub fn environment(name: impl Into<String>) -> Self {
        Self::Environment(name.into())
    }

    /// How this source is named in a refusal. Never the value it holds.
    fn describe(&self) -> String {
        match self {
            Self::File(path) => format!("the credential file `{}`", path.display()),
            Self::Environment(name) => format!("the environment variable `{name}`"),
        }
    }

    fn read(&self) -> Result<String, WireError> {
        match self {
            Self::File(path) => std::fs::read_to_string(path).map_err(|error| {
                WireError::unauthorized(format!("reading {}: {error}", self.describe()))
            }),
            Self::Environment(name) => std::env::var(name)
                .map_err(|_| WireError::unauthorized(format!("{} is not set", self.describe()))),
        }
    }
}

/// A subscription access token, re-read from a named source on every call.
///
/// The document may be the bare token, or JSON with the token at a **caller-named** JSON pointer
/// (RFC 6901). The pointer is named rather than known: which field a given credential store puts
/// its access token in is that store's business, and a built-in path would be this crate guessing
/// at somebody else's file format and silently reading the wrong field when it changed.
pub struct SubscriptionToken {
    source: NamedSource,
    pointer: Option<String>,
}

impl SubscriptionToken {
    /// A source whose named document is the token itself.
    pub fn new(source: NamedSource) -> Self {
        Self {
            source,
            pointer: None,
        }
    }

    /// A source whose named document is JSON, with the token at `pointer`.
    #[must_use]
    pub fn at_pointer(mut self, pointer: impl Into<String>) -> Self {
        self.pointer = Some(pointer.into());
        self
    }

    /// How this source is named in a refusal, for a caller that has to report it.
    pub fn describe(&self) -> String {
        self.source.describe()
    }
}

impl std::fmt::Debug for SubscriptionToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The source and the pointer are things the caller typed on a command line; the value they
        // lead to never reaches here.
        formatter
            .debug_struct("SubscriptionToken")
            .field("source", &self.source)
            .field("pointer", &self.pointer)
            .finish()
    }
}

impl BearerSource for SubscriptionToken {
    fn bearer(&self) -> Result<Bearer, WireError> {
        let document = self.source.read()?;
        let token = match &self.pointer {
            None => document.trim().to_owned(),
            Some(pointer) => {
                let parsed: serde_json::Value =
                    serde_json::from_str(&document).map_err(|error| {
                        WireError::unauthorized(format!(
                            "{} was read as JSON because a pointer was named, and is not JSON: \
                             {error}",
                            self.source.describe()
                        ))
                    })?;
                parsed
                    .pointer(pointer)
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        WireError::unauthorized(format!(
                            "{} holds no string at `{pointer}`",
                            self.source.describe()
                        ))
                    })?
                    .trim()
                    .to_owned()
            }
        };
        // A source that was named and answered with nothing is a refusal, not a declaration: the
        // caller meant to authenticate and something went wrong upstream of here.
        if token.is_empty() {
            return Err(WireError::unauthorized(format!(
                "{} is empty",
                self.source.describe()
            )));
        }
        Ok(Bearer::new(token))
    }

    fn kind(&self) -> CredentialKind {
        CredentialKind::Oauth
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_wire::WireErrorCode;

    fn written(dir: &tempfile::TempDir, name: &str, body: &str) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, body).expect("write");
        path
    }

    #[test]
    fn a_bare_token_file_is_read_and_trimmed() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        // Synthetic and obviously so. A fixture that writes a real credential to disk leaks one
        // (AGENTS.md invariant 17).
        let path = written(&dir, "token", "  synthetic-not-a-real-token\n");
        let source = SubscriptionToken::new(NamedSource::file(&path));
        assert_eq!(
            source.bearer().expect("readable").expose(),
            "synthetic-not-a-real-token"
        );
        assert_eq!(source.kind(), CredentialKind::Oauth);
    }

    #[test]
    fn a_json_document_yields_the_token_at_the_named_pointer() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = written(
            &dir,
            "credentials.json",
            r#"{"store": {"accessToken": "synthetic-oauth-token", "refreshToken": "unused"}}"#,
        );
        let source =
            SubscriptionToken::new(NamedSource::file(&path)).at_pointer("/store/accessToken");
        assert_eq!(
            source.bearer().expect("readable").expose(),
            "synthetic-oauth-token"
        );
    }

    #[test]
    fn a_pointer_that_leads_nowhere_refuses_by_name() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = written(&dir, "credentials.json", r#"{"store": {}}"#);
        let error = SubscriptionToken::new(NamedSource::file(&path))
            .at_pointer("/store/accessToken")
            .bearer()
            .expect_err("a missing pointer refuses");
        assert_eq!(error.code, WireErrorCode::Unauthorized);
        assert!(error.message.contains("/store/accessToken"), "{error}");
        assert!(error.message.contains("credentials.json"), "{error}");
    }

    #[test]
    fn a_source_that_is_not_there_refuses_and_never_falls_back() {
        // The one behaviour that matters most: there is no second place to look. A source that
        // searched a vendor directory on failure would be an ambient credential fallback.
        let error = SubscriptionToken::new(NamedSource::file("/definitely/not/here"))
            .bearer()
            .expect_err("a missing file refuses");
        assert_eq!(error.code, WireErrorCode::Unauthorized);
        assert!(error.message.contains("/definitely/not/here"), "{error}");

        let error = SubscriptionToken::new(NamedSource::environment(
            "B10X_HARNESS_ABSENT_OAUTH_TEST_TOKEN",
        ))
        .bearer()
        .expect_err("an unset variable refuses");
        assert!(
            error
                .message
                .contains("B10X_HARNESS_ABSENT_OAUTH_TEST_TOKEN"),
            "{error}"
        );
    }

    #[test]
    fn an_empty_named_source_refuses_rather_than_reaching_the_endpoint() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = written(&dir, "token", "\n \n");
        let error = SubscriptionToken::new(NamedSource::file(&path))
            .bearer()
            .expect_err("an empty source refuses");
        assert_eq!(error.code, WireErrorCode::Unauthorized);
    }

    #[test]
    fn a_renewed_token_is_picked_up_without_restarting_the_run() {
        // The whole of the renewal story: this source re-reads on every call, so an owner outside
        // this process that renews the token is followed. A source that cached at construction
        // would serve an expired token until the run was restarted.
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = written(&dir, "token", "synthetic-first");
        let source = SubscriptionToken::new(NamedSource::file(&path));
        assert_eq!(
            source.bearer().expect("readable").expose(),
            "synthetic-first"
        );
        std::fs::write(&path, "synthetic-renewed").expect("write");
        assert_eq!(
            source.bearer().expect("readable").expose(),
            "synthetic-renewed"
        );
    }

    #[test]
    fn debug_names_the_source_and_carries_no_value() {
        let source = SubscriptionToken::new(NamedSource::file("/named/by/the/caller"))
            .at_pointer("/store/accessToken");
        let rendered = format!("{source:?}");
        assert!(rendered.contains("/named/by/the/caller"), "{rendered}");
        assert!(rendered.contains("/store/accessToken"), "{rendered}");
    }
}
