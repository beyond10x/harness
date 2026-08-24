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
        elif scenario == "dynamic-tool":
            # Bridge mode is the other surface: the tools are the *client's*, registered by name at
            # `thread/start`, and the verbs are not among them. A scenario of its own rather than a
            # flag, because the two surfaces are two different things a model can be offered.
            if has_function_output(body):
                self._send_sse(text_events("The file says: hello harness"))
            else:
                self._send_sse(function_call_events("workspace_read", {"path": "README.md"}))
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
    arguments = parser.parse_args()

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
