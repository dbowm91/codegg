"""Tests for the codegg_plugin SDK helpers."""

import io
import json
import sys
import unittest
from unittest.mock import patch

from codegg_plugin.protocol import PLUGIN_PROTOCOL_VERSION, is_valid_invocation
from codegg_plugin.io import InvalidInvocationError, read_invocation, write_response, write_diagnostic
from codegg_plugin.responses import ok_response, error_response, diagnostic
from codegg_plugin.effects import (
    emit_chat,
    open_dialog,
    open_panel,
    show_toast,
    add_status_item,
    text_node,
    markdown_node,
    table_node,
    key_value_node,
)


def _make_invocation(**overrides):
    inv = {
        "protocol_version": PLUGIN_PROTOCOL_VERSION,
        "invocation_id": "test-001",
        "plugin_id": "test-plugin",
        "capability": {"type": "command", "name": "test-cmd"},
        "args": [],
        "input": {},
        "context": {
            "session_id": None,
            "turn_id": None,
            "project_dir": None,
            "model": None,
            "agent": None,
            "frontend_capabilities": [],
            "metadata": {},
        },
    }
    inv.update(overrides)
    return inv


def _patch_stdin(text):
    return patch("sys.stdin", new=io.StringIO(text), create=True)


def _capture_stdout():
    buf = io.StringIO()
    return patch("sys.stdout", buf), buf


class TestReadInvocation(unittest.TestCase):
    def test_read_invocation_valid(self):
        inv = _make_invocation()
        stdin_text = json.dumps(inv)
        with _patch_stdin(stdin_text):
            result = read_invocation()
        self.assertEqual(result["protocol_version"], PLUGIN_PROTOCOL_VERSION)
        self.assertEqual(result["invocation_id"], "test-001")

    def test_read_invocation_invalid_json_raises(self):
        with _patch_stdin("not json {{{"):
            with self.assertRaises(InvalidInvocationError) as ctx:
                read_invocation()
            self.assertIn("malformed JSON", str(ctx.exception))

    def test_read_invocation_wrong_protocol_version(self):
        inv = _make_invocation(protocol_version=99)
        with _patch_stdin(json.dumps(inv)):
            with self.assertRaises(InvalidInvocationError) as ctx:
                read_invocation()
            self.assertIn("protocol_version", str(ctx.exception))


class TestResponses(unittest.TestCase):
    def test_ok_response_shape(self):
        resp = ok_response()
        self.assertTrue(resp["ok"])
        self.assertEqual(resp["effects"], [])
        self.assertEqual(resp["data"], {})
        self.assertEqual(resp["diagnostics"], [])

    def test_ok_response_with_payloads(self):
        resp = ok_response(
            effects=[emit_chat("hi")],
            data={"k": "v"},
            diagnostics=[diagnostic("info", "done")],
        )
        self.assertTrue(resp["ok"])
        self.assertEqual(len(resp["effects"]), 1)
        self.assertEqual(resp["data"]["k"], "v")
        self.assertEqual(len(resp["diagnostics"]), 1)

    def test_error_response_shape(self):
        resp = error_response("something broke")
        self.assertFalse(resp["ok"])
        self.assertEqual(resp["effects"], [])
        self.assertEqual(len(resp["diagnostics"]), 1)
        self.assertEqual(resp["diagnostics"][0]["level"], "error")
        self.assertEqual(resp["diagnostics"][0]["message"], "something broke")

    def test_diagnostic_levels(self):
        for level in ("info", "warning", "error"):
            d = diagnostic(level, f"msg {level}")
            self.assertEqual(d["level"], level)
            self.assertEqual(d["message"], f"msg {level}")


class TestEffects(unittest.TestCase):
    def test_emit_chat_format(self):
        plain = emit_chat("hello", fmt="plain")
        self.assertEqual(plain["type"], "emit_chat")
        self.assertEqual(plain["block"]["format"], "plain")
        self.assertEqual(plain["block"]["content"], "hello")

        md = emit_chat("# heading")
        self.assertEqual(md["block"]["format"], "markdown")
        self.assertEqual(md["block"]["content"], "# heading")

    def test_show_toast_shape(self):
        t = show_toast("warning", "careful")
        self.assertEqual(t["type"], "show_toast")
        self.assertEqual(t["toast"]["level"], "warning")
        self.assertEqual(t["toast"]["message"], "careful")

    def test_open_dialog_shape(self):
        body = text_node("details")
        d = open_dialog("dlg-1", "My Dialog", body, modal=False)
        self.assertEqual(d["type"], "open_dialog")
        self.assertEqual(d["dialog"]["id"], "dlg-1")
        self.assertEqual(d["dialog"]["title"], "My Dialog")
        self.assertEqual(d["dialog"]["body"]["kind"], "text")
        self.assertFalse(d["dialog"]["modal"])

    def test_open_panel_shape(self):
        p = open_panel("p1", "Panel", "right", text_node("content"))
        self.assertEqual(p["type"], "open_panel")
        self.assertEqual(p["panel"]["id"], "p1")
        self.assertEqual(p["panel"]["placement"], "right")

    def test_add_status_item_shape(self):
        s = add_status_item("s1", "right", text_node("ok"), label="Status")
        self.assertEqual(s["type"], "add_status_item")
        self.assertEqual(s["item"]["id"], "s1")
        self.assertEqual(s["item"]["label"], "Status")

    def test_add_status_item_no_label(self):
        s = add_status_item("s1", "left", text_node("x"))
        self.assertNotIn("label", s["item"])

    def test_text_node_shape(self):
        n = text_node("hello")
        self.assertEqual(n["kind"], "text")
        self.assertEqual(n["text"], "hello")

    def test_markdown_node_shape(self):
        n = markdown_node("# Title")
        self.assertEqual(n["kind"], "markdown")
        self.assertEqual(n["markdown"], "# Title")

    def test_table_node_shape(self):
        n = table_node(["A", "B"], [["1", "2"], ["3", "4"]])
        self.assertEqual(n["kind"], "table")
        self.assertEqual(n["columns"], ["A", "B"])
        self.assertEqual(len(n["rows"]), 2)

    def test_key_value_node_shape(self):
        n = key_value_node([("k1", "v1"), ("k2", "v2")])
        self.assertEqual(n["kind"], "key_value")
        self.assertEqual(len(n["entries"]), 2)
        self.assertEqual(n["entries"][0]["key"], "k1")
        self.assertEqual(n["entries"][1]["value"], "v2")


class TestWriteResponse(unittest.TestCase):
    def test_write_response_produces_json(self):
        resp = ok_response(data={"x": 1})
        mock_stdout = io.StringIO()
        with patch("sys.stdout", mock_stdout):
            write_response(resp)
        output = mock_stdout.getvalue()
        parsed = json.loads(output)
        self.assertTrue(parsed["ok"])
        self.assertEqual(parsed["data"]["x"], 1)


class TestWriteDiagnostic(unittest.TestCase):
    def test_write_diagnostic_appends(self):
        diags = []
        write_diagnostic("info", "first", diags)
        write_diagnostic("warning", "second", diags)
        self.assertEqual(len(diags), 2)
        self.assertEqual(diags[0]["level"], "info")
        self.assertEqual(diags[1]["level"], "warning")


class TestIsValidInvocation(unittest.TestCase):
    def test_valid(self):
        self.assertTrue(is_valid_invocation({"protocol_version": 1}))

    def test_wrong_version(self):
        self.assertFalse(is_valid_invocation({"protocol_version": 99}))

    def test_missing_version(self):
        self.assertFalse(is_valid_invocation({}))

    def test_not_dict(self):
        self.assertFalse(is_valid_invocation("hello"))


class TestRoundTrip(unittest.TestCase):
    def test_full_round_trip(self):
        inv = _make_invocation(args=["--provider", "anthropic"])
        stdin_text = json.dumps(inv)

        with _patch_stdin(stdin_text):
            parsed = read_invocation()

        provider = "anthropic"
        args = parsed.get("args", [])
        for i, arg in enumerate(args):
            if arg == "--provider" and i + 1 < len(args):
                provider = args[i + 1]

        resp = ok_response(
            effects=[
                emit_chat(f"provider: {provider}"),
                open_dialog(
                    id="quota",
                    title="Quota",
                    body=table_node(
                        columns=["Provider"],
                        rows=[[provider]],
                    ),
                ),
            ],
            data={"provider": provider},
        )

        out = io.StringIO()
        with patch("sys.stdout", out):
            write_response(resp)

        output = out.getvalue()
        parsed = json.loads(output)
        self.assertTrue(parsed["ok"])
        self.assertEqual(len(parsed["effects"]), 2)
        self.assertEqual(parsed["effects"][0]["type"], "emit_chat")
        self.assertEqual(parsed["effects"][1]["type"], "open_dialog")
        self.assertEqual(parsed["data"]["provider"], "anthropic")


if __name__ == "__main__":
    unittest.main()
