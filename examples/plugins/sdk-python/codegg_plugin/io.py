"""stdin/stdout helpers for codegg process plugins."""

import json
import sys

from codegg_plugin.protocol import is_valid_invocation


class InvalidInvocationError(Exception):
    """Raised when stdin does not contain a valid PluginInvocation."""


def read_invocation():
    """Read a ``PluginInvocation`` dict from stdin.

    Raises ``InvalidInvocationError`` on malformed JSON or missing
    ``protocol_version``.
    """
    try:
        obj = json.load(sys.stdin)
    except (json.JSONDecodeError, ValueError) as exc:
        raise InvalidInvocationError(f"malformed JSON: {exc}") from exc

    if not is_valid_invocation(obj):
        version = obj.get("protocol_version") if isinstance(obj, dict) else None
        raise InvalidInvocationError(
            f"invalid or missing protocol_version (got {version!r})"
        )

    return obj


def write_response(response):
    """Write a ``PluginResponse`` dict to stdout as JSON."""
    json.dump(response, sys.stdout)
    sys.stdout.write("\n")
    sys.stdout.flush()


def write_diagnostic(level, message, diagnostics):
    """Append a diagnostic entry to a diagnostics list."""
    diagnostics.append({"level": level, "message": message})
