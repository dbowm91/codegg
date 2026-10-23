"""Resolve the case roots the verifier needs.

The verifier runs in two situations and must work in both:

* **Standalone (in-repo).** The verifier sits at ``<case root>/verifier/`` and
  is run with the case root as the working directory.
* **Live lane.** The harness copies ``fixture/`` into a private lane
  workspace and copies ``verifier/`` somewhere else entirely, then exports
  ``CODEGG_AUTONOMY_CASE_ROOT`` (and ``CODEGG_AUTONOMY_TRANSCRIPT``).

So every path is resolved through :func:`case_root`, which honours the
environment override and falls back to the directory that holds this file's
parent.
"""
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
