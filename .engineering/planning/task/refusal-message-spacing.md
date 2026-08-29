---
format: aep.planning-md/1
id: task:refusal-message-spacing
kind: task
status: implemented
title: The no-endpoint-or-model refusal prints without runs of spaces
relations:
- decomposes: story:missing-model-refuses-by-name
revision: 5
---
## Evidence

- `crates/harness-cli/src/lib.rs:1173` — the refusal string, one source line containing runs of fourteen spaces where a wrapped literal was joined: `"… name a provider in              \`{source}\` with …"`.
- Runtime, `target/release/b10x-harness` from `d1ab5dd`: `XDG_CONFIG_HOME=/nonexistent-dir b10x-harness run --base-url … --input hi --json` prints `error: no endpoint or model: type '--base-url' and '--model', or name a provider in              '/nonexistent-dir/b10x/harness.toml' with …`.
- `README.md:92-97` — this is the message a caller sees on the exit-1 path, which is the path a driver reads.

## What to do

Make the literal a properly continued multi-line string (`\` at each line end) so the message prints
with single spaces. One line; the text itself does not change.

## Done When

The refusal prints as one sentence with no runs of spaces, and the test that covers this path asserts
the printed form.
