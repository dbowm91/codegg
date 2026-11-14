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

## The declared merge strategy

Coverage alone answers "is this field merged". It does not answer "how", and
the provenance dump in `crates/codegg-config/src/provenance.rs` needs the second
fact: a key's winning layer is computed from a declared combine depth, so a
section whose declaration says "replaced whole" while the merge combines its
fields would attribute keys to the wrong layer -- confidently, which is worse
than not answering.

So the guard also reads `STRATEGY_TABLE` from `provenance.rs` and derives the
expected shape **from the merge arm itself**: a `merge_option!` field replaces
whole (depth 0); an arm that touches `existing.<field>` or calls `.merge(`
combines (depth 1); a map arm that calls `.merge(` on its entries combines one
level deeper (depth 2); a list arm that appends is a concatenation. Every
`Config` field must appear exactly once, the declared depth must equal the
derived one, and an exception (a concatenation, or a whole-value section with an
inherited path) must carry a note naming its rule.

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
PROVENANCE_MODULE = CONFIG_SRC_DIR / "provenance.rs"


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


def _delimited_block(source: str, start: int, open_ch: str, close_ch: str) -> str:
    """Return the text inside the matching delimiters that open at `start`."""
    depth = 0
    for j in range(start, len(source)):
        if source[j] == open_ch:
            depth += 1
        elif source[j] == close_ch:
            depth -= 1
            if depth == 0:
                return source[start + 1 : j]
    raise ValueError(f"unbalanced {open_ch}{close_ch} after offset {start}")


def strategy_entries(source: str) -> list[dict[str, str]]:
    """The declared strategy table, one dict per entry.

    The table is Rust source rather than data because it is compiled into the
    engine; this reader is the second consumer, and the guard's job is to fail
    when the two disagree.
    """
    source = _strip_comments(source)
    header = re.search(r"pub const STRATEGY_TABLE\s*:\s*&\[StrategyEntry\]\s*=\s*&\[", source)
    if not header:
        raise ValueError("`pub const STRATEGY_TABLE: &[StrategyEntry] = &[` not found")
    start = source.index("[", header.end() - 1)
    body = _delimited_block(source, start, "[", "]")

    entries: list[dict[str, str]] = []
    # `scalar("name")` is the plain-override shorthand.
    for call in re.finditer(r"\bscalar\(\s*\"([^\"]+)\"\s*\)", body):
        entries.append(
            {
                "section": call.group(1),
                "shape": "WholeValue",
                "rule": "LastContaining",
                "combine_depth": "0",
                "inherited": "",
                "note": "",
            }
        )

    for literal in re.finditer(r"StrategyEntry\s*\{", body):
        brace = body.index("{", literal.start())
        block = _delimited_block(body, brace, "{", "}")
        entries.append(_entry_from_literal(block))
    return entries


def _entry_from_literal(block: str) -> dict[str, str]:
    """One `StrategyEntry { .. }` literal as a dict."""

    def first(pattern: str) -> str | None:
        match = re.search(pattern, block, re.S)
        return match.group(1) if match else None

    section = first(r'section:\s*"([^"]+)"')
    shape = first(r"shape:\s*MergeShape::(\w+)")
    rule = first(r"rule:\s*AttributionRule::(\w+)")
    depth = first(r"combine_depth:\s*(\d+)")
    inherited = first(r"inherited_paths:\s*&\[(.*?)\]")
    if section is None or shape is None or rule is None or depth is None or inherited is None:
        raise ValueError(f"incomplete StrategyEntry literal:\n{block}")

    # `note` is optional in Rust only because `scalar()` omits it; a literal
    # must spell it, and its presence is what the guard reads.
    note_match = re.search(r'note:\s*"((?:[^"\\]|\\.)*)"', block, re.S)
    if note_match is None:
        raise ValueError(f"StrategyEntry for `{section}` has no `note:` field")

    paths = re.findall(r'"([^"]+)"', inherited)
    return {
        "section": section,
        "shape": shape,
        "rule": rule,
        "combine_depth": depth,
        "inherited": ",".join(paths),
        "note": note_match.group(1).strip(),
    }


def _field_arms(merge_body: str, field: str) -> list[str]:
    """The `if let .. = config.<field> { .. }` bodies for one field."""
    arms: list[str] = []
    for match in re.finditer(rf"\bconfig\.{re.escape(field)}\b", merge_body):
        brace = merge_body.find("{", match.end())
        if brace == -1:
            continue
        try:
            arms.append(_delimited_block(merge_body, brace, "{", "}"))
        except ValueError:
            continue
    return arms


def _derived_strategy(
    field: str, field_type: str, arms: list[str], covered_by_macro: set[str]
) -> tuple[str, str, int]:
    """The strategy a merge arm actually implements.

    Derived from the arm, not from a list: this is what makes the declared
    table falsifiable.
    """
    if field in covered_by_macro:
        return ("WholeValue", "LastContaining", 0)
    block = "\n".join(arms)
    if not block:
        return ("WholeValue", "LastContaining", 0)
    is_map = re.match(r"Option<\s*HashMap<", field_type) is not None
    is_list = re.match(r"Option<\s*Vec<", field_type) is not None
    if is_list and ".extend(" in block:
        return ("Concatenate", "Concatenate", 0)
    if is_map:
        depth = 2 if ".merge(" in block else 1
        return ("UnionOfKeys", "LastContaining", depth)
    if ".merge(" in block or re.search(r"\bexisting\b", block):
        return ("FieldByField", "LastContaining", 1)
    return ("WholeValue", "LastContaining", 0)


def merge_option_fields(source: str) -> set[str]:
    """Fields merged by the `merge_option!` macro in `merge_configs`."""
    source = _strip_comments(source)
    header = re.search(r"^pub fn merge_configs\s*\(", source, re.M)
    if not header:
        raise ValueError("`pub fn merge_configs(` not found")
    body = _braced_block(source, header)
    covered: set[str] = set()
    for invocation in re.findall(r"merge_option!\s*\((.*?)\)\s*;", body, re.S):
        identifiers = [part.strip() for part in invocation.split(",")]
        if len(identifiers) >= 3 and identifiers[1] == "config":
            covered.update(identifiers[2:])
    return covered


SHAPE_FAMILY = {
    "WholeValue": {"WholeValue", "WholeValueWithInherit"},
    "WholeValueWithInherit": {"WholeValue", "WholeValueWithInherit"},
    "FieldByField": {"FieldByField"},
    "UnionOfKeys": {"UnionOfKeys"},
    "Concatenate": {"Concatenate"},
}


def _arm_block_for(paths_source: str, field: str, derived: str, depth: int) -> str:
    """The arm body a derived strategy came from, for the inheritance check."""
    source = _strip_comments(paths_source)
    header = re.search(r"^pub fn merge_configs\s*\(", source, re.M)
    if not header:
        return ""
    if derived == "WholeValue" and depth == 0:
        return "\n".join(_field_arms(_braced_block(source, header), field))
    return ""


def strategy_problems(
    schema_source: str, paths_source: str, provenance_source: str
) -> list[str]:
    """Every disagreement between the strategy table and the merge it describes."""
    problems: list[str] = []
    try:
        fields = config_fields(schema_source)
        definitions = struct_definitions([("schema.rs", schema_source)])
        entries = strategy_entries(provenance_source)
        covered_by_macro = merge_option_fields(paths_source)
    except ValueError as exc:
        return [f"cannot read the strategy table: {exc}"]

    by_section: dict[str, list[dict[str, str]]] = {}
    for entry in entries:
        by_section.setdefault(entry["section"], []).append(entry)

    field_set = set(fields)
    for section, declared in sorted(by_section.items()):
        if len(declared) > 1:
            problems.append(f"`{section}` is declared {len(declared)} times")
        if section not in field_set:
            problems.append(f"`{section}` has a strategy but is not a `Config` field")
    for field in fields:
        if field not in by_section:
            problems.append(f"`{field}` is a `Config` field with no declared strategy")

    field_types = dict(definitions.get("Config", []))
    for section, declared in sorted(by_section.items()):
        if section not in field_set:
            continue
        entry = declared[0]
        shape = entry["shape"]
        rule = entry["rule"]
        try:
            depth = int(entry["combine_depth"])
        except ValueError:
            problems.append(f"`{section}` declares a non-numeric combine depth")
            continue
        arms = _field_arms(_merge_body(paths_source), section)
        derived = _derived_strategy(section, field_types.get(section, ""), arms, covered_by_macro)
        derived_shape, derived_rule, derived_depth = derived

        if shape not in SHAPE_FAMILY:
            problems.append(f"`{section}` declares an unknown shape `{shape}`")
            continue
        if shape not in SHAPE_FAMILY[derived_shape]:
            problems.append(
                f"`{section}` is declared `{shape}` but its merge arm is `{derived_shape}`"
            )
            continue
        if rule != derived_rule:
            problems.append(
                f"`{section}` is declared rule `{rule}` but its merge arm is `{derived_rule}`"
            )
            continue
        if depth != derived_depth:
            problems.append(
                f"`{section}` is declared combine depth {depth} but its merge arm "
                f"combines at {derived_depth}"
            )
        if shape == "WholeValueWithInherit":
            inherited = [path for path in entry["inherited"].split(",") if path]
            if not inherited:
                problems.append(
                    f"`{section}` is whole-value-with-inherit but declares no inherited path"
                )
            block = _arm_block_for(paths_source, section, derived_shape, derived_depth)
            for path in inherited:
                leaf = path.rsplit(".", 1)[-1]
                if leaf not in block:
                    problems.append(
                        f"`{section}` declares inherited path `{path}` but the arm never "
                        f"mentions `{leaf}`"
                    )
            if not entry["note"]:
                problems.append(
                    f"`{section}` is an exception and must name its attribution rule in `note`"
                )
        elif shape == "Concatenate":
            if not entry["note"]:
                problems.append(
                    f"`{section}` is an exception and must name its attribution rule in `note`"
                )
        elif entry["inherited"]:
            problems.append(
                f"`{section}` declares an inherited path but is not whole-value-with-inherit"
            )
    return problems


def _merge_body(source: str) -> str:
    """The body of `merge_configs`."""
    source = _strip_comments(source)
    header = re.search(r"^pub fn merge_configs\s*\(", source, re.M)
    if not header:
        raise ValueError("`pub fn merge_configs(` not found")
    return _braced_block(source, header)


_STRATEGY_SCHEMA = """pub struct Config {
    pub server: Option<ServerConfig>,
    pub instructions: Option<Vec<String>>,
    pub provider: Option<HashMap<String, ProviderConfig>>,
    pub security: Option<SecurityConfig>,
}
"""

_STRATEGY_PATHS = """pub fn merge_configs(configs: &[Config]) -> Config {
    let mut merged = Config::default();
    for config in configs {
        if let Some(ref v) = config.server {
            match &mut merged.server {
                Some(existing) => existing.merge(v),
                None => merged.server = Some(v.clone()),
            }
        }
        if let Some(ref instr) = config.instructions {
            merged
                .instructions
                .get_or_insert_with(Vec::new)
                .extend(instr.clone());
        }
        if let Some(ref providers) = config.provider {
            match &mut merged.provider {
                Some(ref mut existing) => {
                    for (k, v) in providers {
                        existing.merge(v);
                    }
                }
                None => merged.provider = Some(providers.clone()),
            }
        }
        if let Some(ref v) = config.security {
            let mut next = v.clone();
            if next.gates.deny_critical_commands.is_none() {
                next.gates.deny_critical_commands = merged.security.as_ref().and_then(|c| c.gates.deny_critical_commands);
            }
            merged.security = Some(next);
        }
    }
    merged
}
"""


def _strategy_literal(
    section: str,
    shape: str,
    rule: str,
    depth: int,
    inherited: str = "&[]",
    note: str = "",
) -> str:
    return (
        f'    StrategyEntry {{ section: "{section}", shape: MergeShape::{shape}, '
        f"rule: AttributionRule::{rule}, combine_depth: {depth}, "
        f'inherited_paths: {inherited}, note: "{note}" }},\n'
    )


def _strategy_source(entries: list[str]) -> str:
    return (
        "pub const STRATEGY_TABLE: &[StrategyEntry] = &[\n" + "".join(entries) + "];\n"
    )


def _strategy_self_test() -> int:
    """Prove the strategy check rejects every way a declaration can lie.

    Each case removes or corrupts exactly one fact and asserts the guard
    reports it; the baseline case proves the fixture itself is consistent, so a
    case that passes cannot be passing because the fixture was unreadable.
    """
    baseline = [
        _strategy_literal("server", "FieldByField", "LastContaining", 1),
        _strategy_literal(
            "instructions",
            "Concatenate",
            "Concatenate",
            0,
            note="appended in precedence order",
        ),
        _strategy_literal("provider", "UnionOfKeys", "LastContaining", 2),
        _strategy_literal(
            "security",
            "WholeValueWithInherit",
            "LastContaining",
            0,
            inherited='&["gates.deny_critical_commands"]',
            note="carried forward",
        ),
    ]
    cases: list[tuple[str, list[str], str | None]] = [
        ("a complete, consistent table passes", baseline, None),
        (
            "a Config section with no strategy entry fails",
            [entry for entry in baseline if "provider" not in entry],
            "provider",
        ),
        (
            "a strategy entry for no Config section fails",
            baseline + [_strategy_literal("scheduler", "WholeValue", "LastContaining", 0)],
            "scheduler",
        ),
        (
            "a field-by-field arm declared whole-value fails",
            [_strategy_literal("server", "WholeValue", "LastContaining", 0)]
            + baseline[1:],
            "server",
        ),
        (
            "a combined arm declared at depth 0 fails",
            [_strategy_literal("server", "FieldByField", "LastContaining", 0)]
            + baseline[1:],
            "combine depth",
        ),
        (
            "an appending arm declared last-containing fails",
            [baseline[0]]
            + [_strategy_literal("instructions", "Concatenate", "LastContaining", 0, note="x")]
            + baseline[2:],
            "instructions",
        ),
        (
            "an exception with no note fails",
            baseline[:3]
            + [
                _strategy_literal(
                    "security",
                    "WholeValueWithInherit",
                    "LastContaining",
                    0,
                    inherited='&["gates.deny_critical_commands"]',
                )
            ],
            "note",
        ),
        (
            "an inherit declaration with no inherited path fails",
            baseline[:3]
            + [
                _strategy_literal(
                    "security", "WholeValueWithInherit", "LastContaining", 0, note="x"
                )
            ],
            "inherited",
        ),
        (
            "an inherited path the arm never mentions fails",
            baseline[:3]
            + [
                _strategy_literal(
                    "security",
                    "WholeValueWithInherit",
                    "LastContaining",
                    0,
                    inherited='&["gates.no_such_key"]',
                    note="x",
                )
            ],
            "no_such_key",
        ),
        (
            "an inherited path on a plain whole-value entry fails",
            baseline[:3]
            + [
                _strategy_literal(
                    "security",
                    "WholeValue",
                    "LastContaining",
                    0,
                    inherited='&["gates.deny_critical_commands"]',
                )
            ],
            "inherited",
        ),
    ]

    failures = 0
    for description, entries, expected in cases:
        try:
            problems = strategy_problems(
                _STRATEGY_SCHEMA, _STRATEGY_PATHS, _strategy_source(entries)
            )
        except ValueError as exc:
            problems = [f"<parse error: {exc}>"]
        if expected is None:
            ok = not problems
            detail = "" if ok else f" -> {problems}"
        else:
            ok = any(expected in problem for problem in problems)
            detail = "" if ok else f" -> {problems}"
        print(f"  [{'PASS' if ok else 'FAIL'}] self-test: {description}{detail}")
        if not ok:
            failures += 1

    # The real tree's table must be complete and consistent.
    try:
        real = strategy_problems(
            _read(SCHEMA_MODULE), _read(PATHS_MODULE), _read(PROVENANCE_MODULE)
        )
    except ValueError as exc:
        real = [f"<parse error: {exc}>"]
    real_ok = not real
    print(
        f"  [{'PASS' if real_ok else 'FAIL'}] self-test: real tree's strategy table "
        f"matches its merge{'' if real_ok else f' -> {real}'}"
    )
    if not real_ok:
        failures += 1
    return failures


def check_strategy_table() -> bool:
    """Every `Config` section must declare the merge it actually gets."""
    if not PROVENANCE_MODULE.is_file():
        print(f"  FAIL: {PROVENANCE_MODULE.relative_to(REPO_ROOT)} is missing")
        return False
    try:
        problems = strategy_problems(
            _read(SCHEMA_MODULE), _read(PATHS_MODULE), _read(PROVENANCE_MODULE)
        )
    except ValueError as exc:
        print(f"  FAIL: {exc}")
        return False
    if problems:
        print(f"  FAIL: {len(problems)} strategy-table disagreement(s):")
        for problem in problems:
            print(f"        - {problem}")
        print(
            "  The provenance engine computes a key's winning layer from this "
            "table, so a declaration that disagrees with the merge attributes "
            "keys to the wrong layer."
        )
        return False
    return True


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

    failures += _strategy_self_test()

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
    return (
        SCHEMA_MODULE.is_file()
        and PATHS_MODULE.is_file()
        and PROVENANCE_MODULE.is_file()
    )


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
        (
            "every Config section declares the merge strategy it gets",
            check_strategy_table,
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
