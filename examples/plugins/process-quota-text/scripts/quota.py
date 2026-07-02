#!/usr/bin/env python3
"""Zero-SDK quota reporter — plain text output, no dependencies."""

import signal
import sys

_PROVIDER_DATA = {
    "anthropic": {
        "requests_per_min": 60,
        "requests_per_day": 10000,
        "tokens_per_day": 5_000_000,
        "status": "healthy",
    },
    "openai": {
        "requests_per_min": 120,
        "requests_per_day": 20000,
        "tokens_per_day": 10_000_000,
        "status": "healthy",
    },
}

terminated = False


def handle_sigterm(signum, frame):
    global terminated
    terminated = True


def main():
    signal.signal(signal.SIGTERM, handle_sigterm)

    provider = "anthropic"
    args = sys.argv[1:]
    i = 0
    while i < len(args):
        if args[i] == "--provider" and i + 1 < len(args):
            provider = args[i + 1]
            i += 2
        else:
            i += 1

    if terminated:
        return

    data = _PROVIDER_DATA.get(provider)
    if data is None:
        print(f"unknown provider: {provider}", file=sys.stderr)
        sys.exit(1)

    print("codegg provider quota")
    print("---------------------")
    print(f"provider: {provider}")
    print(f"requests/min: {data['requests_per_min']}")
    print(f"requests/day: {data['requests_per_day']}")
    print(f"tokens/day: {data['tokens_per_day']:,}")
    print(f"status: {data['status']}")


if __name__ == "__main__":
    main()
