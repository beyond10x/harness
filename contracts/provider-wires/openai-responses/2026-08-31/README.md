# openai-responses wire — 2026-08-31

This immutable cut pins the exact compact JSON request bytes (including their trailing newline),
every non-secret request header and value, the `[DONE]` sentinel policy, and the complete event
inventory the production decoder interprets. Unknown events and output items remain opaque and
warn; they are not members of the interpreted inventory.

The fixtures are synthetic. `provider_emulated` means no provider was contacted.
