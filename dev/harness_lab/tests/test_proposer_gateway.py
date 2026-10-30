"""HTTP-level, provider-free admission tests for OpenCode proposals."""
from __future__ import annotations

from http.client import HTTPConnection
import json
import os
from pathlib import Path
import signal
import shutil
import subprocess
import tempfile
import threading
import time
import unittest
from unittest import mock

from dev.harness_lab.proposer_gateway import (
    GatewayResponse,
    ProposerRequestGateway,
    opencode_inline_config,
)
from dev.harness_lab import proposer_gateway
from dev.harness_lab import improve
from dev.harness_lab.improve import SUPPORTED_OPENCODE_VERSION


def _responses_stream(text: str = "ok") -> bytes:
    response_id, message_id = "resp_fixture", "msg_fixture"
    part = {"type": "output_text", "text": text, "annotations": []}
    item = {"type": "message", "id": message_id, "role": "assistant",
            "status": "completed", "content": [part]}
    response = {"id": response_id, "object": "response", "created_at": 1,
                "status": "completed", "model": "fixture-model", "output": [item],
                "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}}
    events = [
        ("response.created", {"type": "response.created", "response": {
            **response, "status": "in_progress", "output": []}}),
        ("response.output_item.added", {"type": "response.output_item.added", "output_index": 0,
            "item": {"type": "message", "id": message_id, "role": "assistant",
                     "status": "in_progress", "content": []}}),
        ("response.content_part.added", {"type": "response.content_part.added",
            "item_id": message_id, "output_index": 0, "content_index": 0,
            "part": {"type": "output_text", "text": "", "annotations": []}}),
        ("response.output_text.delta", {"type": "response.output_text.delta", "item_id": message_id,
            "output_index": 0, "content_index": 0, "delta": text}),
        ("response.output_text.done", {"type": "response.output_text.done", "item_id": message_id,
            "output_index": 0, "content_index": 0, "text": text}),
        ("response.content_part.done", {"type": "response.content_part.done", "item_id": message_id,
            "output_index": 0, "content_index": 0, "part": part}),
        ("response.output_item.done", {"type": "response.output_item.done", "output_index": 0,
            "item": item}),
        ("response.completed", {"type": "response.completed", "response": response}),
    ]
    return "".join(f"event: {name}\ndata: {json.dumps(value)}\n\n"
                   for name, value in events).encode()


class ProposerGatewayTests(unittest.TestCase):
    def test_inline_config_pins_only_the_requested_provider_and_denies_other_tools(self) -> None:
        config = json.loads(opencode_inline_config("opencode-go/muse-spark-1.3-contributor",
                                                   "http://127.0.0.1:1234/v1"))
        self.assertEqual(config["enabled_providers"], ["opencode-go"])
        self.assertEqual(config["provider"]["opencode-go"]["options"]["baseURL"],
                         "http://127.0.0.1:1234/v1")
        self.assertEqual(config["permission"]["*"], "deny")
        self.assertNotIn("shell", config["permission"])

    def test_request_n_plus_one_is_refused_before_fixture_forwarding(self) -> None:
        forwarded: list[tuple[int, str, bytes]] = []

        def fixture(sequence: int, path: str, body: bytes) -> GatewayResponse:
            forwarded.append((sequence, path, body))
            return GatewayResponse(200, "application/json", b'{"ok":true}')

        with ProposerRequestGateway(1, fixture_handler=fixture) as gateway:
            conn = HTTPConnection("127.0.0.1", int(gateway.base_url.split(":")[-1].split("/")[0]))
            try:
                for index, body in enumerate((b'{"prompt":"secret-one"}', b'{"prompt":"secret-two"}')):
                    conn.request("POST", "/v1/responses", body,
                                 {"Content-Type": "application/json", "Authorization": "Bearer secret"})
                    response = conn.getresponse()
                    response.read()
                    self.assertEqual(response.status, 200 if index == 0 else 429)
            finally:
                conn.close()
            receipt = gateway.receipt()

        self.assertEqual(len(forwarded), 1)
        self.assertEqual(receipt["request_ceiling"], 1)
        self.assertEqual(receipt["observed_requests"], 2)
        self.assertEqual(receipt["admitted_requests"], 1)
        self.assertEqual(receipt["refused_requests"], 1)
        self.assertEqual(receipt["forwarded_requests"], 1)
        rendered = json.dumps(receipt)
        self.assertNotIn("secret-one", rendered)
        self.assertNotIn("secret-two", rendered)
        self.assertNotIn("Bearer secret", rendered)

    def test_request_ceiling_is_atomic_under_concurrent_requests(self) -> None:
        forwarded: list[int] = []
        forward_lock = threading.Lock()

        def fixture(sequence: int, _path: str, _body: bytes) -> GatewayResponse:
            with forward_lock:
                forwarded.append(sequence)
            return GatewayResponse(200, "application/json", b"{}")

        with ProposerRequestGateway(1, fixture_handler=fixture) as gateway:
            port = int(gateway.base_url.split(":")[-1].split("/")[0])
            statuses: list[int] = []

            def send() -> None:
                conn = HTTPConnection("127.0.0.1", port)
                conn.request("POST", "/v1/chat/completions", b"{}",
                             {"Content-Type": "application/json"})
                response = conn.getresponse()
                response.read()
                statuses.append(response.status)
                conn.close()

            threads = [threading.Thread(target=send) for _ in range(2)]
            for thread in threads:
                thread.start()
            for thread in threads:
                thread.join(timeout=5)
            self.assertTrue(all(not thread.is_alive() for thread in threads))
            receipt = gateway.receipt()

        self.assertCountEqual(statuses, [200, 429])
        self.assertEqual(len(forwarded), 1)
        self.assertEqual(receipt["admitted_requests"], 1)
        self.assertEqual(receipt["refused_requests"], 1)

    def test_unsupported_path_and_oversized_fixture_response_never_forward(self) -> None:
        calls: list[str] = []

        def fixture(_sequence: int, path: str, _body: bytes) -> GatewayResponse:
            calls.append(path)
            return GatewayResponse(200, "application/json", b"12345")

        with mock.patch.object(proposer_gateway, "MAX_RESPONSE_BYTES", 4):
            with ProposerRequestGateway(2, fixture_handler=fixture) as gateway:
                port = int(gateway.base_url.split(":")[-1].split("/")[0])
                statuses = []
                for path in ("/v1/not-supported", "/v1/responses"):
                    conn = HTTPConnection("127.0.0.1", port)
                    conn.request("POST", path, b"{}", {"Content-Type": "application/json"})
                    response = conn.getresponse()
                    response.read()
                    statuses.append(response.status)
                    conn.close()
                receipt = gateway.receipt()
        self.assertEqual(statuses, [404, 502])
        self.assertEqual(calls, ["/v1/responses"])
        self.assertEqual(receipt["admitted_requests"], 2)
        self.assertEqual(receipt["forwarded_requests"], 1)

    def test_oversized_request_body_is_refused_before_fixture_forwarding(self) -> None:
        calls: list[int] = []

        def fixture(sequence: int, _path: str, _body: bytes) -> GatewayResponse:
            calls.append(sequence)
            return GatewayResponse(200, "application/json", b"{}")

        with mock.patch.object(proposer_gateway, "MAX_REQUEST_BYTES", 4):
            with ProposerRequestGateway(1, fixture_handler=fixture) as gateway:
                port = int(gateway.base_url.split(":")[-1].split("/")[0])
                conn = HTTPConnection("127.0.0.1", port)
                conn.request("POST", "/v1/responses", b"12345",
                             {"Content-Type": "application/json"})
                response = conn.getresponse()
                response.read()
                conn.close()
                receipt = gateway.receipt()
        self.assertEqual(response.status, 413)
        self.assertEqual(calls, [])
        self.assertEqual(receipt["admitted_requests"], 1)
        self.assertEqual(receipt["forwarded_requests"], 0)

    def test_live_upstream_is_explicit_https_and_host_pinned(self) -> None:
        for upstream in ("http://opencode.ai/v1", "https://other.example/v1",
                         "https://user:secret@opencode.ai/v1", "https://opencode.ai/v1?key=x"):
            with self.subTest(upstream=upstream):
                with self.assertRaises(ValueError):
                    ProposerRequestGateway(1, upstream_base_url=upstream)

    @unittest.skipUnless(shutil.which("opencode"), "OpenCode CLI is not installed")
    def test_actual_opencode_cli_hits_only_the_loopback_gate_and_n_plus_one_stops(self) -> None:
        version = subprocess.run([shutil.which("opencode"), "--version"], capture_output=True,
                                 text=True, timeout=20)
        if version.returncode or version.stdout.strip() != SUPPORTED_OPENCODE_VERSION:
            self.skipTest("installed OpenCode CLI does not match the qualified version")

        with tempfile.TemporaryDirectory(prefix="harness-opencode-gateway-") as temporary:
            root = Path(temporary)

            def fixture(_sequence: int, _path: str, _body: bytes) -> GatewayResponse:
                return GatewayResponse(200, "text/event-stream", _responses_stream())

            with self.assertRaises(improve.ProposerAdmissionError) as caught:
                improve.run_opencode(root, "say ok", timeout=45, executable=shutil.which("opencode"),
                                     model="fixture/fixture-model", max_provider_requests=1,
                                     fixture_handler=fixture)
            receipt = caught.exception.request_admission

        self.assertEqual(receipt["request_ceiling"], 1)
        self.assertGreaterEqual(receipt["observed_requests"], 2)
        self.assertEqual(receipt["admitted_requests"], 1)
        self.assertEqual(receipt["refused_requests"], receipt["observed_requests"] - 1)
        self.assertEqual(receipt["forwarded_requests"], 1)
        rendered = json.dumps(receipt)
        self.assertNotIn("say ok", rendered)
        self.assertNotIn("fixture-only", rendered)


if __name__ == "__main__":
    unittest.main()
