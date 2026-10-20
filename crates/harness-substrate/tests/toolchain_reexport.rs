//! The `tool_toolchain` types `b10x-harness-substrate`'s public API names are re-exported from it.
//!
//! A consumer (agentide `crates/agentide-substrate`) resolves its own `Registry` and passes the
//! providers into [`Toolchain::from_providers`]. For the call to type-check it must hand over the
//! exact `tool_toolchain::ResolvedProvider` the API accepts, and it should be able to name that type
//! — and `Registry` — from `b10x-harness-substrate` without pinning `b10x-tool-toolchain` itself.
//! This is the compile-time proof: the forwarding function below only compiles if the re-exported
//! type is the one `from_providers` declares.

use b10x_harness_substrate::{Toolchain, tool_toolchain};

/// Compiles only when `b10x_harness_substrate::tool_toolchain::ResolvedProvider` is exactly the type
/// `Toolchain::from_providers` names. A different or second-version crate here would not type-check.
fn forwards(providers: Vec<tool_toolchain::ResolvedProvider>) -> Result<Toolchain, String> {
    Toolchain::from_providers(providers)
}

#[test]
fn the_reexport_names_the_types_the_public_api_accepts() {
    // The coercion is the check: this function pointer has the shape of `from_providers` only if the
    // re-exported `ResolvedProvider` is identical to the one the signature declares.
    let _: fn(Vec<tool_toolchain::ResolvedProvider>) -> Result<Toolchain, String> = forwards;
    // `Registry` is reachable through the re-export too, so a consumer builds its providers here.
    let registry = tool_toolchain::Registry::builtins().expect("the built-in registry");
    assert!(!registry.names().is_empty(), "the registry has built-ins");
    // An empty provider set is accepted, proving the whole path type-checks end to end.
    let toolchain = forwards(Vec::new()).expect("no providers is a valid, empty toolchain");
    assert!(toolchain.programs().is_empty());
}
