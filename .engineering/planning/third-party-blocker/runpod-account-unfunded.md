---
format: aep.planning-md/1
id: third-party-blocker:runpod-account-unfunded
kind: third-party-blocker
status: open
title: 'RunPod refuses pod creation: account balance too low'
summary: Every create on the dev account returns a 500 carrying a funding message. No pod can start, on a workstation or in the cluster.
relations:
- blocks: verification-report:openai-responses-on-vllm
revision: 1
---
## What is blocked

Any measurement that needs the real pod: the 27B model's tool-call and reasoning behaviour, a cold
start, the pod proxy's bearer posture, and 32k-context compaction.

## Evidence

RunPod refuses pod creation on the dev account, on every configured GPU type, with a **500**:

```
POST https://rest.runpod.io/v1/pods
{"error":"create pod: Your account balance is too low to rent a pod. Please add funds to your account.","status":500}
```

Observed 2026-08-30 against the key in `secret/b10x-llmgw-runpod`, namespace `b10x`. All three
configured GPU types are in stock and priced (`NVIDIA L40S` 0.79, `NVIDIA RTX A6000` 0.33,
`NVIDIA A40` 0.35, all `secureCloud: true`), so this is funding and not availability.

No pod was created and nothing was billed.

## Consequence beyond this measurement

**The deployed gateway cannot serve a single request.** `b10x-llmgw` is `1/1` in namespace `b10x`
and answers `GET /v1/models` from its registry without touching RunPod, so it looks healthy; the
first inference request would cold-start a pod and get this same refusal, after holding the caller
for the model's `start_wait_seconds` of 1800.

## What clears it

Funds on the RunPod account. Nothing in either repository can.
