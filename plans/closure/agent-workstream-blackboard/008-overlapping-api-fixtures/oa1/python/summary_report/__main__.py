from __future__ import annotations

import argparse
import sys
from pathlib import Path

from .consumer import SummaryError, load_summary, render_report


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("input", help="summary JSON file or - for stdin")
    args = parser.parse_args()
    try:
        text = sys.stdin.read() if args.input == "-" else Path(args.input).read_text()
        print(render_report(load_summary(text)), end="")
    except (OSError, SummaryError) as exc:
        print(str(exc), file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
