//! The pinned method inventory this server implements.
//!
//! These lists are a deliberate copy of the client inventory `runtime/agent` pins for Codex
//! `0.145.0`. Copying rather than depending is the point: this component has no monorepo
//! dependency, and a protocol is a better seam between two components than a shared crate. The
//! copy is checked against `contracts/app-server-profile/`, so drift shows up as a failing gate
//! rather than as a bridge that mysteriously stops driving this server.

/// The profile identifier the pinned client negotiates.
///
/// Deliberately the operation-tools profile rather than the plain `codex-app-server-stdio-v2`.
/// That plain id is the client's *stable* profile, and a stable client registers no dynamic tools,
/// refuses `item/tool/call` as an out-of-profile server method, and cannot classify a
/// `dynamicToolCall` item at all. Declaring it while emitting tool frames would have produced a
/// server that looked compatible and failed at the first tool call.
pub const PROFILE: &str = "codex-app-server-stdio-v2-dynamic-operation-tools-experimental";

/// The capability a client must negotiate before this server will accept registered tools.
pub const EXPERIMENTAL_API_CAPABILITY: &str = "experimentalApi";

/// The product string this server reports at `initialize`.
///
/// Deliberately not `codex-cli`. A bridge reading this must be able to tell which implementation
/// answered, and a server that impersonates the vendor makes an incident unreadable.
pub const PRODUCT: &str = "b10x-harness";

/// Requests and notifications this server accepts.
pub const CLIENT_METHODS: &[&str] = &[
    "initialize",
    "initialized",
    "thread/start",
    "turn/interrupt",
    "turn/start",
];

/// Pinned client methods this server deliberately refuses, by name.
///
/// Refusing beats a silent success: a bridge that resumes a thread this server never retained, or
/// steers a turn it cannot redirect, would be told the operation worked.
pub const REFUSED_CLIENT_METHODS: &[&str] = &["thread/resume", "turn/steer"];

/// Notifications and requests this server emits.
pub const SERVER_METHODS: &[&str] = &[
    "item/agentMessage/delta",
    "item/completed",
    "item/started",
    "item/tool/call",
    "thread/started",
    "thread/tokenUsage/updated",
    "turn/completed",
    "turn/started",
];

/// The item discriminator the pinned client maps onto a generic tool call.
pub const DYNAMIC_TOOL_ITEM: &str = "dynamicToolCall";

/// Terminal turn statuses the pinned client accepts. Anything else is an unknown-status refusal
/// on its side, so this server emits only these three.
pub const TERMINAL_STATUSES: &[&str] = &["completed", "failed", "interrupted"];

/// Largest single JSON-RPC frame accepted from the client.
///
/// Matches the pinned client's `MAX_LINE_BYTES`, so neither side refuses a frame the other
/// considers legal.
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

/// Largest text this server returns for one tool call.
///
/// Matches the pinned client's `MAX_DYNAMIC_TOOL_RESPONSE_BYTES`. A smaller bound here would
/// refuse answers the client is entitled to send, and tell the model a real result was too large.
pub const MAX_TOOL_RESPONSE_BYTES: usize = 256 * 1024;

/// Largest number of tools a client may register on one thread.
pub const MAX_DYNAMIC_TOOLS: usize = 512;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inventories_are_sorted_and_disjoint() {
        for list in [CLIENT_METHODS, REFUSED_CLIENT_METHODS, SERVER_METHODS] {
            let mut sorted = list.to_vec();
            sorted.sort_unstable();
            assert_eq!(
                list,
                sorted.as_slice(),
                "inventories stay sorted for review"
            );
        }
        for method in REFUSED_CLIENT_METHODS {
            assert!(
                !CLIENT_METHODS.contains(method),
                "`{method}` cannot be both served and refused"
            );
        }
    }

    #[test]
    fn the_server_never_claims_to_be_the_vendor() {
        assert_ne!(PRODUCT, "codex-cli");
        assert!(PRODUCT.starts_with("b10x"));
    }
}
