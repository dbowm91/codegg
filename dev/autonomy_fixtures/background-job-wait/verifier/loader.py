"""Resolve the case roots and load the trial transcript.

The verifier runs in two situations and this module is the only place that
knows about either of them:

* **Standalone (in-repo).** ``verifier/`` is a sibling of ``fixture/``, so
  the case root is the parent of this file.
* **Live lane.** The harness copies ``fixture/`` into a private lane
  workspace, copies ``verifier/`` somewhere else entirely, and points the
  verifier at the lane with ``CODEGG_AUTONOMY_CASE_ROOT`` and
  ``CODEGG_AUTONOMY_TRANSCRIPT``.

Everything else in the verifier goes through these helpers, so the same
code judges a shipped case and a live trial.
"""
from __future__ import annotations

import json
import os
from pathlib import Path


def case_root() -> Path:
    """Return the case root: the directory that holds fixture/ and verifier/."""
    override = os.environ.get("CODEGG_AUTONOMY_CASE_ROOT")
    if override:
        return Path(override).expanduser().resolve()
    return Path(__file__).resolve().parents[1]


def fixture_root() -> Path:
    """Return the agent-editable tree."""
    return case_root() / "fixture"


def transcript_path() -> Path:
    """Return the harness-written transcript, or None when there is none."""
    override = os.environ.get("CODEGG_AUTONOMY_TRANSCRIPT")
    if override:
        return Path(override).expanduser().resolve()
    candidate = case_root() / "TRANSCRIPT.json"
    return candidate if candidate.is_file() else None


def load_transcript():
    """Return the transcript mapping, or None when no transcript was written."""
    path = transcript_path()
    if path is None or not path.is_file():
        return None
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)
