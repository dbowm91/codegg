"""Bounded loopback HTTP admission for the external OpenCode proposer.

The gateway counts provider HTTP requests, rather than CLI invocations. It
never stores request or response bodies or authorization headers. A fixture
handler keeps tests entirely offline; forwarding is opt-in and requires an
explicit HTTPS upstream.
"""
from __future__ import annotations

from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import hashlib
import json
import threading
import time
from typing import Callable
from urllib.error import HTTPError
from urllib.parse import urlsplit, urlunsplit
from urllib.request import HTTPRedirectHandler, ProxyHandler, Request, build_opener

MAX_REQUEST_BYTES = 4 * 1024 * 1024
MAX_RESPONSE_BYTES = 16 * 1024 * 1024
SUPPORTED_PATHS = {"/v1/responses", "/v1/chat/completions"}


@dataclass(frozen=True)
class GatewayResponse:
    status: int
    content_type: str
    body: bytes


FixtureHandler = Callable[[int, str, bytes], GatewayResponse]


def opencode_inline_config(model: str, base_url: str, *, fixture_api_key: bool = False) -> str:
    """Build highest-precedence OpenCode policy that pins all provider traffic.

    The only allowed model tools are read/search/edit operations in the target
    project. Shell, web, MCP, delegation and every unknown tool are denied so
    the proposer cannot create an unmetered network route around the gateway.
    """
    if not isinstance(model, str) or model.count("/") != 1:
        raise ValueError("proposer model must be pinned as provider/model")
    provider, model_id = model.split("/", 1)
    if not provider or not model_id:
        raise ValueError("proposer model must be pinned as provider/model")
    provider_config: dict[str, object] = {
        "npm": "@ai-sdk/openai",
        "name": "CodeGG request-gated proposer",
        "options": {"baseURL": base_url},
        "models": {model_id: {"name": model_id}},
    }
    if fixture_api_key:
        provider_config["options"] = {"baseURL": base_url, "apiKey": "fixture-only"}
    config = {
        "$schema": "https://opencode.ai/config.json",
        "provider": {provider: provider_config},
        "enabled_providers": [provider],
        "permission": {
            "*": "deny", "read": "allow", "edit": "allow",
            "glob": "allow", "grep": "allow", "list": "allow",
            "external_directory": "deny",
        },
    }
    return json.dumps(config, sort_keys=True, separators=(",", ":"))


class _NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class ProposerRequestGateway:
    """One invocation's pretransport request gate, bound to loopback only."""

    def __init__(self, max_requests: int, *, fixture_handler: FixtureHandler | None = None,
                 upstream_base_url: str | None = None, timeout: float = 30.0,
                 admit_request: Callable[[], None] | None = None,
                 observe_request: Callable[[], None] | None = None):
        if isinstance(max_requests, bool) or not isinstance(max_requests, int) or max_requests < 1:
            raise ValueError("proposer request ceiling must be a positive integer")
        if (fixture_handler is None) == (upstream_base_url is None):
            raise ValueError("select exactly one of fixture mode or an explicit upstream")
        if fixture_handler is not None and upstream_base_url is not None:
            raise ValueError("fixture mode cannot also configure an upstream")
        if upstream_base_url is not None:
            parsed = urlsplit(upstream_base_url)
            if parsed.scheme != "https" or not parsed.hostname or parsed.username or parsed.password \
                    or parsed.query or parsed.fragment:
                raise ValueError("proposer upstream must be an HTTPS base URL without credentials or query")
            if parsed.hostname.lower() != "opencode.ai":
                raise ValueError("external proposer upstream host is not the approved OpenCode Go host")
        if timeout <= 0:
            raise ValueError("proposer gateway timeout must be positive")
        self.max_requests = max_requests
        self.fixture_handler = fixture_handler
        self.upstream_base_url = upstream_base_url.rstrip("/") if upstream_base_url else None
        self.timeout = timeout
        self.admit_request = admit_request
        self.observe_request = observe_request
        self._lock = threading.Lock()
        self._rows: list[dict[str, object]] = []
        self._admitted = 0
        self._refused = 0
        self._forwarded = 0
        self._refusal_event = threading.Event()
        self._started_at = time.monotonic()
        self._server: ThreadingHTTPServer | None = None
        self._thread: threading.Thread | None = None

    def __enter__(self) -> "ProposerRequestGateway":
        owner = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def do_POST(self):
                owner._handle(self)

            def do_GET(self):
                owner._handle(self)

            def do_PUT(self):
                owner._handle(self)

            def do_DELETE(self):
                owner._handle(self)

            def do_PATCH(self):
                owner._handle(self)

            def log_message(self, _format: str, *args) -> None:
                return

        self._server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self._server.daemon_threads = True
        self._thread = threading.Thread(target=self._server.serve_forever,
                                        name="harness-proposer-gateway", daemon=True)
        self._thread.start()
        return self

    @property
    def base_url(self) -> str:
        if self._server is None:
            raise RuntimeError("proposer request gateway is not running")
        return f"http://127.0.0.1:{self._server.server_port}/v1"

    @property
    def refusal_event(self) -> threading.Event:
        """Set as soon as any request is refused, so the owning CLI can stop."""
        return self._refusal_event

    def _handle(self, handler: BaseHTTPRequestHandler) -> None:
        started = time.monotonic()
        path = handler.path.split("?", 1)[0]
        row: dict[str, object] = {
            "sequence": 0, "method": handler.command,
            "path": path if path in SUPPORTED_PATHS else "unsupported",
            "body_sha256": None, "admitted": False, "forwarded": False,
            "status": None, "duration_ms": None,
        }
        with self._lock:
            row["sequence"] = len(self._rows) + 1
            if len(self._rows) >= self.max_requests:
                self._refused += 1
                self._refusal_event.set()
                row["refusal"] = "request_ceiling"
                self._rows.append(row)
                self._reply(handler, 429, "application/json",
                            b'{"error":{"message":"proposer request ceiling exhausted","type":"rate_limit_error"}}')
                row["status"] = 429
                row["duration_ms"] = round((time.monotonic() - started) * 1000, 3)
                return
            try:
                if self.admit_request is not None:
                    self.admit_request()
            except Exception as exc:
                self._refused += 1
                self._refusal_event.set()
                row["refusal"] = "campaign_budget"
                row["budget_refusal"] = type(exc).__name__
                self._rows.append(row)
                self._reply(handler, 429, "application/json",
                            b'{"error":{"message":"campaign request budget exhausted","type":"rate_limit_error"}}')
                row["status"] = 429
                row["duration_ms"] = round((time.monotonic() - started) * 1000, 3)
                return
            self._admitted += 1
            row["admitted"] = True
            self._rows.append(row)

        try:
            handler.connection.settimeout(self.timeout)
            if handler.command != "POST" or path not in SUPPORTED_PATHS:
                response = GatewayResponse(404, "application/json", b'{"error":"unsupported proposer request"}')
            else:
                length = handler.headers.get("Content-Length")
                if length is None or not length.isdecimal() or int(length) > MAX_REQUEST_BYTES:
                    response = GatewayResponse(413, "application/json", b'{"error":"request body refused"}')
                else:
                    body = handler.rfile.read(int(length))
                    if len(body) != int(length):
                        response = GatewayResponse(400, "application/json", b'{"error":"incomplete request body"}')
                    else:
                        row["body_sha256"] = hashlib.sha256(body).hexdigest()
                        with self._lock:
                            self._forwarded += 1
                            row["forwarded"] = True
                        if self.fixture_handler is not None:
                            response = self.fixture_handler(int(row["sequence"]), path, body)
                        else:
                            response = self._forward(handler, path, body)
                        if not isinstance(response, GatewayResponse) or not isinstance(response.body, bytes) \
                                or not isinstance(response.content_type, str) \
                                or "\r" in response.content_type or "\n" in response.content_type:
                            raise ValueError("invalid proposer gateway response")
                        if len(response.body) > MAX_RESPONSE_BYTES:
                            response = GatewayResponse(502, "application/json",
                                                       b'{"error":"proposer response too large"}')
            self._reply(handler, response.status, response.content_type, response.body)
            row["status"] = response.status
        except Exception as exc:
            row["transport_error"] = type(exc).__name__
            self._reply(handler, 502, "application/json", b'{"error":"proposer upstream unavailable"}')
            row["status"] = 502
        finally:
            if self.observe_request is not None:
                try:
                    self.observe_request()
                except Exception:
                    row["observation_error"] = True
            row["duration_ms"] = round((time.monotonic() - started) * 1000, 3)

    def _forward(self, handler: BaseHTTPRequestHandler, path: str, body: bytes) -> GatewayResponse:
        assert self.upstream_base_url is not None
        suffix = path.removeprefix("/v1")
        upstream = urlsplit(self.upstream_base_url)
        url = urlunsplit((upstream.scheme, upstream.netloc, upstream.path.rstrip("/") + suffix, "", ""))
        headers = {"Content-Type": handler.headers.get("Content-Type", "application/json"),
                   "Accept": handler.headers.get("Accept", "application/json")}
        for name in ("Authorization", "OpenAI-Beta"):
            value = handler.headers.get(name)
            if value:
                headers[name] = value
        request = Request(url, data=body, headers=headers, method="POST")
        # Do not let ambient HTTP(S)_PROXY settings redirect the gated request
        # through an unreviewed endpoint. Forward directly to the validated
        # HTTPS host or refuse at the transport layer.
        opener = build_opener(_NoRedirect, ProxyHandler({}))
        try:
            upstream_response = opener.open(request, timeout=self.timeout)
        except HTTPError as exc:
            upstream_response = exc
        try:
            response_body = upstream_response.read(MAX_RESPONSE_BYTES + 1)
            if len(response_body) > MAX_RESPONSE_BYTES:
                return GatewayResponse(502, "application/json", b'{"error":"upstream response too large"}')
            content_type = upstream_response.headers.get("Content-Type", "application/json")
            return GatewayResponse(upstream_response.code, content_type, response_body)
        finally:
            upstream_response.close()

    @staticmethod
    def _reply(handler: BaseHTTPRequestHandler, status: int, content_type: str, body: bytes) -> None:
        try:
            handler.send_response(status)
            handler.send_header("Content-Type", content_type)
            handler.send_header("Content-Length", str(len(body)))
            handler.send_header("Connection", "close")
            handler.end_headers()
            handler.wfile.write(body)
        except OSError:
            pass

    def receipt(self) -> dict[str, object]:
        with self._lock:
            return {
                "schema": "proposer_http_admission_v1",
                "request_ceiling": self.max_requests,
                "observed_requests": len(self._rows),
                "admitted_requests": self._admitted,
                "refused_requests": self._refused,
                "forwarded_requests": self._forwarded,
                "upstream_host": "opencode.ai" if self.upstream_base_url else "fixture",
                "elapsed_ms": round((time.monotonic() - self._started_at) * 1000, 3),
                "requests": [dict(row) for row in self._rows],
            }

    def __exit__(self, _exc_type, _exc, _tb) -> None:
        server, thread = self._server, self._thread
        self._server = None
        self._thread = None
        if server is not None:
            server.shutdown()
            server.server_close()
        if thread is not None:
            thread.join(timeout=2)
