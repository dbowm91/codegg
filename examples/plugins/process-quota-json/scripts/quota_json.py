#!/usr/bin/env python3
"""Structured quota reporter — reads PluginInvocation JSON, emits PluginResponse JSON."""

import json
import sys

PROTOCOL_VERSION = 1

PROVIDER_DATA = {
    "anthropic": {
        "limit": "10,000 req/day",
        "used": "3,421 req",
        "remaining": "6,579 req",
    },
    "openai": {
        "limit": "20,000 req/day",
        "used": "8,104 req",
        "remaining": "11,896 req",
    },
}


def read_invocation():
    try:
        return json.load(sys.stdin)
    except (json.JSONDecodeError, ValueError) as exc:
        return {"_parse_error": str(exc)}


def write_response(response):
    json.dump(response, sys.stdout)
    sys.stdout.write("\n")
    sys.stdout.flush()


def parse_provider(args):
    for i, arg in enumerate(args):
        if arg == "--provider" and i + 1 < len(args):
            return args[i + 1]
    return "anthropic"


def build_quota_rows(provider):
    data = PROVIDER_DATA.get(provider)
    if data is None:
        return [
            [provider, "unknown", "unknown", "unknown"],
        ]
    return [
        [provider, data["limit"], data["used"], data["remaining"]],
    ]


def build_response(provider):
    rows = build_quota_rows(provider)
    summary = f"Quota for {provider}: {rows[0][3]} remaining"

    return {
        "ok": True,
        "effects": [
            {
                "type": "emit_chat",
                "block": {
                    "format": "markdown",
                    "content": summary,
                },
            },
            {
                "type": "open_dialog",
                "dialog": {
                    "id": "quota",
                    "title": "Provider Quota",
                    "body": {
                        "kind": "table",
                        "columns": ["Provider", "Limit", "Used", "Remaining"],
                        "rows": rows,
                    },
                    "modal": True,
                },
            },
        ],
        "data": {
            "provider": provider,
            "limit": rows[0][1],
            "used": rows[0][2],
            "remaining": rows[0][3],
        },
        "diagnostics": [
            {
                "level": "info",
                "message": f"rendered quota for provider {provider}",
            }
        ],
    }


def build_error_response(message):
    return {
        "ok": False,
        "effects": [],
        "data": {},
        "diagnostics": [
            {
                "level": "error",
                "message": message,
            }
        ],
    }


def main():
    invocation = read_invocation()

    if "_parse_error" in invocation:
        write_response(
            build_error_response(f"malformed stdin: {invocation['_parse_error']}")
        )
        sys.exit(0)

    if invocation.get("protocol_version") != PROTOCOL_VERSION:
        write_response(
            build_error_response(
                f"unsupported protocol version: {invocation.get('protocol_version')}"
            )
        )
        sys.exit(0)

    args = invocation.get("args", [])

    if "--print-invocation" in args:
        clean_args = [a for a in args if a != "--print-invocation"]
        invocation["args"] = clean_args
        write_response(
            {
                "ok": True,
                "effects": [],
                "data": invocation,
                "diagnostics": [
                    {
                        "level": "info",
                        "message": "invocation dump (debug mode)",
                    }
                ],
            }
        )
        sys.exit(0)

    provider = parse_provider(args)
    write_response(build_response(provider))


if __name__ == "__main__":
    main()
