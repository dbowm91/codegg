"""CLI for testing codegg_plugin helpers.

Usage::

    python -m codegg_plugin --read-invocation   # read stdin, pretty-print
    python -m codegg_plugin --demo               # print a sample PluginResponse
"""

import argparse
import json
import sys

from codegg_plugin.io import InvalidInvocationError, read_invocation, write_response
from codegg_plugin.responses import ok_response
from codegg_plugin.effects import emit_chat, open_dialog, table_node


def cmd_read_invocation():
    try:
        inv = read_invocation()
    except InvalidInvocationError as exc:
        print(f"error: {exc}", file=sys.stderr)
        sys.exit(1)
    json.dump(inv, sys.stdout, indent=2)
    sys.stdout.write("\n")


def cmd_demo():
    resp = ok_response(
        effects=[
            emit_chat("Hello from codegg_plugin!"),
            open_dialog(
                id="demo",
                title="Demo Dialog",
                body=table_node(
                    columns=["Key", "Value"],
                    rows=[["greeting", "hello"], ["target", "world"]],
                ),
            ),
        ],
        data={"demo": True},
        diagnostics=[{"level": "info", "message": "demo response generated"}],
    )
    write_response(resp)


def main():
    parser = argparse.ArgumentParser(
        prog="codegg_plugin",
        description="codegg plugin SDK test helper",
    )
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument(
        "--read-invocation",
        action="store_true",
        help="Read a PluginInvocation from stdin and pretty-print it",
    )
    group.add_argument(
        "--demo",
        action="store_true",
        help="Print a sample PluginResponse",
    )
    parser.parse_args()

    if getattr(sys.argv, "read_invocation", False) or "--read-invocation" in sys.argv:
        cmd_read_invocation()
    else:
        cmd_demo()


if __name__ == "__main__":
    main()
