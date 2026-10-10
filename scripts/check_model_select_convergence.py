#!/usr/bin/env python3
"""M004 guard: ModelSelect must converge on the durable selection service.

Fails when:
- `CoreRequest::ModelSelect` handler mutates `runtime.selected_model`
  without first resolving through `resolve_model_select_target` and
  `service.update` (runtime-only authority regression), or
- the old runtime-only pattern `*selected = Some(model.clone())` remains
  in the ModelSelect arm, or
- `SessionSelectionUpdate` success path does not project into the runtime
  cache and remember the last-used preference, or
- the TUI `/model` commit path does not persist the choice to the daemon.
  The daemon resolves every turn from its durable `SessionSelection`, so a
  dialog that only mutated local state left the status line advertising a
  model the next turn would never run. That divergence is silent, so it is
  pinned here.
"""
from __future__ import annotations

import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TURNS = ROOT / "src" / "core" / "daemon_turns.rs"
SESSIONS = ROOT / "src" / "core" / "daemon_sessions.rs"
TUI_INPUT = ROOT / "src" / "tui" / "app" / "input.rs"
TUI_COMMANDS = ROOT / "src" / "tui" / "app" / "commands.rs"


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


def extract_fn_body(path: pathlib.Path, signature: str) -> str:
    """Text of one function, bounded by the next sibling method.

    A fixed line window is unsafe here: it can reach past the closing
    brace and pick up the *definition* of a helper it was meant to check
    for a *call* of, which would make the check vacuous.
    """
    lines = path.read_text().splitlines()
    start = next((i for i, l in enumerate(lines) if signature in l), None)
    if start is None:
        print(f"FAIL: {path.name} missing {signature!r}")
        sys.exit(1)
    for i in range(start + 1, len(lines)):
        line = lines[i]
        if line.startswith("    pub fn ") or line.startswith("    pub(crate) fn ") or line.startswith("    fn "):
            return "\n".join(lines[start:i])
    return "\n".join(lines[start:])


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

    # The TUI half of the contract: `/model` must reach the durable
    # selection, and the refusal must be reportable.
    select_arm = extract_arm(TUI_INPUT, "TuiMsg::SelectModel", 40)
    if "persist_durable_model_selection" not in select_arm:
        failures.append(
            "tui SelectModel arm must persist the choice via "
            "persist_durable_model_selection"
        )
    commands_rs = TUI_COMMANDS.read_text()
    for variant in ("ModelSelectPersist", "ModelSelectPersisted"):
        if variant not in commands_rs:
            failures.append(f"TuiCommand missing {variant}")

    # A tab restored from the manifest, or a brand-new one, starts with
    # `session_id: null`. A `/model` choice made then has no durable row
    # to write, so the session created afterwards must adopt it — otherwise
    # the same divergence reappears one step later, at session creation.
    set_session = extract_fn_body(
        ROOT / "src" / "tui" / "app" / "mod.rs", "pub fn set_session("
    )
    if "persist_durable_model_selection" not in set_session:
        failures.append(
            "App::set_session must adopt a model chosen before the session existed"
        )
    elif "if !session_has_own_selection" not in set_session:
        # The gate matters as much as the call: an unconditional adopt
        # would overwrite a restored session's own durable selection.
        failures.append(
            "App::set_session must adopt the pending model only when the "
            "session has no selection of its own (gate on "
            "`if !session_has_own_selection`)"
        )

    if failures:
        for failure in failures:
            print(f"FAIL: {failure}")
        return 1
    print("model-select convergence guard: pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
