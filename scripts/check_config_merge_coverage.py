#!/usr/bin/env python3
"""Static guard for config layer-merge coverage.

`merge_configs` (crates/codegg-config/src/paths.rs) is an explicit
whitelist: it combines a hand-written `merge_option!` identifier list with
one hand-written arm per map/nested-struct field. It is the ONLY path from a
parsed config file to `Config` -- `Config::load` and `ConfigWatcher::reload_config`
both call it -- so a `Config` field with no arm is discarded for every
config layer, not just for multi-layer loads. The setting then parses
without error and silently does nothing.

This guard derives the `Config` field set from the struct definition and the
merged field set from `merge_configs`, then requires them to be equal. A new
field with no merge arm fails the guard, which forces the arm to be written
in the same change.

The top-level check is not enough on its own. A nested configuration struct
that owns its own `merge` method is merged by that method, so a field added
to it with no arm is dropped for every layer exactly like an uncovered
`Config` field -- and the top-level check cannot see it, because the nested
struct appears on `Config` as a single covered field. `ProviderConfig` was
the proof: `chunk_timeout` sat in the schema and in the merge for as long as
the config crate existed, and no provider ever read it.

So the guard also descends into the nested configuration structs. The set of
structs it descends into is DERIVED FROM THE SOURCE, not listed by hand: the
guard walks the field types reachable from `Config` and requires merge
coverage of every reachable struct that owns its own `merge` method. A new
nested configuration struct is therefore covered automatically, and a struct
that is not reachable from `Config` is left alone.

A reachable struct that owns no `merge` method of its own is not required to
have merge arms: it is never merged field by field, so its whole value is
replaced by its parent's arm and there is nothing for the guard to compare.

No field names are embedded here: adding a covered field passes without
touching this file, and adding an uncovered one fails.

Exit code 0 if all checks pass, 1 if any fail.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CONFIG_SRC_DIR = REPO_ROOT / "crates" / "codegg-config" / "src"
SCHEMA_MODULE = CONFIG_SRC_DIR / "schema.rs"
PATHS_MODULE = CONFIG_SRC_DIR / "paths.rs"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _strip_comments(source: str) -> str:
    """Remove line and block comments, preserving string literals.

    Merge coverage is read from code, so a commented-out arm must not count
    as coverage and a commented-out field must not count as a field.
    """
    out: list[str] = []
    i = 0
    in_string = False
    while i < len(source):
        ch = source[i]
        if in_string:
            if ch == "\\":
                out.append(source[i : i + 2])
                i += 2
                continue
            if ch == '"':
                in_string = False
            out.append(ch)
            i += 1
            continue
        if ch == '"':
            in_string = True
            out.append(ch)
            i += 1
            continue
        if source.startswith("//", i):
            while i < len(source) and source[i] != "\n":
                i += 1
            continue
        if source.startswith("/*", i):
            end = source.find("*/", i + 2)
            i = len(source) if end == -1 else end + 2
            continue
        out.append(ch)
        i += 1
    return "".join(out)


def _braced_block(source: str, header: re.Match[str]) -> str:
    """Return the text inside the braces of a block found by `header`."""
    start = source.index("{", header.start())
    depth = 0
    for j in range(start, len(source)):
        if source[j] == "{":
            depth += 1
        elif source[j] == "}":
            depth -= 1
            if depth == 0:
                return source[start + 1 : j]
    raise ValueError(f"unbalanced braces after {header.group(0)!r}")


def _field_type(body: str, start: int) -> str:
    """Read one field's type, honouring nested `<>`, `()` and `[]`.

    A field type may itself contain commas (`HashMap<String, ProviderConfig>`),
    so the type ends at the first comma that is not inside a generic.
    """
    depth = 0
    out: list[str] = []
    for ch in body[start:]:
        if ch in "<([":
            depth += 1
        elif ch in ">)]":
            depth -= 1
        elif ch == "," and depth == 0:
            break
        out.append(ch)
    return "".join(out).strip()


def config_fields(source: str) -> list[str]:
    """Field names of the `Config` struct, in declaration order."""
    source = _strip_comments(source)
    header = re.search(r"^pub struct Config\s*\{", source, re.M)
    if not header:
        raise ValueError("`pub struct Config {` not found")
    body = _braced_block(source, header)
    return re.findall(r"^\s*pub\s+(\w+)\s*:", body, re.M)


def merged_fields(source: str) -> set[str]:
    """Fields `merge_configs` actually reads from each layer."""
    source = _strip_comments(source)
    header = re.search(r"^pub fn merge_configs\s*\(", source, re.M)
    if not header:
        raise ValueError("`pub fn merge_configs(` not found")
    body = _braced_block(source, header)

    covered: set[str] = set()

    # `merge_option!(merged, config, a, b, c);` -- the bulk scalar path.
    for invocation in re.findall(r"merge_option!\s*\((.*?)\)\s*;", body, re.S):
        identifiers = [part.strip() for part in invocation.split(",")]
        if len(identifiers) >= 3 and identifiers[1] == "config":
            covered.update(identifiers[2:])

    # Hand-written arms read from the layer as `config.<field>`.
    covered.update(re.findall(r"\bconfig\.(\w+)", body))
    return covered


def unmerged_fields(schema_source: str, paths_source: str) -> list[str]:
    """Fields present on `Config` that `merge_configs` never reads."""
    fields = config_fields(schema_source)
    covered = merged_fields(paths_source)
    return [name for name in fields if name not in covered]


def struct_definitions(sources: list[tuple[str, str]]) -> dict[str, list[tuple[str, str]]]:
    """Every `pub struct` in the sources: name -> [(field name, field type)]."""
    definitions: dict[str, list[tuple[str, str]]] = {}
    for _path, source in sources:
        source = _strip_comments(source)
        for header in re.finditer(r"^pub struct (\w+)\s*\{", source, re.M):
            body = _braced_block(source, header)
            fields: list[tuple[str, str]] = []
            for field in re.finditer(r"^\s*pub\s+(\w+)\s*:", body, re.M):
                fields.append((field.group(1), _field_type(body, field.end())))
            definitions[header.group(1)] = fields
    return definitions


def merge_owners(sources: list[tuple[str, str]]) -> dict[str, tuple[str, str]]:
    """Every struct that owns a `merge` method: name -> (parameter, body).

    The parameter name is read from the signature rather than assumed, so a
    rename of the merged-in value does not silently blind the guard.
    """
    owners: dict[str, tuple[str, str]] = {}
    for _path, source in sources:
        source = _strip_comments(source)
        for header in re.finditer(r"^impl\s+(\w+)\s*\{", source, re.M):
            impl_body = _braced_block(source, header)
            method = re.search(
                r"pub fn merge\s*\(\s*&mut self\s*,\s*(\w+)\s*:", impl_body
            )
            if not method:
                continue
            owners[header.group(1)] = (
                method.group(1),
                _braced_block(impl_body, method),
            )
    return owners


def reachable_structs(
    definitions: dict[str, list[tuple[str, str]]],
) -> set[str]:
    """Structs reachable from `Config` through their field types.

    Reachability is a name walk over the field types, so a nested
    configuration struct is covered without this file naming it.
    """
    if "Config" not in definitions:
        raise ValueError("`pub struct Config {` not found")
    reachable: set[str] = set()
    frontier = ["Config"]
    while frontier:
        current = frontier.pop()
        if current in reachable:
            continue
        reachable.add(current)
        for _field, field_type in definitions.get(current, []):
            for name in re.findall(r"\b[A-Z]\w*\b", field_type):
                if name in definitions and name not in reachable:
                    frontier.append(name)
    return reachable


def merged_members(merge_body: str, parameter: str) -> set[str]:
    """Fields a struct's own `merge` method touches.

    An arm reads the incoming layer as `<parameter>.<field>` and writes the
    accumulated layer as `self.<field>`; either mention is a merge arm.
    """
    members = set(re.findall(rf"\b{re.escape(parameter)}\.(\w+)", merge_body))
    members.update(re.findall(r"\bself\.(\w+)", merge_body))
    return members


def unmerged_nested_fields(sources: list[tuple[str, str]]) -> list[tuple[str, str]]:
    """Reachable nested config fields with no arm in their own `merge`.

    Only structs that own a `merge` method are considered: a reachable
    struct without one is never merged field by field, so there are no arms
    to compare it against.
    """
    definitions = struct_definitions(sources)
    owners = merge_owners(sources)
    reachable = reachable_structs(definitions)

    unmerged: list[tuple[str, str]] = []
    for name in sorted(owners):
        if name not in reachable:
            continue
        parameter, merge_body = owners[name]
        covered = merged_members(merge_body, parameter)
        for field, _field_type in definitions.get(name, []):
            if field not in covered:
                unmerged.append((name, field))
    return unmerged


def _self_test() -> int:
    """Prove the guard is future-proof and still sensitive.

    A field added to both `Config` and `merge_configs` must pass without
    editing this guard; a field added only to `Config` must fail. The same
    two properties must hold for a nested configuration struct that owns its
    own `merge` method, because that is where a dead key survived.
    """
    import tempfile

    base_struct = """pub struct Config {{
    pub log_level: Option<String>,
{maybe}    pub model: Option<String>,
}}
"""

    base_merge = """pub fn merge_configs(configs: &[Config]) -> Config {{
    let mut merged = Config::default();
    for config in configs {{
        merge_option!(
            merged,
            config,
            log_level,
            model,{maybe_macro}
        );
    }}
    merged
}}
"""

    nested_struct = """pub struct Config {{
    pub log_level: Option<String>,
    pub provider: Option<HashMap<String, ProviderConfig>>,
}}

pub struct ProviderConfig {{
    pub api_key: Option<String>,
{maybe_field}}}

impl ProviderConfig {{
    pub fn merge(&mut self, other: &ProviderConfig) {{
        if other.api_key.is_some() {{
            self.api_key.clone_from(&other.api_key);
        }}
{maybe_arm}    }}
}}
"""

    nested_merge = """pub fn merge_configs(configs: &[Config]) -> Config {{
    let mut merged = Config::default();
    for config in configs {{
        merge_option!(
            merged,
            config,
            log_level,
            provider
        );
    }}
    merged
}}
"""

    cases = [
        # (description, field_on_config, field_in_merge, should_pass)
        ("field covered by merge_option! passes", "scheduler", "scheduler", True),
        (
            "field on Config with no merge arm fails",
            "scheduler",
            None,
            False,
        ),
        ("field covered by a later field passes", "scheduler", "scheduler", True),
        ("Config with no extra field passes", None, None, True),
    ]

    failures = 0
    for description, struct_extra, merge_extra, should_pass in cases:
        struct_src = base_struct.format(
            maybe=(
                f"    pub {struct_extra}: Option<SchedulerConfig>,\n" if struct_extra else ""
            )
        )
        merge_src = base_merge.format(
            maybe_macro=(f"\n            {merge_extra}" if merge_extra else "")
        )
        with tempfile.TemporaryDirectory() as raw:
            tmp = Path(raw)
            schema_path = tmp / "schema.rs"
            paths_path = tmp / "paths.rs"
            schema_path.write_text(struct_src, encoding="utf-8")
            paths_path.write_text(merge_src, encoding="utf-8")
            try:
                unmerged = unmerged_fields(_read(schema_path), _read(paths_path))
                ok = (not unmerged) if should_pass else bool(unmerged)
            except ValueError:
                ok = False
        print(f"  [{'PASS' if ok else 'FAIL'}] self-test: {description}")
        if not ok:
            failures += 1

    # The nested cases: a nested field that owns a merge arm passes, and the
    # same field with no arm fails -- the `chunk_timeout` shape.
    nested_cases = [
        # (description, field_on_struct, field_in_merge, should_pass)
        ("nested field covered by its merge arm passes", True, True, True),
        ("nested field with no merge arm fails", True, False, False),
        ("nested struct with no extra field passes", False, False, True),
    ]
    for description, struct_extra, merge_extra, should_pass in nested_cases:
        struct_src = nested_struct.format(
            maybe_field=(
                "    pub timeout: Option<u64>,\n" if struct_extra else ""
            ),
            maybe_arm=(
                "        if other.timeout.is_some() {\n"
                "            self.timeout.clone_from(&other.timeout);\n"
                "        }\n"
                if merge_extra
                else ""
            ),
        )
        merge_src = nested_merge
        with tempfile.TemporaryDirectory() as raw:
            tmp = Path(raw)
            schema_path = tmp / "schema.rs"
            paths_path = tmp / "paths.rs"
            schema_path.write_text(struct_src, encoding="utf-8")
            paths_path.write_text(merge_src, encoding="utf-8")
            try:
                unmerged = unmerged_nested_fields(
                    [("schema.rs", _read(schema_path)), ("paths.rs", _read(paths_path))]
                )
                ok = (not unmerged) if should_pass else bool(unmerged)
            except ValueError:
                ok = False
        print(f"  [{'PASS' if ok else 'FAIL'}] self-test: {description}")
        if not ok:
            failures += 1

    # The real tree must be fully covered.
    try:
        real = unmerged_fields(_read(SCHEMA_MODULE), _read(PATHS_MODULE))
    except ValueError as exc:
        print(f"  [FAIL] self-test: real tree parse error: {exc}")
        real = ["<parse error>"]
    real_ok = not real
    print(f"  [{'PASS' if real_ok else 'FAIL'}] self-test: real tree is fully covered")
    if not real_ok:
        failures += 1

    try:
        nested_real = unmerged_nested_fields(_config_sources())
    except ValueError as exc:
        print(f"  [FAIL] self-test: real tree nested parse error: {exc}")
        nested_real = [("<parse error>", "<parse error>")]
    nested_ok = not nested_real
    print(
        f"  [{'PASS' if nested_ok else 'FAIL'}] self-test: real tree nested structs "
        "are fully covered"
    )
    if not nested_ok:
        failures += 1

    if failures:
        print(f"{failures} self-test case(s) FAILED.")
        return 1
    print("All guard self-test cases passed.")
    return 0


def check_modules_exist() -> bool:
    return SCHEMA_MODULE.is_file() and PATHS_MODULE.is_file()


def check_merge_coverage() -> bool:
    """Every `Config` field must be read by `merge_configs`."""
    try:
        unmerged = unmerged_fields(_read(SCHEMA_MODULE), _read(PATHS_MODULE))
    except ValueError as exc:
        print(f"  FAIL: {exc}")
        return False
    if unmerged:
        print(f"  FAIL: {len(unmerged)} Config field(s) have no merge arm:")
        for name in unmerged:
            print(f"        - {name}")
        print(
            "  A field with no arm is dropped for EVERY layer, so the setting "
            "parses and then silently does nothing."
        )
        return False
    return True


def check_nested_merge_coverage() -> bool:
    """Every reachable nested config struct field must have a merge arm."""
    try:
        unmerged = unmerged_nested_fields(_config_sources())
    except ValueError as exc:
        print(f"  FAIL: {exc}")
        return False
    if unmerged:
        print(
            f"  FAIL: {len(unmerged)} nested config field(s) have no merge arm:"
        )
        for struct_name, field in unmerged:
            print(f"        - {struct_name}.{field}")
        print(
            "  A nested struct that owns its `merge` method drops an unarmed "
            "field for EVERY layer, so the setting parses and then silently "
            "does nothing."
        )
        return False
    return True


def _config_sources() -> list[tuple[str, str]]:
    """Every Rust source in the config crate, as (name, text) pairs."""
    return [
        (str(path.relative_to(REPO_ROOT)), _read(path))
        for path in sorted(CONFIG_SRC_DIR.rglob("*.rs"))
    ]


def main() -> int:
    if "--self-test" in sys.argv:
        return _self_test()
    verbose = "--verbose" in sys.argv or "-v" in sys.argv
    checks: list[tuple[str, object]] = [
        ("Config + paths modules exist", check_modules_exist),
        ("every Config field has a merge_configs arm", check_merge_coverage),
        (
            "every reachable nested config struct field has a merge arm",
            check_nested_merge_coverage,
        ),
    ]

    results: list[tuple[str, bool]] = []
    for name, check_fn in checks:
        if verbose:
            print(f"CHECK: {name} ... ", end="", flush=True)
        ok = check_fn()  # type: ignore[operator]
        if verbose:
            print("PASS" if ok else "FAIL")
        results.append((name, ok))

    print()
    passed = sum(1 for _, ok in results if ok)
    failed = sum(1 for _, ok in results if not ok)

    for name, ok in results:
        status = "PASS" if ok else "FAIL"
        print(f"  [{status}] {name}")

    print(f"\n{passed}/{len(results)} checks passed.")
    if failed:
        print(f"{failed} check(s) FAILED.")
        return 1

    print("All config merge coverage invariants verified.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
