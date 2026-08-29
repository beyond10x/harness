use zeroize::Zeroizing;

use crate::WireError;

/// One credential, held only for the duration of one call and wiped on drop.
///
/// It has no `Display`, and its `Debug` prints a fixed placeholder, so it cannot reach a log line
/// or an error message by accident. The wire adapter reads it, writes one header, and drops it.
#[derive(Clone, PartialEq, Eq)]
pub struct Bearer(Zeroizing<String>);

impl Bearer {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    /// Returns the credential. Every call site of this is a place a secret can escape.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Bearer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Bearer(<redacted>)")
    }
}

/// What kind of secret a source answers with, so a wire can present it the way its route wants.
///
/// # Why the neutral layer knows this at all
///
/// The first wire never needed it: one route, one presentation, one header, and the adapter could
/// hard-code it. The second wire has two routes for the same endpoint that take the *same string*
/// under **different header names** — one for a key issued to a program, one for a token obtained
/// on a person's behalf — so which one a credential is stopped being derivable from the wire alone
/// and became a property of the source that produced it.
///
/// This names the **kind**, never the header. Header names are vendor-shaped bytes and live in the
/// wire crate (AGENTS.md invariant 3); a source declares what it holds and the wire decides how to
/// send it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CredentialKind {
    /// A long-lived secret issued to a program.
    ApiKey,
    /// A short-lived token obtained on a person's behalf, which expires and is renewed elsewhere.
    Oauth,
}

/// Where a wire adapter obtains its credential, at call time.
///
/// The indirection is the point: no config struct in this component holds a secret, so no config
/// struct can serialize one into a log, a fixture, or a crash report.
pub trait BearerSource: Send + Sync {
    /// Returns the credential for the next call.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal when the credential is unavailable. The message must name the
    /// source, never the value.
    fn bearer(&self) -> Result<Bearer, WireError>;

    /// What kind of secret [`BearerSource::bearer`] answers with.
    ///
    /// Defaulted to [`CredentialKind::ApiKey`], which is what every source written before a wire
    /// could tell the difference actually holds.
    fn kind(&self) -> CredentialKind {
        CredentialKind::ApiKey
    }
}

/// A credential the caller already holds.
///
/// The value lives as long as this source does, which is what reading a key file at startup means.
/// A source that fetches per call — a token service, a keyring — implements the trait instead.
pub struct StaticBearer(Bearer);

impl StaticBearer {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Bearer::new(value))
    }
}

impl std::fmt::Debug for StaticBearer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("StaticBearer(<redacted>)")
    }
}

impl BearerSource for StaticBearer {
    fn bearer(&self) -> Result<Bearer, WireError> {
        Ok(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_static_source_returns_its_credential() {
        let source = StaticBearer::new("token");
        assert_eq!(source.bearer().expect("available").expose(), "token");
        assert_eq!(format!("{source:?}"), "StaticBearer(<redacted>)");
    }

    #[test]
    fn a_source_that_says_nothing_holds_an_api_key() {
        // The default is what every source written before a wire could tell the difference
        // actually holds. A source whose secret is presented differently says so.
        assert_eq!(StaticBearer::new("token").kind(), CredentialKind::ApiKey);
    }

    #[test]
    fn debug_never_prints_the_secret() {
        let bearer = Bearer::new("sk-do-not-print-me");
        assert_eq!(format!("{bearer:?}"), "Bearer(<redacted>)");
        assert!(!format!("{bearer:?}").contains("sk-"));
    }

    #[test]
    fn expose_returns_the_value() {
        assert_eq!(Bearer::new("token").expose(), "token");
        assert!(Bearer::new("").is_empty());
    }
}
