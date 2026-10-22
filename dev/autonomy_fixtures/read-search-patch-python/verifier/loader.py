"""Load the case's parser without depending on the fixture's own test file.

The verifier must be able to judge the candidate's work without trusting
anything the candidate could have edited, so it imports the implementation
directly and re-derives the contract itself.
"""
from __future__ import annotations

import importlib
import os
from pathlib import Path
import sys

#: The harness sets this when it runs the case in a live lane: the verifier
#: lives outside the agent's workspace there, so the case root — which holds
#: the agent's editable ``fixture/`` — is somewhere else entirely.
CASE_ROOT_ENV = "CODEGG_AUTONOMY_CASE_ROOT"


def case_root() -> Path:
    """Return the case directory that holds ``fixture/`` and ``verifier/``.

    In a live lane the verifier is copied to a private directory so the agent
    under test can neither read nor edit it, and the case root is passed in
    the environment. Standalone, the case root is this file's parent.
    """
    override = os.environ.get(CASE_ROOT_ENV)
    if override:
        return Path(override).expanduser().resolve()
    return Path(__file__).resolve().parents[1]


def load_parser():
    """Import the candidate's ``parser`` module from the fixture's source tree."""
    root = case_root()
    src = root / "fixture" / "src"
    added: list[str] = []
    try:
        if (src / "__init__.py").exists():
            added.append(str(root / "fixture"))
            sys.path.insert(0, added[-1])
            return importlib.import_module("src.parser")
        added.append(str(src))
        sys.path.insert(0, added[-1])
        return importlib.import_module("parser")
    finally:
        for entry in added:
            try:
                sys.path.remove(entry)
            except ValueError:
                pass
