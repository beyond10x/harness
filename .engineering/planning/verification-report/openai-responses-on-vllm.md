---
format: aep.planning-md/1
id: verification-report:openai-responses-on-vllm
kind: verification-report
status: draft
title: The openai-responses wire reaches vLLM v0.27.1 unchanged
summary: A two-turn tool-calling run completed against the deployment's own vLLM image and the production chat template. No wire change is needed.
relations:
- informed_by: initiative:live-evidence
revision: 1
---
## What was measured

Whether this repository's `openai-responses` wire reaches a vLLM endpoint unchanged — the question
that decides whether a RunPod + vLLM route needs a third wire crate or none.

## What ran

| | |
|---|---|
| server | `vllm/vllm-openai:v0.27.1` — the **exact image** the dev deployment's registry names |
| chat template | `Qwen/Qwen3.8-27B-FP8`'s own `chat_template.jinja`, fetched from the Hub and passed as `--chat-template` |
| serve flags | the deployment's, minus `--language-model-only`, the three `--mamba-*` flags and `--kv-cache-dtype fp8` |
| weights | **`Qwen/Qwen3-0.6B`**, not the 27B the deployment serves |
| host | a local RTX 3090, not a RunPod pod |
| client | `b10x-harness run --wire openai-responses`, unmodified |

**This is not `vendor_live` and it is not `provider_emulated`.** It is the real vLLM binary and the
real production chat template, on different weights and a different host. Every claim below is a
claim about *that server's HTTP surface*, which is the property the wire depends on; none of them is
a claim about the 27B model's behaviour, and none may be relayed as one. RunPod was never reached —
the account is unfunded (see the blocker).

## Result: the wire works unchanged

A two-turn run completed: the model called `file_read`, the tool executed, the model answered from
the file's contents.

```
{"kind":"tool-requested","call_id":"chatcmpl-tool-b5d68304f77193d8","name":"file_read",
 "arguments":{"path":"~/.cache/llmgw-work/ws/README.md"}}
{"kind":"tool-completed","call_id":"chatcmpl-tool-b5d68304f77193d8","failed":false}
{"kind":"usage","model":"qwen3.8-27b","input_tokens":1788,"output_tokens":115,"cached_input_tokens":1680}
{"kind":"finished","stop":{"kind":"completed"},"turns":2}
```

Exit 0.

## Every field this wire sends, and what vLLM did with it

Each row was predicted from vLLM's own source at tag `v0.27.1` and then observed at runtime.

| sent, from `crates/harness-responses/src/project.rs` | vLLM v0.27.1 | verdict |
|---|---|---|
| `input[0].role = "developer"` (`:325-329`) | logged `Chat template does not support the 'developer' message role. Converting developer messages to 'system' role.` | **accepted** |
| `include: ["reasoning.encrypted_content"]` (`:341`) | in the allowed `Literal` (`responses/protocol.py:140-151`); returned `encrypted_content: null` on every turn | **accepted, never populated** |
| `prompt_cache_key` (`:354`) | present, documented `"has not been implemented yet and vLLM will ignore it"` (`responses/protocol.py:205-212`) | **accepted, ignored** |
| `store: false` (`:337`) | off by default unless `VLLM_ENABLE_RESPONSES_API_STORE` (`responses/serving.py:203-208`) | **accepted** |
| `stream: true`, `tools`, `tool_choice`, `max_output_tokens`, `temperature`, `top_p`, `reasoning.effort` | all present on `ResponsesRequest` | **accepted** |
| SSE terminated by `data: [DONE]` (`harness-responses/src/lib.rs:62`) | **no sentinel** — `responses/api_router.py:34-45` ends the generator without one | **harmless**, see below |

### The missing `[DONE]` costs nothing

`crates/harness-http/src/sse.rs:91` maps end-of-stream and the sentinel to the same `Ok(None)`, and
`:317` pins an empty stream as valid rather than truncated. Only a stream cut **mid-frame** refuses
(`:292`), and vLLM writes `\n\n` after every event. `Framing::DoneSentinel` is therefore correct on
this route as it stands; no change is needed.

### `prompt_cache_key` being ignored costs nothing either

The concern was that this loop replays the whole conversation every turn, so a route without prompt
caching is quadratic in turns. vLLM's `--enable-prefix-caching` covers it without the hint: the run
measured **1456/1470 = 99%** cached on turn 1 and **1680/1788 = 94%** on turn 2.

### Reasoning is carried as plaintext, not as an opaque item

`include: reasoning.encrypted_content` is accepted but `encrypted_content` comes back `null`; the
reasoning arrives in `content` instead. Replaying the item verbatim (invariant 5) was tested and
returned **HTTP 200** — the `raise ValueError("Encrypted content is not supported.")` at
`responses/utils.py:275` is guarded by `if item.encrypted_content:` and never fires on a null.

## What this does not establish

- Nothing about the 27B model: tool-call quality, reasoning quality, or 32k-context behaviour.
- Nothing about the RunPod pod, its proxy, or a cold start. The account is unfunded.
- Nothing about the `anthropic-messages` wire against the same server. Not measured.
- Nothing about compaction, which needs a run long enough to trigger it.
