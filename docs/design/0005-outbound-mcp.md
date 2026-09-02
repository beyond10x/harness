# 0005 — Outbound MCP is standalone transport with Harness-owned authority

## Decision

Harness consumes the independently released `beyond10x/mcp` client foundation directly. It does
not reach MCP through Connectors, and it does not depend on Connectors. Connectors may consume the
same lower foundation for a governed hosted path; those two products retain different authority and
credential-custody boundaries.

The foundation owns MCP revision negotiation, stdio and Streamable HTTP transports, OAuth
mechanics, bounded lossless descriptors/results and a shared local named registry. Harness owns
which tools a run may publish and invoke. A server's `tools/list` is input to review, never a grant.

## Publication

`--mcp-profile <FILE>` is repeatable. A profile names one connection, pins the SHA-256 of the whole
registry and the exact frozen tools snapshot, and explicitly maps a subset of original names to
provider-safe published names. Every entry supplies locally reviewed prose, an `Envelope`, and
static subjects. Server annotations stay in the foundation's raw descriptor and are never read into
those fields.

Preparation connects and lists once before the first model request. A registry or snapshot mismatch
refuses the run. Notifications do not mutate the active list; a changed server requires a new
snapshot and a reviewed profile update. Names collide closed across local tools and all MCP
profiles.

The two reviewed digests come from `b10x-mcp config check` and
`b10x-mcp connections check <connection>`. A minimal read-only profile is:

```toml
connection = "issues"
registry-sha256 = "<64 hex characters from config check>"
snapshot-sha256 = "<64 hex characters from connections check>"

[[tools]]
remote = "read_issue"
publish = "mcp_issues_read_issue"
description = "Read one issue from the reviewed issue service"
subjects = [{ kind = "host", value = "issues.example.com" }]

[tools.envelope]
effects = ["read", "network"]
risk = "low"
idempotency = "idempotent"
access = ["network", "secret"]
```

`b10x-mcp tools snapshot <connection>` is the review body. It preserves the server's complete
descriptor; the profile above deliberately re-states only the authority Harness needs.

The combined port delegates `specs`, `subjects`, `operation`, `invoked`, `reachable_specs`, calls
and deadlines. It does not fork while an outbound connection is attached: one MCP session is not
silently shared across delegate threads. Calls therefore enter the existing unpublished-tool,
approval, hook, bound, budget and failed-outcome sequence with no MCP exception.

## Configuration and evidence

Absent `--mcp-registry`, Harness reads the same XDG registry and local OAuth state as `b10x-mcp`.
The registry names credential sources, not values; stdio receives only explicitly inherited
variables. `started.mcp` and `b10x-harness tools` carry connection, registry/profile/snapshot
digests and negotiated protocol revision, never credentials or policy bodies.

## Why not through Connectors

Requiring Connectors would invert Harness's dependency boundary, couple a local loop to a hosted
catalog/grant service and make offline stdio depend on infrastructure it does not need. Direct MCP
is therefore the standalone path. Connectors is the governed distribution path for reviewed MCP
catalog revisions, grants, hosted custody and egress—not a transport dependency of Harness.
