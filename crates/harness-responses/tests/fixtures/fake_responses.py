#!/usr/bin/env python3
"""A deterministic local Responses endpoint.

Standard library only, no model, no credential, no network egress. It exists so the whole stack --
HTTP, SSE framing, projection, the loop, the tools -- is proven against a real socket rather than
against a mock of itself. Evidence from here is `provider_emulated`; it is never evidence that a
real provider behaves this way.
"""

import argparse
import json
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MAX_REQUEST_BYTES = 4 * 1024 * 1024
MODEL = "b10x-emulated"

# The scenarios both emulators serve. Declared here and in the Messages emulator, and asserted
# equal by a test: the roadmap's exit criterion for the second wire is that both wires pass the
# same loop suite, and a suite the two sides could name differently is one that could drift
# apart while both stayed green.
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
    "unpublished-tool"
]

# What a delegating parent puts in the sub-task, so this emulator can tell the child's requests
# from the parent's: the child's conversation starts empty and its first user item **is** the task.
DELEGATED = "DELEGATE-TASK"

RECORD_LOCK = threading.Lock()


def response_object(status, output, usage, incomplete=None, error=None):
    return {
        "id": "resp_b10x_001",
        "object": "response",
        "created_at": 1786706400,
        "status": status,
        "model": MODEL,
        "output": output,
        "incomplete_details": incomplete,
        "error": error,
        "usage": usage,
    }


def usage_object(output_tokens):
    return {
        "input_tokens": 42,
        "input_tokens_details": {"cached_tokens": 7},
        "output_tokens": output_tokens,
        "output_tokens_details": {"reasoning_tokens": 0},
        "total_tokens": 42 + output_tokens,
    }


def message_item(text):
    return {
        "id": "msg_b10x_001",
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": [{"type": "output_text", "text": text, "annotations": []}],
    }


def reasoning_item():
    return {
        "id": "rs_b10x_001",
        "type": "reasoning",
        "summary": [],
        "encrypted_content": "OPAQUE-REASONING-BLOB",
    }


def function_call_item(name, arguments):
    return {
        "id": "fc_b10x_001",
        "type": "function_call",
        "status": "completed",
        "name": name,
        "call_id": "call_b10x_001",
        "arguments": json.dumps(arguments, separators=(",", ":")),
    }


def text_events(text, extra_output=()):
    midpoint = len(text) // 2
    output = [*extra_output, message_item(text)]
    events = [
        {"type": "response.created", "response": response_object("in_progress", [], None)},
        {
            "type": "response.output_item.added",
            "output_index": 0,
            "item": {**message_item(""), "content": []},
        },
        {
            "type": "response.output_text.delta",
            "item_id": "msg_b10x_001",
            "output_index": 0,
            "delta": text[:midpoint],
        },
        {
            "type": "response.output_text.delta",
            "item_id": "msg_b10x_001",
            "output_index": 0,
            "delta": text[midpoint:],
        },
        {"type": "response.output_item.done", "output_index": 0, "item": message_item(text)},
        {
            "type": "response.completed",
            "response": response_object("completed", output, usage_object(4)),
        },
    ]
    return events


def function_call_events(name, arguments):
    item = function_call_item(name, arguments)
    added = {**item, "status": "in_progress", "arguments": ""}
    return [
        {"type": "response.created", "response": response_object("in_progress", [], None)},
        {"type": "response.output_item.added", "output_index": 0, "item": added},
        {
            "type": "response.function_call_arguments.delta",
            "item_id": item["id"],
            "output_index": 0,
            "delta": item["arguments"],
        },
        {"type": "response.output_item.done", "output_index": 0, "item": item},
        {
            "type": "response.completed",
            "response": response_object("completed", [item], usage_object(8)),
        },
    ]


def first_user_text(body):
    """The first thing a person (or a delegating parent) said in this conversation."""
    for entry in body.get("input", []):
        if not isinstance(entry, dict) or entry.get("role") != "user":
            continue
        content = entry.get("content")
        if isinstance(content, str):
            return content
        if isinstance(content, list) and content and isinstance(content[0], dict):
            return content[0].get("text") or ""
        return ""
    return ""


def user_items(body):
    """How many user items the conversation carries, one per turn somebody asked for."""
    return sum(
        1
        for entry in body.get("input", [])
        if isinstance(entry, dict) and entry.get("role") == "user"
    )


def has_function_output(body):
    return any(
        isinstance(entry, dict) and entry.get("type") == "function_call_output"
        for entry in body.get("input", [])
    )


def replayed_reasoning(body):
    return [
        entry
        for entry in body.get("input", [])
        if isinstance(entry, dict) and entry.get("type") == "reasoning"
    ]


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
            self.wfile.write(b"data: {not json}\n\n")
            self.wfile.flush()
            return
        for index, event in enumerate(events):
            payload = json.dumps(event)
            if truncate and index == len(events) - 1:
                # Stop mid-event: a client must treat this as truncation, not completion.
                self.wfile.write(b"data: {\"type\": \"response.comp")
                self.wfile.flush()
                return
            self.wfile.write(f"data: {payload}\n\n".encode("utf-8"))
            self.wfile.flush()
            if delay:
                time.sleep(delay)
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()

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
        if self.path != "/v1/responses":
            self._send_json(404, {"error": {"message": "unknown path", "code": "not_found"}})
            return
        length = int(self.headers.get("content-length", "0"))
        if length > MAX_REQUEST_BYTES:
            self._send_json(413, {"error": {"message": "too large", "code": "too_large"}})
            return
        body = json.loads(self.rfile.read(length) or b"{}")
        authorization = self.headers.get("authorization", "")
        Handler.turn_count += 1
        self._record(
            {
                "turn": Handler.turn_count,
                "scenario": Handler.scenario,
                "has_authorization": authorization.startswith("Bearer "),
                # The value itself is never recorded. A fixture that writes a credential to disk
                # is a fixture that leaks one.
                "authorization_length": len(authorization),
                "accept": self.headers.get("accept", ""),
                "model": body.get("model"),
                "store": body.get("store"),
                "stream": body.get("stream"),
                "include": body.get("include"),
                "instructions": body.get("instructions"),
                # The standing instruction now rides at the head of `input` as a developer message,
                # because that is the region this wire's cache prefix is computed over.
                "first_input_role": (body.get("input") or [{}])[0].get("role"),
                "first_input_text": (
                    ((body.get("input") or [{}])[0].get("content") or [{}])[0].get("text")
                ),
                "tool_names": [tool.get("name") for tool in body.get("tools", [])],
                "input": body.get("input", []),
                "max_output_tokens": body.get("max_output_tokens"),
                "replayed_reasoning": replayed_reasoning(body),
            }
        )

        scenario = Handler.scenario
        if scenario == "unauthorized":
            self._send_json(
                401, {"error": {"message": "the key was rejected", "code": "invalid_api_key"}}
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
                self._send_sse(text_events("provider emulation passed"))
        elif scenario == "slow":
            # Paced so a client has time to cancel mid-stream. A cancel that only lands between
            # turns proves nothing about the case that matters.
            self._send_sse(text_events("this answer should never be delivered"), delay=0.5)
        elif scenario == "malformed":
            self._send_sse([], malformed=True)
        elif scenario == "truncated":
            self._send_sse(text_events("never finished"), truncate=True)
        elif scenario == "failed":
            self._send_sse(
                [
                    {
                        "type": "response.failed",
                        "response": response_object(
                            "failed",
                            [],
                            None,
                            error={"message": "upstream exploded", "code": "server_error"},
                        ),
                    }
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
            if has_function_output(body):
                self._send_json(
                    400,
                    {
                        "error": {
                            "message": "that conversation is no longer accepted",
                            "code": "invalid_request_error",
                        }
                    },
                )
            else:
                self._send_sse(function_call_events("file_read", {"path": "README.md"}))
        elif scenario == "incomplete":
            self._send_sse(
                [
                    {
                        "type": "response.incomplete",
                        "response": response_object(
                            "incomplete",
                            [message_item("cut off")],
                            usage_object(2),
                            incomplete={"reason": "max_output_tokens"},
                        ),
                    }
                ]
            )
        elif scenario == "no-usage":
            self._send_sse(
                [
                    {
                        "type": "response.completed",
                        "response": response_object("completed", [message_item("no usage")], None),
                    }
                ]
            )
        elif scenario == "unknown-events":
            self._send_sse(
                [
                    {"type": "response.something.new", "detail": 1},
                    {
                        "type": "response.completed",
                        "response": response_object(
                            "completed",
                            [{"type": "web_search_call", "id": "ws_1"}, message_item("done")],
                            usage_object(3),
                        ),
                    },
                ]
            )
        elif scenario == "bad-arguments":
            item = {
                "id": "fc_1",
                "type": "function_call",
                "name": "tool_invoke",
                "call_id": "call_bad",
                "arguments": "{not json",
            }
            self._send_sse(
                [
                    {
                        "type": "response.completed",
                        "response": response_object("completed", [item], usage_object(3)),
                    }
                ]
            )
        elif scenario == "unpublished-tool":
            if has_function_output(body):
                self._send_sse(text_events("I see, I cannot use that."))
            else:
                self._send_sse(function_call_events("shell.exec", {"cmd": "id"}))
        elif scenario == "tool":
            # The three-verb surface: the model calls `tool_invoke` and names an entry inside it.
            if has_function_output(body):
                self._send_sse(text_events("The file says: hello harness"))
            else:
                self._send_sse(
                    function_call_events(
                        "tool_invoke", {"name": "file_read", "arguments": {"path": "README.md"}}
                    )
                )
        elif scenario == "flat-tool":
            # The flat surface: the model calls the catalogue entry by its own name, with the
            # entry's own arguments. No verb, and nothing nested a level down.
            if has_function_output(body):
                self._send_sse(text_events("The file says: hello harness"))
            else:
                self._send_sse(function_call_events("file_read", {"path": "README.md"}))
        elif scenario == "flat-write":
            # The same surface, asking for an effect: what a run does depends on the approver and
            # on the ceiling, and the second turn reports whichever answer came back.
            if has_function_output(body):
                self._send_sse(text_events("that is what the tool said"))
            else:
                self._send_sse(
                    function_call_events(
                        "file_write", {"path": "note.md", "text": "written by the harness\n"}
                    )
                )
        elif scenario == "dynamic-tool":
            # Bridge mode is the other surface: the tools are the *client's*, registered by name at
            # `thread/start`, and the verbs are not among them. A scenario of its own rather than a
            # flag, because the two surfaces are two different things a model can be offered.
            if has_function_output(body):
                self._send_sse(text_events("The file says: hello harness"))
            else:
                self._send_sse(function_call_events("workspace_read", {"path": "README.md"}))
        elif scenario == "answer-call":
            # Structured output: the model finishes by calling the answer tool, and its arguments
            # are the answer. Nothing is said in prose, because prose is not what was asked for.
            if has_function_output(body):
                self._send_sse(text_events("I have already answered."))
            else:
                self._send_sse(
                    function_call_events(
                        "answer", {"verdict": "ok", "file": "README.md", "bytes": 14}
                    )
                )
        elif scenario == "answer-prose":
            # The same run, from a model that will not call it: one nudge, one more turn in prose,
            # and a stop that is not `completed`.
            self._send_sse(text_events("The readme says hello harness."))
        elif scenario == "answer-stop-hook":
            # Two structured answers. The first is withdrawn by a stop hook, whose reason arrives
            # as one more user item; the second is what the run actually answers, and the only one
            # a consumer reading stdout may ever see.
            if user_items(body) > 1:
                self._send_sse(
                    function_call_events("answer", {"verdict": "second, after the hook"})
                )
            else:
                self._send_sse(function_call_events("answer", {"verdict": "first"}))
        elif scenario == "flow-passes":
            # A walk of a workflow. Every step is one turn that ends in the `answer` call the
            # runner derived for it, and every one of them passes. `gives` carries the name the
            # test's document promises, so the section after it can be checked for having been
            # handed it -- and a document that promises nothing simply never asks for it.
            self._send_sse(
                function_call_events(
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
                function_call_events(
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
                        "error": {
                            "message": "that conversation is no longer accepted",
                            "code": "invalid_request_error",
                        }
                    },
                )
            else:
                self._send_sse(
                    function_call_events(
                        "answer",
                        {"outcome": "passed", "gives": {"specification_id": "SPEC-1"}},
                    )
                )
        elif scenario == "hooks-block":
            # A write the ceiling allows and a hook refuses. The second turn reports whichever
            # answer came back, exactly as it would for a denial.
            if has_function_output(body):
                self._send_sse(text_events("the hook stopped the write"))
            else:
                self._send_sse(
                    function_call_events(
                        "file_write", {"path": "note.md", "text": "written by the harness\n"}
                    )
                )
        elif scenario == "stop-hook":
            # Two answers in prose. The second turn exists only because a stop hook refused the
            # first ending and its reason arrived as one more user item.
            if user_items(body) > 1:
                self._send_sse(text_events("second answer, after the hook"))
            else:
                self._send_sse(text_events("first answer"))
        elif scenario == "delegate":
            # One emulator serves both loops. The child's conversation starts empty, so its first
            # user item is the task itself -- that is what tells the two apart.
            if DELEGATED in first_user_text(body):
                if has_function_output(body):
                    self._send_sse(text_events("README.md says: hello harness"))
                else:
                    self._send_sse(function_call_events("file_read", {"path": "README.md"}))
            elif has_function_output(body):
                self._send_sse(text_events("the delegate read it: hello harness"))
            else:
                self._send_sse(
                    function_call_events(
                        "delegate",
                        {"task": f"{DELEGATED} read README.md and say what it says"},
                    )
                )
        elif scenario == "reasoning":
            if has_function_output(body):
                self._send_sse(text_events("done", extra_output=[reasoning_item()]))
            else:
                events = function_call_events("tool_invoke", {"name": "file_read", "arguments": {"path": "README.md"}})
                terminal = events[-1]
                terminal["response"]["output"] = [
                    reasoning_item(),
                    *terminal["response"]["output"],
                ]
                self._send_sse(events)
        else:
            self._send_sse(text_events("provider emulation passed"))


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
