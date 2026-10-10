#!/usr/bin/env python3
"""Check first-party skill package identity, local links, and registry aliases."""

from __future__ import annotations

import re
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SKILLS = ROOT / ".opencode" / "skills"
LINK = re.compile(r"(?<!!)\[[^\]]*\]\(([^)]+)\)")
SAFE_NAME = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")


def check_document(path: Path, repo_root: Path) -> list[str]:
    errors: list[str] = []
    text = path.read_text(encoding="utf-8")
    if not text.startswith("---\n") or "\n---" not in text[4:]:
        return [f"{path}: missing frontmatter"]
    frontmatter = text[4 : text.index("\n---", 4)]
    match = re.search(r"^name:\s*([a-z0-9-]+)\s*$", frontmatter, re.M)
    if not match:
        errors.append(f"{path}: missing simple `name` metadata")
    elif match.group(1) != path.parent.name or not SAFE_NAME.fullmatch(match.group(1)):
        errors.append(f"{path}: name must match its directory and portable spelling")
    if not re.search(r"^description:\s*\S", frontmatter, re.M):
        errors.append(f"{path}: missing description metadata")
    for target in LINK.findall(text):
        target = target.strip().split("#", 1)[0]
        if not target or target.startswith(("https://", "http://", "mailto:", "#")):
            continue
        if target.startswith(("<", "`")):
            continue
        relative = path.parent / target
        rooted = repo_root / target
        if not relative.exists() and not rooted.exists():
            errors.append(f"{path}: broken local link {target!r}")
    return errors


def self_test() -> None:
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        package = root / "broken"
        package.mkdir()
        guide = package / "SKILL.md"
        guide.write_text("---\nname: wrong\ndescription: test\n---\n[missing](nope.md)\n", encoding="utf-8")
        errors = check_document(guide, root)
        if len(errors) < 2:
            raise AssertionError("broken fixture did not trigger metadata and link checks")


def main() -> int:
    errors: list[str] = []
    try:
        self_test()
        if not SKILLS.is_dir():
            errors.append(f"missing skills directory: {SKILLS}")
        else:
            documents = sorted(SKILLS.glob("*/SKILL.md"))
            if not documents:
                errors.append("no first-party skill packages found")
            for document in documents:
                errors.extend(check_document(document, ROOT))
        for alias in (ROOT / ".skills", ROOT / ".agents" / "skills"):
            if not alias.is_symlink() or alias.resolve() != SKILLS.resolve():
                errors.append(f"{alias.relative_to(ROOT)} must resolve to .opencode/skills")
    except (OSError, AssertionError, ValueError) as error:
        errors.append(f"guard self-check failed: {error}")
    if errors:
        print("Skill guide checks failed:")
        print("\n".join(f"- {error}" for error in errors))
        return 1
    print("Skill guide package, link, fixture, and alias checks passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
