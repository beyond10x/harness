//! Where to talk to a model, and how — the half of a run's configuration that grants nothing.
//!
//! # Why this is separate from a profile, and why only this half ships compiled in
//!
//! A **provider** is an endpoint, a wire dialect, a default model, and where the credential is
//! read from. None of it grants a run anything: the same provider serves a read-only run and one
//! that rewrites your checkout. So the collection can ship inside the binary without putting a
//! permission bundle there.
//!
//! A **profile** is the other half — `write`, an approval ceiling, an allow-list, a write scope —
//! and none of *that* is compiled in, for exactly the reason this is. The line between the two
//! files is permission, and it is the whole design.
//!
//! # The credential is defaulted, and that is a softening paid for by the record
//!
//! `crate::resolve_credential`'s own doc says there is no ambient fallback, and `--oauth-token-file`
//! says there is "no default path and no vendor directory this looks in". A built-in `claude` that
//! names `~/.claude/.credentials.json` is a vendor directory this looks in.
//!
//! That is accepted deliberately, and the invariant's *purpose* — that a run can be explained
//! afterwards — is met by the record instead: a run whose credential came from a provider reports
//! `credential_source: "provider:<name>"` rather than `"named"`, and `providers show <name>` prints
//! the path before a token is spent. Something is defaulted; nothing is silent. If those two ever
//! come apart, this trade is not paid for and the default should go.
//!
//! # Only what has been measured is here
//!
//! `claude`'s values are the ones a live run actually used on 2026-08-29, read out of the eval that
//! drives it. `codex` is deliberately absent: its subscription credential location has not been
//! read off a working install, and inventing a vendor path is the one mistake this table exists to
//! avoid. Add it when somebody measures it.

use std::collections::BTreeMap;

use serde::Deserialize;

/// Where a provider's bearer credential is read from.
///
/// Two shapes because two vendors differ: a subscription drops a JSON document holding an OAuth
/// token, and an API key is conventionally an environment variable. A provider names one; the
/// operator may replace it field by field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    /// A JSON document, and a pointer to the token inside it.
    OauthFile { path: String, pointer: String },
    /// An environment variable holding the key.
    ApiKeyEnv { name: String },
}

/// One way to reach a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    pub name: String,
    pub base_url: String,
    /// `anthropic-messages` or `openai-responses` — the two this build speaks.
    pub wire: String,
    pub model: String,
    pub credential: Credential,
    /// Short names for the models this provider serves, resolved before the request is built.
    ///
    /// **A vendor's exact identifiers carry dates, and the dates move.**
    /// `claude-haiku-4-5-20251001` is a fact about one release; `haiku` is what a person means.
    /// Writing the dated string into a config means every config in the fleet goes stale on the
    /// next release, and the failure is a `404` from the far side rather than anything this build
    /// can explain.
    ///
    /// Aliases are **this build's** answer to *which one is current*, so a run that asked for
    /// `haiku` is pinned by the binary it ran under rather than by whatever a config was last
    /// edited. `session.started.model` records what it resolved to, so the record still names the
    /// exact model — an alias is a convenience at the command line, never in the evidence.
    pub aliases: BTreeMap<String, String>,
}

/// The operator's override of one provider, field by field.
///
/// **Merged, not replaced**, unlike a profile: a provider is a bag of independent connection facts,
/// and someone setting `model` to try a bigger one should not silently lose the endpoint with it.
/// A profile is the opposite — see `profile.rs` — because a half-merged permission bundle is worse
/// than either half.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ProviderOverride {
    pub base_url: Option<String>,
    pub wire: Option<String>,
    pub model: Option<String>,
    pub oauth_token_file: Option<String>,
    pub oauth_token_pointer: Option<String>,
    pub api_key_env: Option<String>,
    /// Extra model aliases, merged over the ones this build ships.
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
}

/// The providers this build ships.
///
/// # Panics
///
/// Never: the table is a constant this crate's own tests read back.
#[must_use]
pub fn built_in() -> Vec<Provider> {
    vec![
        Provider {
            name: "claude".to_owned(),
            base_url: "https://api.anthropic.com/v1".to_owned(),
            wire: "anthropic-messages".to_owned(),
            // Haiku rather than the largest model: a default that quietly costs ten times as much
            // per run is a default nobody chose. `[providers.claude] model = …` is one line.
            model: "claude-haiku-4-5-20251001".to_owned(),
            credential: Credential::OauthFile {
                path: "~/.claude/.credentials.json".to_owned(),
                pointer: "/claudeAiOauth/accessToken".to_owned(),
            },
            aliases: [
                ("haiku", "claude-haiku-4-5-20251001"),
                ("sonnet", "claude-sonnet-5"),
                ("opus", "claude-opus-5"),
                ("fable", "claude-fable-5"),
            ]
            .into_iter()
            .map(|(short, exact)| (short.to_owned(), exact.to_owned()))
            .collect(),
        },
        Provider {
            name: "openai".to_owned(),
            base_url: "https://api.openai.com/v1".to_owned(),
            wire: "openai-responses".to_owned(),
            model: "gpt-5.2".to_owned(),
            // An environment variable and not a file: there is no vendor directory here whose
            // layout has been measured, and the conventional variable is what every other tool
            // reads. Nothing is invented.
            credential: Credential::ApiKeyEnv {
                name: "OPENAI_API_KEY".to_owned(),
            },
            // None: this vendor's current identifiers have not been read off a working account,
            // and an alias pointing at a model that does not exist is worse than no alias — it
            // fails at the far side, where nothing here can say why. `--model` still works.
            aliases: BTreeMap::new(),
        },
    ]
}

/// The provider a run will use, after the operator's overrides.
///
/// # Errors
///
/// Names the provider that does not exist and lists the ones that do — a misspelling that silently
/// fell back to a default would be a run against an endpoint nobody chose.
pub fn resolve(
    name: &str,
    overrides: &BTreeMap<String, ProviderOverride>,
) -> Result<Provider, String> {
    let mut provider = built_in()
        .into_iter()
        .find(|provider| provider.name == name)
        .ok_or_else(|| {
            format!(
                "`{name}` is not a provider this build knows. It has: {}. Define your own with a \
                 `[providers.{name}]` table carrying `base-url`, `wire`, `model` and a credential.",
                built_in()
                    .iter()
                    .map(|provider| provider.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    let Some(over) = overrides.get(name) else {
        return Ok(provider);
    };
    if let Some(base_url) = &over.base_url {
        provider.base_url.clone_from(base_url);
    }
    if let Some(wire) = &over.wire {
        provider.wire.clone_from(wire);
    }
    if let Some(model) = &over.model {
        provider.model.clone_from(model);
    }
    // Merged into the shipped set rather than replacing it: an operator adding `mine = "…"` should
    // not silently lose `haiku`. A same-named alias wins, which is how a stale built-in gets
    // corrected without waiting for a release.
    for (short, exact) in &over.aliases {
        provider.aliases.insert(short.clone(), exact.clone());
    }
    // A credential override replaces the *whole* credential, because the two shapes are not
    // interchangeable field by field: a `path` left over from an OAuth default beside an
    // `api-key-env` would be two credentials, and the run would use whichever the reader looked at
    // first.
    match (&over.api_key_env, &over.oauth_token_file) {
        (Some(name), None) => {
            provider.credential = Credential::ApiKeyEnv { name: name.clone() };
        }
        (None, Some(path)) => {
            let pointer =
                over.oauth_token_pointer
                    .clone()
                    .unwrap_or_else(|| match &provider.credential {
                        Credential::OauthFile { pointer, .. } => pointer.clone(),
                        Credential::ApiKeyEnv { .. } => String::new(),
                    });
            provider.credential = Credential::OauthFile {
                path: path.clone(),
                pointer,
            };
        }
        (Some(_), Some(_)) => {
            return Err(format!(
                "`[providers.{name}]` names both `api-key-env` and `oauth-token-file`. A run takes \
                 one credential; naming two leaves which one it used to whoever reads the code."
            ));
        }
        (None, None) => {}
    }
    Ok(provider)
}

impl Provider {
    /// The exact identifier for a model a caller named, expanding an alias where there is one.
    ///
    /// Unknown names pass through untouched: a provider's alias table is a convenience, not a
    /// registry of everything the endpoint serves, and refusing a model this build has not heard
    /// of would make every new release unusable until somebody edited a table.
    #[must_use]
    pub fn exact_model(&self, wanted: &str) -> String {
        self.aliases
            .get(wanted)
            .cloned()
            .unwrap_or_else(|| wanted.to_owned())
    }
}

/// `~` at the start of a declared path, expanded against `HOME`.
///
/// Only at the start, and only `~/`: a shell expands this before the process sees it, and a value
/// read from a file was never through a shell. Anything else is left exactly as written.
#[must_use]
pub fn expand_home(path: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => std::env::var("HOME").map_or_else(
            |_| path.to_owned(),
            |home| format!("{}/{rest}", home.trim_end_matches('/')),
        ),
        None => path.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overrides(name: &str, over: ProviderOverride) -> BTreeMap<String, ProviderOverride> {
        let mut map = BTreeMap::new();
        map.insert(name.to_owned(), over);
        map
    }

    #[test]
    fn an_override_changes_one_field_and_keeps_the_rest() {
        // The difference from a profile, and it is deliberate: a provider is independent
        // connection facts, so setting `model` to try a bigger one must not silently drop the
        // endpoint it is served from.
        let claude = resolve(
            "claude",
            &overrides(
                "claude",
                ProviderOverride {
                    model: Some("claude-sonnet-4-5".to_owned()),
                    ..ProviderOverride::default()
                },
            ),
        )
        .expect("resolves");
        assert_eq!(claude.model, "claude-sonnet-4-5");
        assert_eq!(claude.base_url, "https://api.anthropic.com/v1");
        assert_eq!(claude.wire, "anthropic-messages");
        assert!(matches!(claude.credential, Credential::OauthFile { .. }));
    }

    #[test]
    fn a_credential_override_replaces_the_whole_credential() {
        // Field-by-field here would leave an OAuth path beside an API key env var — two
        // credentials, with the run using whichever the reader looked at first.
        let claude = resolve(
            "claude",
            &overrides(
                "claude",
                ProviderOverride {
                    api_key_env: Some("MY_KEY".to_owned()),
                    ..ProviderOverride::default()
                },
            ),
        )
        .expect("resolves");
        assert_eq!(
            claude.credential,
            Credential::ApiKeyEnv {
                name: "MY_KEY".to_owned()
            }
        );
    }

    #[test]
    fn naming_two_credentials_is_refused_rather_than_one_of_them_winning() {
        let error = resolve(
            "claude",
            &overrides(
                "claude",
                ProviderOverride {
                    api_key_env: Some("MY_KEY".to_owned()),
                    oauth_token_file: Some("/tmp/t.json".to_owned()),
                    ..ProviderOverride::default()
                },
            ),
        )
        .expect_err("refused");
        assert!(error.contains("takes \none credential") || error.contains("one credential"));
    }

    #[test]
    fn an_oauth_override_keeps_the_pointer_it_did_not_restate() {
        let claude = resolve(
            "claude",
            &overrides(
                "claude",
                ProviderOverride {
                    oauth_token_file: Some("/tmp/other.json".to_owned()),
                    ..ProviderOverride::default()
                },
            ),
        )
        .expect("resolves");
        assert_eq!(
            claude.credential,
            Credential::OauthFile {
                path: "/tmp/other.json".to_owned(),
                pointer: "/claudeAiOauth/accessToken".to_owned()
            },
            "the pointer is part of knowing how to read that file, not a separate decision"
        );
    }

    #[test]
    fn an_unknown_provider_names_the_ones_that_exist() {
        // A misspelling that fell back to a default would be a run against an endpoint nobody
        // chose, and the bill would arrive before anyone noticed.
        let error = resolve("clade", &BTreeMap::new()).expect_err("refused");
        assert!(error.contains("clade"), "{error}");
        assert!(error.contains("claude"), "{error}");
        assert!(error.contains("openai"), "{error}");
    }

    #[test]
    fn no_built_in_provider_carries_a_permission() {
        // The line this whole file is drawn on. If a provider ever grows a field that decides what
        // a run may *do*, it belongs in a profile — which is not compiled in — and this test is
        // where that gets caught.
        let rendered = format!("{:?}", built_in());
        for permission in ["write", "approve", "allow_program", "scope"] {
            assert!(
                !rendered.contains(permission),
                "a provider carries connection facts only, and this one mentions `{permission}`"
            );
        }
    }

    #[test]
    fn a_tilde_is_expanded_only_at_the_start() {
        let home = std::env::var("HOME").expect("a home directory");
        assert_eq!(expand_home("~/x.json"), format!("{home}/x.json"));
        assert_eq!(expand_home("/a/~/b"), "/a/~/b", "only at the start");
        assert_eq!(expand_home("~x"), "~x", "only `~/`");
    }
}

#[cfg(test)]
mod alias_tests {
    use super::*;

    #[test]
    fn a_short_name_resolves_to_the_exact_identifier_this_build_pins() {
        // A vendor's identifiers carry release dates, and a config holding a dated string goes
        // stale on the next release — as a 404 from the far side, which nothing here can explain.
        let claude = resolve("claude", &BTreeMap::new()).expect("resolves");
        assert_eq!(claude.exact_model("haiku"), "claude-haiku-4-5-20251001");
        assert_eq!(claude.exact_model("opus"), "claude-opus-5");
        assert_eq!(claude.exact_model("sonnet"), "claude-sonnet-5");
    }

    #[test]
    fn a_model_this_build_has_not_heard_of_passes_through_untouched() {
        // Refusing an unknown name would make every model released after this binary unusable
        // until somebody edited a table. The alias set is a convenience, not a registry.
        let claude = resolve("claude", &BTreeMap::new()).expect("resolves");
        assert_eq!(
            claude.exact_model("claude-something-9-20990101"),
            "claude-something-9-20990101"
        );
    }

    #[test]
    fn an_operators_alias_is_merged_over_the_shipped_ones_rather_than_replacing_them() {
        // Adding one alias must not silently cost the others; and a same-named one wins, which is
        // how a stale built-in gets corrected without waiting for a release.
        let mut over = ProviderOverride::default();
        over.aliases
            .insert("haiku".to_owned(), "my-haiku".to_owned());
        over.aliases
            .insert("mine".to_owned(), "my-model".to_owned());
        let mut map = BTreeMap::new();
        map.insert("claude".to_owned(), over);
        let claude = resolve("claude", &map).expect("resolves");
        assert_eq!(
            claude.exact_model("haiku"),
            "my-haiku",
            "the operator's wins"
        );
        assert_eq!(claude.exact_model("mine"), "my-model");
        assert_eq!(
            claude.exact_model("opus"),
            "claude-opus-5",
            "and the rest survive"
        );
    }
}
