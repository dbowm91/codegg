"""Load the case's parser without depending on the fixture's own test file.

The verifier must be able to judge the candidate's work without trusting
anything the candidate could have edited, so it imports the implementation
directly and re-derives the contract itself.
"""
from __future__ import annotations

import importlib
from pathlib import Path
import sys


def case_root() -> Path:
    """Return the case directory that holds ``fixture/`` and ``verifier/``."""
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
