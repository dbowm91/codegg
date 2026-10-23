#!/usr/bin/env python3
"""Static guard for the model-profile capability mirror.

`crates/codegg-core/build.rs` parses the shipped adapter assets with its
**own mirror** of the profile shape, and the runtime parses the same
assets with the real `AdapterProfile`. The two declarations are
hand-maintained and they can drift.

The drift is expensive to diagnose. A field added to the runtime profile
but not to the build script's mirror does not fail as "the two shapes
disagree" — it fails as a `deny_unknown_fields` error against whichever
adapter asset happens to declare the field, which reads like a data
problem in an asset nobody was editing. The same happens in reverse: a
field added only to the build script's mirror is silently dropped from
every asset.

This guard asserts that the runtime types and the build script's mirrors
declare **the same field set**:

1. `AdapterProfile` (`crates/codegg-core/src/model_profile/adapter.rs`)
   against the build script's `Profile`.
2. `ReasoningCapabilityRecord` (`crates/codegg-config/src/schema.rs`)
   against the build script's `ReasoningCapability`.

The second pair matters because the reasoning-capability record is the
one this milestone adds: it lands in the runtime profile, in the build
script's mirror, and in the configuration schema, and a one-sided change
to any of the three is a silent loss — the asset would stop carrying the
record, or the record would stop parsing.

**The scan looks at code, not at text.** A doc comment that *names* a
field, or a string literal that *contains* a field declaration, must not
satisfy a rule that asks whether the code declares that field. The
scanner below blanks comments and string contents before it looks for a
struct body, so a token that only appears inside a literal cannot count.
The `--self-test` injects that bypass, plus a one-sided field on each
side, and proves every one of them is rejected.

Run with `--self-test` to prove the guard fails on injected violations. A
guard that has never been shown to reject anything is a comment.

Exit code 0 if all checks pass, 1 if any fail.
"""

from __future__ import annotations

import re
import sys

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
# The runtime profile type and the runtime capability-record type.
RUNTIME_PROFILE = REPO_ROOT / "crates" / "codegg-core" / "src" / "model_profile" / "adapter.rs"
RUNTIME_RECORD = REPO_ROOT / "crates" / "codegg-config" / "src" / "schema.rs"
# The build script's hand-maintained mirrors.
BUILD_SCRIPT = REPO_ROOT / "crates" / "codegg-core" / "build.rs"

# The struct declarations the guard compares.
RUNTIME_PROFILE_STRUCT = "AdapterProfile"
RUNTIME_RECORD_STRUCT = "ReasoningCapabilityRecord"
MIRROR_PROFILE_STRUCT = "Profile"
MIRROR_RECORD_STRUCT = "ReasoningCapability"

# A struct declaration. The name is followed by a `{` on the same or a
# later line; the body is matched by brace counting.
STRUCT_DECLARATION = re.compile(r"\bstruct\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)\b")

# A field declaration inside a struct body. The `pub` prefix is optional
# because the build script's mirror declares private fields.
FIELD_DECLARATION = re.compile(r"^(?:pub\s+)?(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*:")


def _blank(chunk: str) -> str:
    """Blank a chunk, preserving newlines so line numbers stay honest."""
    return "".join("\n" if char == "\n" else " " for char in chunk)


def _code(source: str) -> str:
    """The source with comments and string contents blanked.

    The scanner is **string-aware**: a string or character literal is
    consumed as a unit before any comment delimiter inside it can be
    mistaken for a comment, and the literal's *contents* are blanked so
    that a token which appears only inside a literal cannot satisfy a
    rule that asks whether the code uses it.
    """
    out: list[str] = []
    index = 0
    length = len(source)
    while index < length:
        char = source[index]

        # A raw string: r"…", r#"…"#, r##"…"##.
        if char == "r" and index + 1 < length and source[index + 1] in '"#':
            cursor = index + 1
            hashes = 0
            while cursor < length and source[cursor] == "#":
                hashes += 1
                cursor += 1
            if cursor < length and source[cursor] == '"':
                terminator = '"' + ("#" * hashes)
                end = source.find(terminator, cursor + 1)
                end = length if end == -1 else end + len(terminator)
                out.append(_blank(source[index:end]))
                index = end
                continue

        # An ordinary string literal, with backslash escapes.
        if char == '"':
            cursor = index + 1
            while cursor < length:
                if source[cursor] == "\\":
                    cursor += 2
                    continue
                if source[cursor] == '"':
                    cursor += 1
                    break
                cursor += 1
            out.append(_blank(source[index:cursor]))
            index = cursor
            continue

        # A character literal. A lifetime (`'a`) matches neither shape and
        # falls through as ordinary text.
        if char == "'":
            cursor = index + 1
            if cursor < length and source[cursor] == "\\":
                while cursor < length and source[cursor] != "'":
                    cursor += 1
                cursor = min(cursor + 1, length)
                out.append(_blank(source[index:cursor]))
                index = cursor
                continue
            if cursor + 1 < length and source[cursor + 1] == "'":
                cursor += 2
                out.append(_blank(source[index:cursor]))
                index = cursor
                continue

        if source.startswith("/*", index):
            end = source.find("*/", index + 2)
            end = length if end == -1 else end + 2
            out.append(_blank(source[index:end]))
            index = end
            continue

        if source.startswith("//", index):
            end = source.find("\n", index)
            end = length if end == -1 else end
            out.append(" " * (end - index))
            index = end
            continue

        out.append(char)
        index += 1
    return "".join(out)


def struct_fields(source: str, struct_name: str) -> list[str] | None:
    """The declared field names of one struct, read from code.

    Returns `None` when the struct is not declared at all, which is a
    violation in its own right: the guard cannot compare a mirror with a
    type that has gone away.
    """
    code = _code(source)
    for match in STRUCT_DECLARATION.finditer(code):
        if match.group("name") != struct_name:
            continue
        open_brace = code.find("{", match.end())
        if open_brace == -1:
            return None
        depth = 0
        close_brace = len(code)
        for index in range(open_brace, len(code)):
            character = code[index]
            if character == "{":
                depth += 1
            elif character == "}":
                depth -= 1
                if depth == 0:
                    close_brace = index
                    break
        body = code[open_brace + 1 : close_brace]
        fields: list[str] = []
        for line in body.splitlines():
            stripped = line.strip()
            if not stripped:
                continue
            field = FIELD_DECLARATION.match(stripped)
            if field:
                fields.append(field.group("name"))
        return fields
    return None


def compare_fields(
    runtime_source: str,
    runtime_struct: str,
    mirror_source: str,
    mirror_struct: str,
    runtime_label: str,
    mirror_label: str,
) -> list[str]:
    """The differences between a runtime type's fields and a mirror's."""
    runtime = struct_fields(runtime_source, runtime_struct)
    mirror = struct_fields(mirror_source, mirror_struct)
    if runtime is None:
        return [f"{runtime_label} does not declare `struct {runtime_struct}`"]
    if mirror is None:
        return [f"{mirror_label} does not declare `struct {mirror_struct}`"]

    violations: list[str] = []
    missing_in_mirror = sorted(set(runtime) - set(mirror))
    missing_in_runtime = sorted(set(mirror) - set(runtime))
    for name in missing_in_mirror:
        violations.append(
            f"{runtime_label} declares `{name}` but the {mirror_label} mirror does not — "
            "a one-sided change here surfaces as a deny_unknown_fields error against an "
            "adapter asset, not as a shape mismatch"
        )
    for name in missing_in_runtime:
        violations.append(
            f"{mirror_label} declares `{name}` but {runtime_label} does not — the field is "
            "silently dropped from every adapter asset"
        )
    return violations


def _read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except (UnicodeError, OSError) as error:
        raise OSError(f"cannot read {path}: {error}") from error


def check(root: Path) -> list[str]:
    """Every mirror-drift violation in the tree."""
    runtime_profile = _read(root / RUNTIME_PROFILE.relative_to(REPO_ROOT))
    runtime_record = _read(root / RUNTIME_RECORD.relative_to(REPO_ROOT))
    build_script = _read(root / BUILD_SCRIPT.relative_to(REPO_ROOT))

    violations = compare_fields(
        runtime_profile,
        RUNTIME_PROFILE_STRUCT,
        build_script,
        MIRROR_PROFILE_STRUCT,
        "crates/codegg-core/src/model_profile/adapter.rs",
        "crates/codegg-core/build.rs",
    )
    violations.extend(
        compare_fields(
            runtime_record,
            RUNTIME_RECORD_STRUCT,
            build_script,
            MIRROR_RECORD_STRUCT,
            "crates/codegg-config/src/schema.rs",
            "crates/codegg-core/build.rs",
        )
    )
    return violations


def _self_test() -> int:
    print("check_model_profile_capability_mirror --self-test")

    runtime_profile = _read(RUNTIME_PROFILE)
    runtime_record = _read(RUNTIME_RECORD)
    build_script = _read(BUILD_SCRIPT)

    # Violation 1: a field added to the runtime profile but not to the
    # build script's mirror.
    injected_runtime = runtime_profile.replace(
        "    pub max_parallel_tools: Option<usize>,",
        "    pub max_parallel_tools: Option<usize>,\n"
        "    pub capability_probe: Option<String>,",
        1,
    )
    if injected_runtime == runtime_profile:
        print("  FAIL: self-test could not find the runtime profile's field list")
        return 1
    violations = compare_fields(
        injected_runtime,
        RUNTIME_PROFILE_STRUCT,
        build_script,
        MIRROR_PROFILE_STRUCT,
        "runtime profile",
        "build script",
    )
    if not violations:
        print(
            "  FAIL: self-test added a one-sided runtime field and the guard still "
            "passed. The guard is not load-bearing."
        )
        return 1
    print("  ok: injected one-sided runtime field rejected")

    # Violation 2: a field added to the build script's mirror but not to
    # the runtime profile.
    injected_build = build_script.replace(
        "    max_parallel_tools: Option<usize>,",
        "    max_parallel_tools: Option<usize>,\n    capability_probe: Option<String>,",
        1,
    )
    if injected_build == build_script:
        print("  FAIL: self-test could not find the build script's field list")
        return 1
    violations = compare_fields(
        runtime_profile,
        RUNTIME_PROFILE_STRUCT,
        injected_build,
        MIRROR_PROFILE_STRUCT,
        "runtime profile",
        "build script",
    )
    if not violations:
        print(
            "  FAIL: self-test added a one-sided mirror field and the guard still "
            "passed. The guard is not load-bearing."
        )
        return 1
    print("  ok: injected one-sided mirror field rejected")

    # Violation 3: a one-sided field in the capability record.
    injected_record = runtime_record.replace(
        "    pub always_reasons: Option<bool>,",
        "    pub always_reasons: Option<bool>,\n    pub capability_probe: Option<String>,",
        1,
    )
    if injected_record == runtime_record:
        print("  FAIL: self-test could not find the capability record's field list")
        return 1
    violations = compare_fields(
        injected_record,
        RUNTIME_RECORD_STRUCT,
        build_script,
        MIRROR_RECORD_STRUCT,
        "runtime record",
        "build script",
    )
    if not violations:
        print(
            "  FAIL: self-test added a one-sided capability-record field and the guard "
            "still passed. The guard is not load-bearing."
        )
        return 1
    print("  ok: injected one-sided capability-record field rejected")

    # Violation 4: the text bypass. A field declaration that only appears
    # inside a string literal must not count as a declared field.
    bypass = (
        "pub struct AdapterProfile {\n"
        "    pub family: Option<String>,\n"
        '    const NOTE: &str = "pub capability_probe: Option<String>,";\n'
        "}\n"
    )
    fields = struct_fields(bypass, RUNTIME_PROFILE_STRUCT)
    if fields is None or "capability_probe" in fields:
        print(
            "  FAIL: self-test put a field declaration inside a string literal and the "
            "guard counted it. The guard must look at code, not at text."
        )
        return 1
    print("  ok: a field declaration inside a string literal is not counted")

    # And the unmodified tree must still pass, so the self-test proves the
    # guard rejects rather than being broken.
    if check(REPO_ROOT):
        print("  FAIL: self-test passed but the real tree has violations")
        return 1
    print("  ok: unmodified tree still passes")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        return _self_test()

    try:
        violations = check(REPO_ROOT)
    except OSError as error:
        print(f"  FAIL: {error}")
        violations = [str(error)]
    for violation in violations:
        print(f"  FAIL: {violation}")
    if violations:
        print(f"check_model_profile_capability_mirror: {len(violations)} failing")
        return 1
    print("check_model_profile_capability_mirror: all checks pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
