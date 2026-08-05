#!/usr/bin/env python3
"""Guard the child-only sandbox boundary and maintained Landlock backend."""

from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
FILES = [
    ROOT / "src/security/sandbox.rs",
    ROOT / "src/python_script/executor.rs",
    ROOT / "src/python_script/sandbox.rs",
    ROOT / "src/bin/codegg-sandbox-helper.rs",
    ROOT / "src/tool/bash.rs",
]
FORBIDDEN = (
    r"landlock_create_ruleset",
    r"landlock_add_rule",
    r"landlock_restrict_self",
    r"SYS_LANDLOCK_",
    r"PR_GET_LANDLOCK",
)

failures: list[str] = []
for path in FILES:
    content = path.read_text()
    for pattern in FORBIDDEN:
        if re.search(pattern, content):
            failures.append(f"{path.relative_to(ROOT)}: forbidden handwritten Landlock symbol {pattern}")
    if path.name == "executor.rs" and ".pre_exec" in content:
        failures.append(f"{path.relative_to(ROOT)}: sandbox policy must not be built in pre_exec")

if failures:
    print("sandbox contract guard failed:", file=sys.stderr)
    print("\n".join(failures), file=sys.stderr)
    raise SystemExit(1)

print("sandbox contract guard passed")
