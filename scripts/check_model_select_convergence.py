#!/usr/bin/env python3
"""M004 guard: ModelSelect must converge on the durable selection service.

Fails when:
- `CoreRequest::ModelSelect` handler mutates `runtime.selected_model`
  without first resolving through `resolve_model_select_target` and
  `service.update` (runtime-only authority regression), or
- the old runtime-only pattern `*selected = Some(model.clone())` remains
  in the ModelSelect arm, or
- `SessionSelectionUpdate` success path does not project into the runtime
  cache and remember the last-used preference.
"""
from __future__ import annotations

import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TURNS = ROOT / "src" / "core" / "daemon_turns.rs"
SESSIONS = ROOT / "src" / "core" / "daemon_sessions.rs"


def extract_arm(path: pathlib.Path, marker: str, length: int = 220) -> str:
    lines = path.read_text().splitlines()
    start = next(
        (i for i, line in enumerate(lines) if marker in line),
        None,
    )
    if start is None:
        print(f"FAIL: {path.name} missing {marker!r}")
        sys.exit(1)
    return "\n".join(lines[start : start + length])


def main() -> int:
    failures: list[str] = []

    turns_arm = extract_arm(TURNS, "CoreRequest::ModelSelect", 260)
    for required in (
        "resolve_model_select_target",
        ".update(",
        "durable_selected_runtime_model",
        "set_model_preference",
        "selection_outcome_code",
    ):
        if required not in turns_arm:
            failures.append(f"daemon_turns ModelSelect arm missing {required}")
    if "*selected = Some(model.clone())" in turns_arm:
        failures.append(
            "daemon_turns ModelSelect arm still uses runtime-only "
            "`*selected = Some(model.clone())`"
        )
    if "session_selection_unavailable" not in turns_arm:
        failures.append("daemon_turns ModelSelect arm must fail closed without a catalog")

    sessions_text = SESSIONS.read_text()
    update_idx = sessions_text.find("CoreRequest::SessionSelectionUpdate")
    if update_idx == -1:
        failures.append("daemon_sessions missing SessionSelectionUpdate arm")
    else:
        update_arm = sessions_text[update_idx : update_idx + 12000]
        for required in (
            "durable_selected_runtime_model",
            "set_model_preference",
            "session selection persisted but last-used preference was not saved",
        ):
            if required not in update_arm:
                failures.append(f"daemon_sessions SelectionUpdate arm missing {required}")

    selection_rs = (ROOT / "src" / "core" / "session_selection.rs").read_text()
    for required in (
        "has_explicit_selection",
        "apply_last_used_preference",
        "resolve_model_select_target",
        "durable_selected_runtime_model",
    ):
        if required not in selection_rs:
            failures.append(f"session_selection.rs missing {required}")

    approval_rs = (
        ROOT / "crates" / "codegg-core" / "src" / "approval.rs"
    ).read_text()
    if "PreferenceApplicationOutcome" not in approval_rs:
        failures.append("approval.rs missing PreferenceApplicationOutcome")

    if failures:
        for failure in failures:
            print(f"FAIL: {failure}")
        return 1
    print("model-select convergence guard: pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
