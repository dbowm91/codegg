"""Shared bounded ACP event observation for harness trial receipts."""
from __future__ import annotations

from collections.abc import Iterable, Mapping
from typing import Any

EVENT_GAP_SESSION_UPDATE = "codegg_event_gap"


def observe_notifications(notifications: Iterable[Mapping[str, Any]], *, drained: bool) -> dict[str, Any]:
    """Summarize tool pairing and explicit stream gaps without retaining bodies.

    A quiet drain is the observer-health evidence. Missing notifications before
    a drain is not evidence that no gap occurred, so old/incomplete streams are
    kept distinct from observed gap-free streams.
    """
    starts: set[str] = set()
    terminal_ids: set[str] = set()
    start_count = 0
    terminal_count = 0
    gaps: list[dict[str, Any]] = []
    for notification in notifications:
        if notification.get("method") != "session/update":
            continue
        params = notification.get("params", {})
        update = params.get("update", {}) if isinstance(params, Mapping) else {}
        if not isinstance(update, Mapping):
            continue
        kind = update.get("sessionUpdate")
        if kind == EVENT_GAP_SESSION_UPDATE:
            dropped = update.get("dropped")
            gaps.append({
                "dropped": dropped if isinstance(dropped, int) and not isinstance(dropped, bool) else 0,
                "source": update.get("source") if isinstance(update.get("source"), str) else "unknown",
            })
        elif kind == "tool_call":
            start_count += 1
            call_id = update.get("toolCallId")
            if isinstance(call_id, str) and call_id:
                starts.add(call_id)
        elif kind == "tool_call_update":
            terminal_count += 1
            call_id = update.get("toolCallId")
            if isinstance(call_id, str) and call_id:
                terminal_ids.add(call_id)

    if gaps:
        state = "unmeasured_gap"
    elif drained:
        state = "observed_complete"
    else:
        state = "not_instrumented"
    return {
        "event_gap_count": len(gaps),
        "event_gap_dropped": sum(gap["dropped"] for gap in gaps),
        "event_gaps": gaps,
        "event_observation_state": state,
        "tool_started_count": start_count,
        "tool_terminal_count": terminal_count,
        "tool_unpaired_start_count": len(starts - terminal_ids) if starts else None,
    }
