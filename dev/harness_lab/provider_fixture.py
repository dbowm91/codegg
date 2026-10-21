"""Local OpenAI-compatible stream used to exercise the native broker without a paid API."""
from __future__ import annotations

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import re
import threading
from typing import Any


#: Fault kinds the fixture can inject on the request that follows the tool calls.
#:
#: `transport` truncates the response body so the provider observes a transport
#: failure rather than a completed response. `http_error` answers with a 503 so
#: the provider observes an API-level failure. Both are provider-free.
FAULT_TRANSPORT = "transport"
FAULT_HTTP_ERROR = "http_error"
FAULT_KINDS = (FAULT_TRANSPORT, FAULT_HTTP_ERROR)


class FixtureProvider:
    def __init__(self, tool_name: str | list[dict[str, Any]], arguments: dict[str, Any] | None = None,
                 stall_after_tools: bool = False, require_context_text: str | None = None,
                 fail_after_tools: bool = False, fault_kind: str = FAULT_TRANSPORT) -> None:
        self.calls_to_make = ([{"name": tool_name, "arguments": arguments or {}}]
                              if isinstance(tool_name, str) else tool_name)
        self.calls = 0
        self.stall_after_tools = stall_after_tools
        self.require_context_text = require_context_text
        self.fail_after_tools = fail_after_tools
        if fault_kind not in FAULT_KINDS:
            raise ValueError(f"unsupported fixture fault kind: {fault_kind}")
        self.fault_kind = fault_kind
        self.stalled = threading.Event()
        self.release_stall = threading.Event()
        self.request_summaries: list[dict[str, Any]] = []
        self._lock = threading.Lock()
        owner = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, _format: str, *_args: Any) -> None:
                return

            def _json(self, value: dict[str, Any]) -> None:
                body = json.dumps(value).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def do_GET(self) -> None:
                if self.path == "/v1/models":
                    self._json({"object": "list", "data": [{"id": "harness-fixture", "object": "model", "owned_by": "local"}]})
                else:
                    self.send_error(404)

            def do_POST(self) -> None:
                if self.path != "/v1/chat/completions":
                    self.send_error(404)
                    return
                length = int(self.headers.get("Content-Length", "0"))
                request = json.loads(self.rfile.read(length))
                tools = request.get("tools", [])
                names = [item.get("function", {}).get("name") for item in tools]
                call_index = owner.calls
                expected = owner.calls_to_make[min(call_index, len(owner.calls_to_make) - 1)]["name"]
                if call_index < len(owner.calls_to_make) and expected not in names:
                    self.send_error(422, f"expected native {expected} tool schema")
                    return
                with owner._lock:
                    owner.calls += 1
                    call = owner.calls
                    messages = request.get("messages", [])
                    tool_messages = [str(message.get("content", "")) for message in messages if message.get("role") == "tool"]
                    called_tools = [call.get("function", {}).get("name")
                                    for message in messages if isinstance(message, dict)
                                    for call in message.get("tool_calls", []) if isinstance(call, dict)]
                    tool_message_names = [message.get("name") for message in messages
                                          if message.get("role") == "tool"]
                    context_read_used = "context_read" in called_tools or "context_read" in tool_message_names
                    recovered = owner.require_context_text is None or any(owner.require_context_text in content for content in tool_messages)
                    owner.request_summaries.append({"call": call, "model": request.get("model"), "tool_names": names,
                                                    "message_roles": [message.get("role") for message in messages],
                                                    "called_tools": called_tools,
                                                    "tool_message_names": tool_message_names,
                                                    "tool_message_bytes": [len(content.encode()) for content in tool_messages],
                                                    "tool_message_has_context_handle": any("Handle: ctx://" in content for content in tool_messages),
                                                    "tool_message_total_bytes": [int(match.group(1)) for content in tool_messages
                                                        for match in [re.search(r"Bytes:\s*(\d+) total", content)] if match],
                                                    "tool_message_has_required_text": (owner.require_context_text is not None
                                                        and any(owner.require_context_text in content for content in tool_messages)),
                                                    "context_read_used": context_read_used,
                                                    "context_recovered": recovered})
                if call <= len(owner.calls_to_make):
                    fixture_call = json.loads(json.dumps(owner.calls_to_make[call - 1]))
                    if "{artifact_handle}" in json.dumps(fixture_call):
                        tool_content = next((str(message.get("content", "")) for message in reversed(request.get("messages", []))
                                             if message.get("role") == "tool" and "Handle:" in str(message.get("content", ""))), "")
                        handle = re.search(r"Handle:\s*(ctx://[^\s]+)", tool_content)
                        if handle is None:
                            self.send_error(422, "expected projected tool output with a context artifact handle")
                            return
                        fixture_call["arguments"] = json.loads(json.dumps(fixture_call["arguments"]).replace(
                            "{artifact_handle}", handle.group(1)))
                    chunks = [
                        {"id": f"fixture-{call}", "object": "chat.completion.chunk",
                         "choices": [{"index": 0,
                                      "delta": {
                                          "role": "assistant",
                                          "tool_calls": [{
                                              "index": 0, "id": f"call-harness-{call}", "type": "function",
                                              "function": {"name": fixture_call["name"],
                                                           "arguments": json.dumps(fixture_call["arguments"])},
                                          }],
                                      },
                                      "finish_reason": None,
                                      }],
                         },
                        {"id": f"fixture-{call}", "object": "chat.completion.chunk", "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]},
                    ]
                    if owner.fail_after_tools:
                        # The fault lands AFTER a metered attempt: this tool-call
                        # response also reports usage, so the failed turn's
                        # aggregate is non-empty and the failure path can
                        # distinguish "spent something" from "never reached
                        # the provider".
                        chunks.append({"id": f"fixture-{call}", "object": "chat.completion.chunk", "choices": [],
                                       "usage": {"prompt_tokens": 11, "completion_tokens": 5, "total_tokens": 16,
                                                 "prompt_tokens_details": {"cached_tokens": 2},
                                                 "completion_tokens_details": {"reasoning_tokens": 1}}})
                else:
                    if owner.require_context_text is not None and (not context_read_used or not recovered):
                        self.send_error(422, "expected context_read to recover the projected fixture tail")
                        return
                    if owner.fail_after_tools:
                        # The request that follows the tool calls is the
                        # injected fault. Nothing is returned to the model:
                        # the turn must end on a failure terminal.
                        if owner.fault_kind == FAULT_HTTP_ERROR:
                            body = json.dumps({"error": {"message": "fixture injected provider fault",
                                                         "type": "server_error"}}).encode()
                            self.send_response(503)
                            self.send_header("Content-Type", "application/json")
                            self.send_header("Content-Length", str(len(body)))
                            self.end_headers()
                            self.wfile.write(body)
                            return
                        # Declare more bytes than are written and close the
                        # connection, so the provider observes a truncated
                        # response instead of a completed one.
                        partial = b'data: {"id":"fixture-fault","object":"chat.completion.chunk","choices":[]}\n\n'
                        self.send_response(200)
                        self.send_header("Content-Type", "text/event-stream")
                        self.send_header("Cache-Control", "no-cache")
                        self.send_header("Content-Length", str(len(partial) + 64))
                        self.end_headers()
                        try:
                            self.wfile.write(partial)
                            self.wfile.flush()
                        except (BrokenPipeError, ConnectionResetError):
                            return
                        self.close_connection = True
                        return
                    if owner.stall_after_tools:
                        self.send_response(200)
                        self.send_header("Content-Type", "text/event-stream")
                        self.send_header("Cache-Control", "no-cache")
                        self.send_header("Connection", "close")
                        self.end_headers()
                        partial = {"id": f"fixture-{call}", "object": "chat.completion.chunk",
                                   "choices": [{"index": 0, "delta": {"role": "assistant", "content": "working"},
                                                "finish_reason": None}]}
                        try:
                            self.wfile.write(("data: " + json.dumps(partial) + "\n\n").encode())
                            self.wfile.flush()
                            owner.stalled.set()
                            owner.release_stall.wait(15)
                        except (BrokenPipeError, ConnectionResetError):
                            return
                        return
                    chunks = [
                        {"id": "fixture-2", "object": "chat.completion.chunk", "choices": [{"index": 0, "delta": {"role": "assistant", "content": "fixture turn complete"}, "finish_reason": None}]},
                        {"id": "fixture-2", "object": "chat.completion.chunk", "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]},
                        {"id": "fixture-2", "object": "chat.completion.chunk", "choices": [],
                         "usage": {"prompt_tokens": 17, "completion_tokens": 9, "total_tokens": 26,
                                   "prompt_tokens_details": {"cached_tokens": 3},
                                   "completion_tokens_details": {"reasoning_tokens": 2}}},
                    ]
                payload = "".join("data: " + json.dumps(chunk) + "\n\n" for chunk in chunks) + "data: [DONE]\n\n"
                body = payload.encode()
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Cache-Control", "no-cache")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    @property
    def base_url(self) -> str:
        host, port = self.server.server_address
        return f"http://{host}:{port}/v1"

    def start(self) -> "FixtureProvider":
        self.thread.start()
        return self

    def close(self) -> None:
        self.release_stall.set()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)
