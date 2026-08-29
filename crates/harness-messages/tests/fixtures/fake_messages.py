#!/usr/bin/env python3
"""A deterministic local Messages endpoint.

Standard library only, no model, no credential, no network egress. It exists so the whole stack --
HTTP, SSE framing, projection, the loop, the tools -- is proven against a real socket rather than
against a mock of itself. Evidence from here is `provider_emulated`; it is never evidence that a
real provider behaves this way.

**The scenario names are the same as `harness-responses`'s emulator, deliberately.** The roadmap's
exit criterion for the second wire is that both wires pass the same loop suite, and a suite the two
sides could name differently is one that could drift apart while both stayed green. A test asserts
the two `--list-scenarios` answers are equal.
"""

import argparse
import json
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MAX_REQUEST_BYTES = 4 * 1024 * 1024
MODEL = "b10x-emulated"

# The scenarios both emulators serve. Kept in one obvious place on each side so the equality test
# reads as a comparison of two declarations rather than of two implementations.
SCENARIOS = [
    "answer-call",
    "answer-prose",
    "answer-stop-hook",
    "bad-arguments",
    "cold",
    "cold-once",
    "delegate",
    "dynamic-tool",
    "failed",
    "fails-after-turn",
    "flat-tool",
    "flat-write",
    "flow-dies-mid-step",
    "flow-fails-second",
    "flow-passes",
    "hooks-block",
    "incomplete",
    "malformed",
    "no-usage",
    "reasoning",
    "slow",
    "stop-hook",
    "text",
    "tool",
    "truncated",
    "unauthorized",
    "unknown-events",
    "unpublished-tool",
]

# What a delegating parent puts in the sub-task, so this emulator can tell the child's requests
# from the parent's: the child's conversation starts empty and its first user message **is** the
# task. The same sentinel as the Responses emulator, for the same reason the scenarios are shared.
DELEGATED = "DELEGATE-TASK"

RECORD_LOCK = threading.Lock()


def usage_object(input_tokens=42, cache_read=7, cache_creation=None, output_tokens=1):
    usage = {
        # Disjoint on this wire: `input_tokens` excludes both cache classes. The client sums them.
        "input_tokens": input_tokens,
        "cache_read_input_tokens": cache_read,
        "output_tokens": output_tokens,
    }
    if cache_creation is not None:
        usage["cache_creation_input_tokens"] = cache_creation
    return usage


def message_start(usage=None):
    return {
        "type": "message_start",
        "message": {
            "id": "msg_b10x_001",
            "type": "message",
            "role": "assistant",
            "model": MODEL,
            "content": [],
            "stop_reason": None,
            "usage": usage_object() if usage is None else usage,
        },
    }


def message_end(stop_reason, output_tokens):
    return [
        {
            "type": "message_delta",
            "delta": {"stop_reason": stop_reason, "stop_sequence": None},
            "usage": {"output_tokens": output_tokens},
        },
        {"type": "message_stop"},
    ]


def text_events(text, index=0):
    midpoint = len(text) // 2
    return [
        {
            "type": "content_block_start",
            "index": index,
            "content_block": {"type": "text", "text": ""},
        },
        {
            "type": "content_block_delta",
            "index": index,
            "delta": {"type": "text_delta", "text": text[:midpoint]},
        },
        {
            "type": "content_block_delta",
            "index": index,
            "delta": {"type": "text_delta", "text": text[midpoint:]},
        },
        {"type": "content_block_stop", "index": index},
    ]


def thinking_events(index=0):
    return [
        {
            "type": "content_block_start",
            "index": index,
            "content_block": {"type": "thinking", "thinking": ""},
        },
        {
            "type": "content_block_delta",
            "index": index,
            "delta": {"type": "thinking_delta", "thinking": "OPAQUE-REASONING-BLOB"},
        },
        {
            "type": "content_block_delta",
            "index": index,
            "delta": {"type": "signature_delta", "signature": "OPAQUE-SIGNATURE"},
        },
        {"type": "content_block_stop", "index": index},
    ]


def tool_use_events(name, arguments, index=0, partial_json=None):
    encoded = json.dumps(arguments, separators=(",", ":")) if partial_json is None else partial_json
    midpoint = len(encoded) // 2
    return [
        {
            "type": "content_block_start",
            "index": index,
            "content_block": {
                "type": "tool_use",
                "id": "toolu_b10x_001",
                "name": name,
                "input": {},
            },
        },
        {
            "type": "content_block_delta",
            "index": index,
            "delta": {"type": "input_json_delta", "partial_json": encoded[:midpoint]},
        },
        {
            "type": "content_block_delta",
            "index": index,
            "delta": {"type": "input_json_delta", "partial_json": encoded[midpoint:]},
        },
        {"type": "content_block_stop", "index": index},
    ]


def answered(text, leading=()):
    index = 1 if leading else 0
    return [message_start(), *leading, *text_events(text, index=index), *message_end("end_turn", 4)]


def called(name, arguments, leading=()):
    index = 1 if leading else 0
    return [
        message_start(),
        *leading,
        *tool_use_events(name, arguments, index=index),
        *message_end("tool_use", 8),
    ]


def first_user_text(body):
    """The first thing a person (or a delegating parent) said in this conversation."""
    for message in body.get("messages", []):
        if message.get("role") != "user":
            continue
        content = message.get("content")
        if isinstance(content, str):
            return content
        if isinstance(content, list):
            for block in content:
                if isinstance(block, dict) and block.get("type") == "text":
                    return block.get("text") or ""
        return ""
    return ""


def user_texts(body):
    """How many user messages carry text, one per turn somebody asked for.

    Tool results travel as user messages on this wire, so they are not counted: what is wanted is
    how many times this conversation was addressed, not how many blocks it holds.
    """
    count = 0
    for message in body.get("messages", []):
        if message.get("role") != "user":
            continue
        content = message.get("content")
        if isinstance(content, str):
            count += 1
        elif isinstance(content, list) and any(
            isinstance(block, dict) and block.get("type") == "text" for block in content
        ):
            count += 1
    return count


def has_tool_result(body):
    """Whether the transcript already carries this run's answer to a tool call."""
    for message in body.get("messages", []):
        content = message.get("content")
        if not isinstance(content, list):
            continue
        for block in content:
            if isinstance(block, dict) and block.get("type") == "tool_result":
                return True
    return False


def replayed_thinking(body):
    blocks = []
    for message in body.get("messages", []):
        content = message.get("content")
        if not isinstance(content, list):
            continue
        blocks.extend(
            block
            for block in content
            if isinstance(block, dict)
            and block.get("type") in ("thinking", "redacted_thinking")
        )
    return blocks


class Handler(BaseHTTPRequestHandler):
    scenario = "text"
    # How many requests this process has served, for scenarios whose answer depends on it.
    requests = 0
    record_path = None
    turn_count = 0

    def log_message(self, *_args):
        """Silence per-request logging; the test reads the record file instead."""

    def _record(self, entry):
        if not Handler.record_path:
            return
        with RECORD_LOCK:
            with open(Handler.record_path, "a", encoding="utf-8") as handle:
                handle.write(json.dumps(entry) + "\n")

    def _send_sse(self, events, truncate=False, malformed=False, delay=0.0):
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("cache-control", "no-store")
        self.end_headers()
        if malformed:
            self.wfile.write(b"event: message_start\ndata: {not json}\n\n")
            self.wfile.flush()
            return
        for index, event in enumerate(events):
            payload = json.dumps(event)
            if truncate and index == len(events) - 1:
                # Stop mid-event: a client must treat this as truncation, not completion.
                self.wfile.write(b"event: message_stop\ndata: {\"type\": \"message_st")
                self.wfile.flush()
                return
            # Framed with the named `event:` field a real stream carries. The client reads the
            # payload's own `type` and not the frame name, and this is what proves it.
            self.wfile.write(f"event: {event['type']}\ndata: {payload}\n\n".encode("utf-8"))
            self.wfile.flush()
            if delay:
                time.sleep(delay)

    def _send_json(self, status, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def handle_one_request(self):
        # A client that cancels mid-stream hangs up mid-write. That is the behaviour under test,
        # not a fault, so it must not print a traceback that reads like one.
        try:
            super().handle_one_request()
        except (BrokenPipeError, ConnectionResetError):
            self.close_connection = True

    def do_POST(self):  # noqa: N802 - the base class fixes this name
        if self.path != "/v1/messages":
            self._send_json(
                404, {"type": "error", "error": {"type": "not_found_error", "message": "unknown path"}}
            )
            return
        length = int(self.headers.get("content-length", "0"))
        if length > MAX_REQUEST_BYTES:
            self._send_json(
                413, {"type": "error", "error": {"type": "request_too_large", "message": "too large"}}
            )
            return
        body = json.loads(self.rfile.read(length) or b"{}")
        api_key = self.headers.get("x-api-key")
        authorization = self.headers.get("authorization")
        Handler.turn_count += 1
        system = body.get("system") or []
        first_message = (body.get("messages") or [{}])[0]
        self._record(
            {
                "turn": Handler.turn_count,
                "scenario": Handler.scenario,
                # Which header carried the credential, never what it was. A fixture that writes a
                # credential to disk is a fixture that leaks one.
                "credential_header": "x-api-key"
                if api_key
                else ("authorization" if authorization else None),
                "credential_length": len(api_key or authorization or ""),
                "anthropic_version": self.headers.get("anthropic-version"),
                "anthropic_beta": self.headers.get("anthropic-beta"),
                "accept": self.headers.get("accept", ""),
                "model": body.get("model"),
                "stream": body.get("stream"),
                "max_tokens": body.get("max_tokens"),
                # A block list rather than a string, so it can carry a cache breakpoint.
                "system_text": (system[0] if system else {}).get("text"),
                "system_cache_control": (system[0] if system else {}).get("cache_control"),
                "first_message_role": first_message.get("role"),
                "message_roles": [message.get("role") for message in body.get("messages", [])],
                "tool_names": [tool.get("name") for tool in body.get("tools", [])],
                "messages": body.get("messages", []),
                "replayed_thinking": replayed_thinking(body),
                # Nothing may be retained on the far side (AGENTS.md invariant 4).
                "conversation_id": body.get("conversation_id"),
            }
        )

        scenario = Handler.scenario
        if scenario == "unauthorized":
            self._send_json(
                401,
                {
                    "type": "error",
                    "error": {"type": "authentication_error", "message": "the key was rejected"},
                },
            )
        elif scenario == "cold":
            self.send_response(503)
            self.send_header("retry-after", "5")
            self.end_headers()
        elif scenario == "cold-once":
            # 503 once, then answer. The shape a gateway starting a backend actually has, and the
            # one that tells a retry apart from a slower failure.
            Handler.requests += 1
            if Handler.requests == 1:
                self.send_response(503)
                self.send_header("retry-after", "0")
                self.end_headers()
            else:
                self._send_sse(answered("provider emulation passed"))
        elif scenario == "slow":
            # Paced so a client has time to cancel mid-stream. A cancel that only lands between
            # turns proves nothing about the case that matters.
            self._send_sse(answered("this answer should never be delivered"), delay=0.5)
        elif scenario == "malformed":
            self._send_sse([], malformed=True)
        elif scenario == "truncated":
            self._send_sse(answered("never finished"), truncate=True)
        elif scenario == "failed":
            self._send_sse(
                [
                    message_start(),
                    {
                        "type": "error",
                        "error": {"type": "api_error", "message": "upstream exploded"},
                    },
                ]
            )
        elif scenario == "fails-after-turn":
            # A turn is bought and answered -- it carries usage, and it asks for one tool call the
            # loop comes back from -- and the request after it is refused. The only scenario where a
            # run breaks on the wire with spend already on the meter, which is the case a session
            # has to keep the figures of.
            #
            # 400 rather than a 5xx or a 429: `harness_http::status_error` maps it to `Refused` and
            # not retriable, so the wire gives up on the first request and this costs no wall clock.
            # A retriable status reaches the same failure after sleeping 1 + 2 + 4 s of back-off.
            if has_tool_result(body):
                self._send_json(
                    400,
                    {
                        "type": "error",
                        "error": {
                            "type": "invalid_request_error",
                            "message": "that conversation is no longer accepted",
                        },
                    },
                )
            else:
                self._send_sse(called("file_read", {"path": "README.md"}))
        elif scenario == "incomplete":
            self._send_sse(
                [
                    message_start(),
                    *text_events("cut off"),
                    *message_end("max_tokens", 2),
                ]
            )
        elif scenario == "no-usage":
            self._send_sse(
                [
                    {
                        "type": "message_start",
                        "message": {"id": "msg_b10x_001", "role": "assistant", "model": MODEL},
                    },
                    *text_events("no usage"),
                    {"type": "message_delta", "delta": {"stop_reason": "end_turn"}},
                    {"type": "message_stop"},
                ]
            )
        elif scenario == "unknown-events":
            self._send_sse(
                [
                    message_start(),
                    {"type": "message_something_new", "detail": 1},
                    {
                        "type": "content_block_start",
                        "index": 0,
                        "content_block": {"type": "server_tool_use", "id": "srvtoolu_1"},
                    },
                    {"type": "content_block_stop", "index": 0},
                    *text_events("done", index=1),
                    *message_end("end_turn", 3),
                ]
            )
        elif scenario == "bad-arguments":
            self._send_sse(
                [
                    message_start(),
                    *tool_use_events("tool_invoke", None, partial_json="{not json"),
                    *message_end("tool_use", 3),
                ]
            )
        elif scenario == "unpublished-tool":
            if has_tool_result(body):
                self._send_sse(answered("I see, I cannot use that."))
            else:
                self._send_sse(called("shell_exec", {"cmd": "id"}))
        elif scenario == "flat-tool":
            # The flat surface: the model calls the catalogue entry by its own name, with the
            # entry's own arguments. No verb, and nothing nested a level down.
            if has_tool_result(body):
                self._send_sse(answered("The file says: hello harness"))
            else:
                self._send_sse(called("file_read", {"path": "README.md"}))
        elif scenario == "flat-write":
            # The same surface, asking for an effect: what a run does depends on the approver and
            # on the ceiling, and the second turn reports whichever answer came back.
            if has_tool_result(body):
                self._send_sse(answered("that is what the tool said"))
            else:
                self._send_sse(
                    called("file_write", {"path": "note.md", "text": "written by the harness\n"})
                )
        elif scenario == "tool":
            # The three-verb surface: the model calls `tool_invoke` and names an entry inside it.
            if has_tool_result(body):
                self._send_sse(answered("The file says: hello harness"))
            else:
                self._send_sse(
                    called(
                        "tool_invoke",
                        {"name": "file_read", "arguments": {"path": "README.md"}},
                    )
                )
        elif scenario == "dynamic-tool":
            # Bridge mode is the other surface: the tools are the *client's*, registered by name at
            # `thread/start`, and the verbs are not among them.
            if has_tool_result(body):
                self._send_sse(answered("The file says: hello harness"))
            else:
                self._send_sse(called("workspace_read", {"path": "README.md"}))
        elif scenario == "answer-call":
            # Structured output: the model finishes by calling the answer tool, and its arguments
            # are the answer. Nothing is said in prose, because prose is not what was asked for.
            if has_tool_result(body):
                self._send_sse(answered("I have already answered."))
            else:
                self._send_sse(
                    called("answer", {"verdict": "ok", "file": "README.md", "bytes": 14})
                )
        elif scenario == "answer-prose":
            # The same run, from a model that will not call it: one nudge, one more turn in prose,
            # and a stop that is not `completed`.
            self._send_sse(answered("The readme says hello harness."))
        elif scenario == "answer-stop-hook":
            # Two structured answers. The first is withdrawn by a stop hook, whose reason arrives
            # as one more user message; the second is what the run actually answers, and the only
            # one a consumer reading stdout may ever see.
            if user_texts(body) > 1:
                self._send_sse(called("answer", {"verdict": "second, after the hook"}))
            else:
                self._send_sse(called("answer", {"verdict": "first"}))
        elif scenario == "flow-passes":
            # A walk of a workflow. Every step is one turn that ends in the `answer` call the
            # runner derived for it, and every one of them passes. `gives` carries the name the
            # test's document promises, so the section after it can be checked for having been
            # handed it -- and a document that promises nothing simply never asks for it.
            self._send_sse(
                called(
                    "answer",
                    {
                        "outcome": "passed",
                        "note": f"step {Handler.turn_count} did what it was asked",
                        "gives": {"specification_id": "SPEC-1"},
                    },
                )
            )
        elif scenario == "flow-fails-second":
            # The same walk with the second step answering `failed`: its section does not come out
            # clean, and whatever needed that section is skipped rather than run.
            self._send_sse(
                called(
                    "answer",
                    {
                        "outcome": "failed" if Handler.turn_count == 2 else "passed",
                        "note": f"step {Handler.turn_count}",
                        "gives": {"specification_id": "SPEC-1"},
                    },
                )
            )
        elif scenario == "flow-dies-mid-step":
            # The wire breaks in the middle of a walk: a broken wire is nobody's failed step, so
            # the flow aborts rather than recording a network blip as a failure.
            #
            # 400 rather than a closed socket or a 5xx, for the reason `fails-after-turn` gives:
            # `harness_http::status_error` maps it to `Refused` and not retriable, so the failure
            # lands on the first request and costs no wall clock, while a stream cut mid-event is
            # a truncation the loop retries.
            if Handler.turn_count >= 2:
                self._send_json(
                    400,
                    {
                        "type": "error",
                        "error": {
                            "type": "invalid_request_error",
                            "message": "that conversation is no longer accepted",
                        },
                    },
                )
            else:
                self._send_sse(
                    called(
                        "answer",
                        {"outcome": "passed", "gives": {"specification_id": "SPEC-1"}},
                    )
                )
        elif scenario == "hooks-block":
            # A write the ceiling allows and a hook refuses. The second turn reports whichever
            # answer came back, exactly as it would for a denial.
            if has_tool_result(body):
                self._send_sse(answered("the hook stopped the write"))
            else:
                self._send_sse(
                    called("file_write", {"path": "note.md", "text": "written by the harness\n"})
                )
        elif scenario == "stop-hook":
            # Two answers in prose. The second turn exists only because a stop hook refused the
            # first ending and its reason arrived as one more user message.
            if user_texts(body) > 1:
                self._send_sse(answered("second answer, after the hook"))
            else:
                self._send_sse(answered("first answer"))
        elif scenario == "delegate":
            # One emulator serves both loops. The child's conversation starts empty, so its first
            # user message is the task itself -- that is what tells the two apart.
            if DELEGATED in first_user_text(body):
                if has_tool_result(body):
                    self._send_sse(answered("README.md says: hello harness"))
                else:
                    self._send_sse(called("file_read", {"path": "README.md"}))
            elif has_tool_result(body):
                self._send_sse(answered("the delegate read it: hello harness"))
            else:
                self._send_sse(
                    called(
                        "delegate",
                        {"task": f"{DELEGATED} read README.md and say what it says"},
                    )
                )
        elif scenario == "reasoning":
            leading = thinking_events(index=0)
            if has_tool_result(body):
                self._send_sse(answered("done", leading=leading))
            else:
                self._send_sse(
                    called("tool_invoke", {"name": "file_read", "arguments": {"path": "README.md"}},
                           leading=leading)
                )
        else:
            self._send_sse(answered("provider emulation passed"))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--scenario", default="text")
    parser.add_argument("--record")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument(
        "--list-scenarios",
        action="store_true",
        help="print the scenario names this emulator serves and exit",
    )
    arguments = parser.parse_args()

    if arguments.list_scenarios:
        print(json.dumps(SCENARIOS), flush=True)
        return 0

    Handler.scenario = arguments.scenario
    Handler.record_path = arguments.record

    server = ThreadingHTTPServer((arguments.host, 0), Handler)
    host, port = server.server_address[0], server.server_address[1]
    print(
        json.dumps({"base_url": f"http://{host}:{port}/v1", "scenario": arguments.scenario}),
        flush=True,
    )
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    sys.exit(main())
