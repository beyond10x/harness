# Messages wire 2026-09-07

This additive cut preserves provider-declared refusal diagnostics as a neutral warning. It retains the previous request bytes and headers. `refusal-stream.sse` and `refusal-diagnostic.json` are synthetic (`provider_emulated`), not observations of any account. A refusal remains incomplete, with no retry or model change. Missing/null diagnostics are reported as not supplied; fields exceeding the declared bounds or carrying terminal controls are explicitly omitted. Only category and explanation are exposed; other metadata is not copied into the warning.

The independent provider-contract checker validates fixture membership, digest, refusal shape and diagnostic mapping. The Messages contract test decodes the same stream and verifies partial output, warning, and incomplete terminal state. Earlier released cuts remain unchanged.

Source: https://platform.claude.com/docs/en/test-and-evaluate/strengthen-guardrails/handle-streaming-refusals
