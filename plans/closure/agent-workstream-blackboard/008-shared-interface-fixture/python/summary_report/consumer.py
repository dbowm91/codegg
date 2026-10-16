from __future__ import annotations

import json
from typing import Any


class SummaryError(ValueError):
    """Invalid or unsupported project-summary document."""


def load_summary(text: str) -> dict[str, Any]:
    """Parse a summary JSON document."""
    try:
        return json.loads(text)
    except json.JSONDecodeError as exc:
        raise SummaryError(f"invalid JSON: {exc}") from exc


def validate_summary(summary: dict[str, Any]) -> dict[str, Any]:
    """Validate the complete v1 interface and all derived aggregates."""
    _ = summary
    raise SummaryError("TODO: implement strict v1 validation")


def render_report(summary: dict[str, Any]) -> str:
    """Validate and render a deterministic Markdown report."""
    _ = summary
    raise SummaryError("TODO: implement report rendering")
