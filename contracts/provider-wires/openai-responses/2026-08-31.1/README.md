# openai-responses wire — 2026-08-31.1

This immutable cut pins the exact compact JSON request bytes (including their trailing newline),
every non-secret request header and value, the `[DONE]` sentinel policy, and the complete event
inventory the production decoder interprets. Unknown events and output items remain opaque and
warn; they are not members of the interpreted inventory.

This successor adds the route's `keepalive` progress marker to that inventory. It advances no turn
state, emits no warning, and is not replayed as an opaque conversation item. No request byte,
header, output item, terminal rule, or other accepted event changed from `2026-08-31`.

The fixtures are synthetic. `provider_emulated` means no provider was contacted.
