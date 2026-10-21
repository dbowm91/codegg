"""Fault-injection tests for the local provider fixture.

The fixture is the provider-free half of the end-to-end failure evidence: it
must be able to fail AFTER a metered tool-calling attempt, and it must be able
to fail BEFORE any provider response, so the harness can distinguish the three
usage provenance states without ever calling a paid provider.
"""
import http.client
import json
import unittest

from dev.harness_lab.provider_fixture import (
    FAULT_HTTP_ERROR,
    FAULT_KINDS,
    FAULT_TRANSPORT,
    FixtureProvider,
)


def _payload(tool_name: str = "bash") -> dict:
    return {"messages": [{"role": "user", "content": "go"}],
            "tools": [{"type": "function", "function": {"name": tool_name}}]}


def _chat_request(base_url: str, payload: dict) -> http.client.HTTPResponse:
    authority = base_url.split("://", 1)[1].split("/", 1)[0]
    host, _, port = authority.partition(":")
    connection = http.client.HTTPConnection(host, int(port), timeout=10)
    body = json.dumps(payload).encode("utf-8")
    connection.request("POST", "/v1/chat/completions", body=body,
                       headers={"Content-Type": "application/json"})
    response = connection.getresponse()
    response.connection = connection
    return response


class FixtureFaultTests(unittest.TestCase):
    def test_rejects_an_unknown_fault_kind(self) -> None:
        with self.assertRaises(ValueError):
            FixtureProvider("bash", fault_kind="nonsense")

    def test_fault_kinds_are_the_documented_allowlist(self) -> None:
        self.assertEqual(FAULT_KINDS, (FAULT_TRANSPORT, FAULT_HTTP_ERROR))

    def test_fault_after_tools_reports_usage_then_fails(self) -> None:
        provider = FixtureProvider(
            "bash", arguments={"command": "printf ok"},
            fail_after_tools=True, fault_kind=FAULT_TRANSPORT).start()
        try:
            first = _chat_request(provider.base_url, _payload())
            body = first.read().decode("utf-8")
            first.close()
            # The tool-calling attempt is metered: the response carries usage.
            self.assertIn("usage", body)
            self.assertIn("tool_calls", body)

            # The request that follows the tool call is the injected fault.
            second = _chat_request(provider.base_url, _payload())
            try:
                second.read()
            except (http.client.HTTPException, ConnectionError, OSError):
                pass
            finally:
                second.close()
            self.assertGreaterEqual(provider.calls, 2)
        finally:
            provider.close()

    def test_http_error_fault_answers_with_a_server_error(self) -> None:
        provider = FixtureProvider(
            "bash", arguments={"command": "printf ok"},
            fail_after_tools=True, fault_kind=FAULT_HTTP_ERROR).start()
        try:
            first = _chat_request(provider.base_url, _payload())
            first.read()
            first.close()

            second = _chat_request(provider.base_url, _payload())
            try:
                payload = json.loads(second.read().decode("utf-8"))
                self.assertEqual(second.status, 503)
                self.assertIn("error", payload)
            finally:
                second.close()
        finally:
            provider.close()

    def test_no_fault_means_the_fixture_completes_normally(self) -> None:
        provider = FixtureProvider(
            "bash", arguments={"command": "printf ok"}).start()
        try:
            first = _chat_request(provider.base_url, _payload())
            tool_body = first.read().decode("utf-8")
            first.close()
            self.assertEqual(first.status, 200)
            self.assertIn("tool_calls", tool_body)
            # The tool-calling attempt is not metered in the normal lane; the
            # completion that follows it carries the usage.
            self.assertNotIn("usage", tool_body)

            second = _chat_request(provider.base_url, _payload())
            body = second.read().decode("utf-8")
            second.close()
            self.assertEqual(second.status, 200)
            self.assertIn("[DONE]", body)
            self.assertIn("usage", body)
        finally:
            provider.close()


if __name__ == "__main__":
    unittest.main()
