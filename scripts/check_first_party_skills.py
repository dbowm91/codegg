#!/usr/bin/env python3
"""Check first-party skill metadata, index entries, and local markdown links."""

from __future__ import annotations

import re
import sys
import tempfile
from pathlib import Path


LINK = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
REQUIRED_GUIDES = {"testing-ci", "tool-execution", "security-hardening", "lsp-ide"}


def validate(root: Path, index: str, *, require_guides: bool = True) -> list[str]:
    errors: list[str] = []
    packages = sorted((root / ".opencode/skills").glob("*/SKILL.md"))
    names: set[str] = set()
    for guide in packages:
        package = guide.parent.name
        content = guide.read_text(encoding="utf-8")
        match = re.match(r"\A---\s*\n(.*?)\n---\s*(?:\n|$)", content, re.S)
        if not match:
            errors.append(f"{guide}: missing YAML frontmatter")
            continue
        name_match = re.search(r"(?m)^name:\s*([a-z0-9]+(?:-[a-z0-9]+)*)\s*$", match.group(1))
        if not name_match or name_match.group(1) != package:
            errors.append(f"{guide}: frontmatter name must match package directory")
        if name_match:
            name = name_match.group(1)
            if len(name) > 64:
                errors.append(f"{guide}: package name exceeds 64 characters")
            if name in names:
                errors.append(f"{guide}: duplicate package name {name}")
            names.add(name)
            if f"| `{name}` |" not in index:
                errors.append(f"{guide}: missing AGENTS.md skills index entry")
        description = re.search(r"(?m)^description:\s*(\S.*)$", match.group(1))
        if not description:
            errors.append(f"{guide}: nonempty frontmatter description is required")
        for target in LINK.findall(content):
            target = target.strip().split("#", 1)[0]
            if not target or target.startswith(("http:", "https:", "mailto:", "#")):
                continue
            if not (guide.parent / target).resolve().exists():
                errors.append(f"{guide}: broken relative link {target}")
    if require_guides:
        missing = REQUIRED_GUIDES - names
        if missing:
            errors.append(f"missing required maintenance guides: {', '.join(sorted(missing))}")
    return errors


def self_test() -> None:
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        guide = root / ".opencode/skills/broken/SKILL.md"
        guide.parent.mkdir(parents=True)
        guide.write_text(
            "---\nname: wrong-name\ndescription: fixture\n---\n[broken](missing.md)\n",
            encoding="utf-8",
        )
        errors = validate(root, "", require_guides=False)
        if len(errors) != 3:
            raise AssertionError(f"broken fixture did not fail deterministically: {errors}")


def main() -> int:
    repository = Path(__file__).resolve().parent.parent
    index = (repository / "AGENTS.md").read_text(encoding="utf-8")
    errors = validate(repository, index)
    source = (repository / "src/skills/registry.rs").read_text(encoding="utf-8")
    if '.join(".opencode").join("skills")' not in source:
        errors.append("src/skills/registry.rs: OpenCode project source is not registered")
    canonical = (repository / ".opencode/skills").resolve()
    for alias in [repository / ".skills", repository / ".agents/skills"]:
        if not alias.exists() or alias.resolve() != canonical:
            errors.append(f"{alias}: expected alias to canonical .opencode/skills directory")
    self_test()
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("first-party skill metadata, index, and relative-link checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
