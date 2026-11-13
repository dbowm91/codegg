#!/usr/bin/env python3
"""Guard: the TUI render-invariant baseline is a ratchet, not a snapshot.

WHAT THIS GUARD ENFORCES
    `docs/tui-render-invariant-baseline.toml` records the failures of the
    three presentation invariants that cannot pass yet — I4 (every advertised
    key is handled), I5 (every rendered foreground clears
    `MIN_DERIVED_TEXT_RATIO`) and I6 (a hint line never loses its exit key).
    The record may shrink and may not grow.

    R1  shape — the file parses as TOML; `[totals]` carries the three counts;
        every entry carries exactly the fields its section defines, non-empty,
        and no section holds the same entry twice. An `[advertised_keys]` entry
        carries a `reason` as well: the recorded I4 set is the residue of a
        decision, so a row without a justification is an unexplained exemption
        rather than a measurement.
    R2  ratchet — each recorded count equals the number of entries in that
        section. Both directions are errors: a count below the entry list is a
        growth nobody banked, and a count above it is a reduction nobody
        banked.
    R3  surface — every named surface still exists in the source: a
        `DialogType` variant, a `SidePanelId` variant (`panel:<id>`), or a
        `WxH` frame size (`app:<w>x<h>`). A renamed or removed surface is a
        failure, never a silently stale row.
    R4  measured surface — an `[advertised_keys]` entry must name a surface the
        harness builds and a key spelling the harness can parse. I4 measures
        dialogs, so an advertisement recorded against a `panel:` surface is
        not measured at all.
    R5  reproduction — every `[contrast]` pair must still fail the bar. The
        guard recomputes the WCAG ratio from the two recorded colours and
        compares it against the bar read out of `src/theme/schema.rs`, so a
        pair that now passes is reported as an entry that no longer
        reproduces rather than kept as a harmless leftover.
    R6  producer — a `[hint_truncation]` entry must name a producer the
        harness measures, at a budget it measures, with the exit key it looks
        for. All three are read out of the harness, not restated here.

WHAT THIS GUARD CANNOT DO, AND WHICH LAYER CAN
    A static read cannot render a frame, so it cannot tell that an advertised
    key still does nothing or that a hint row still drops its exit key. That
    is the harness's job: `cargo test --test tui_render -- invariants`
    recomputes all three sets and requires them to equal this file entry for
    entry, in both directions. The two layers are deliberately different:
    the harness is the measurement, and this guard is the cheap gate that runs
    in `quick` and CI and fails on the shape, the totals, the inventory and
    the arithmetic.

WHY BOTH LANES
    AGENTS.md records that CI is a strict subset of `quick`. This guard runs
    in **both**, because a baseline that only one lane checks is a baseline
    the other lane can grow.

USAGE
    python3 scripts/check_tui_render_invariant_baseline.py             # check
    python3 scripts/check_tui_render_invariant_baseline.py --self-test  # prove it bites
"""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST_REL = "docs/tui-render-invariant-baseline.toml"
DIALOG_SOURCE = "src/tui/components/component.rs"
PANEL_SOURCE = "src/tui/panels/mod.rs"
HARNESS_SOURCE = "tests/tui_render/invariants/baseline.rs"
FIXTURE_SOURCE = "tests/tui_render/invariants/fixtures.rs"
SCHEMA_SOURCE = "src/theme/schema.rs"
REGISTRY_SOURCE = "src/theme/registry.rs"

SECTIONS = ("advertised_keys", "contrast", "hint_truncation")

SECTION_FIELDS = {
    "advertised_keys": ("surface", "key", "reason"),
    "contrast": ("surface", "theme", "fg", "bg"),
    "hint_truncation": ("producer", "budget", "exit_key"),
}

# The fields that identify an entry, so the "held twice" check compares the
# entry rather than its prose. `reason` is justification, not identity: two
# rows cannot name the same `(surface, key)` however differently they explain
# it.
SECTION_IDENTITY = {
    "advertised_keys": ("surface", "key"),
    "contrast": ("surface", "theme", "fg", "bg"),
    "hint_truncation": ("producer", "budget", "exit_key"),
}

HEX_RE = re.compile(r"^#[0-9a-f]{6}$")
APP_SURFACE_RE = re.compile(r"^app:(\d+)x(\d+)$")

# The key spellings the harness's parser understands. Mirrored here only so the
# guard can reject a spelling the harness would silently skip; the harness
# remains the authority on what it measures.
NAMED_KEYS = {"Esc", "esc", "Enter", "enter", "Tab", "tab", "space", "Space"}
ARROW_KEYS = {"→", "←", "↑", "↓"}
CHORD_RE = re.compile(r"^(?:Shift\+|⇧|\^)([A-Za-z0-9])$")
BARE_RE = re.compile(r"^[A-Za-z0-9]$")


@dataclass
class Violation:
    rule: int
    message: str


@dataclass
class Model:
    """Everything the rules read, so `analyse` can run on a synthetic tree."""

    manifest: dict
    dialog_surfaces: set[str]
    panel_surfaces: set[str]
    fixture_surfaces: set[str]
    contrast_bar: float
    bundled_themes: set[str]
    hint_producers: list[str]
    exit_key: str
    hint_budgets: list[int]
    unreadable_spellings: list[str]
    parse_error: str | None = None
    extra: dict = field(default_factory=dict)


# ── Reading the tree ────────────────────────────────────────────────


def _read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def enum_variants(text: str, name: str) -> set[str]:
    """The unit variants of `pub enum <name>` in `text`."""
    match = re.search(rf"pub enum {re.escape(name)}\s*\{{", text)
    if not match:
        raise LookupError(f"no `pub enum {name}` in the source")
    body = text[match.end() :]
    variants: set[str] = set()
    for line in body.splitlines():
        if line.startswith("}"):
            break
        hit = re.match(r"^    ([A-Z][A-Za-z0-9]*),$", line)
        if hit:
            variants.add(hit.group(1))
    return variants


def quoted_const(text: str, name: str) -> list[str]:
    match = re.search(rf"pub const {re.escape(name)}: &\[&str\] = &\[(.*?)\];", text, re.S)
    if not match:
        raise LookupError(f"no `pub const {name}` in the harness")
    return re.findall(r'"([^"]*)"', match.group(1))


def one_quoted_const(text: str, name: str) -> str:
    match = re.search(rf'pub const {re.escape(name)}: &str = "([^"]*)";', text)
    if not match:
        raise LookupError(f"no `pub const {name}` in the harness")
    return match.group(1)


def usize_const(text: str, name: str) -> list[int]:
    match = re.search(rf"pub const {re.escape(name)}: &\[usize\] = &\[(.*?)\];", text, re.S)
    if not match:
        raise LookupError(f"no `pub const {name}` in the harness")
    return [int(value) for value in re.findall(r"\d+", match.group(1))]


def contrast_bar() -> float:
    match = re.search(r"pub const MIN_DERIVED_TEXT_RATIO: f64 = ([0-9.]+);", _read(SCHEMA_SOURCE))
    if not match:
        raise LookupError("no `MIN_DERIVED_TEXT_RATIO` in the theme schema")
    return float(match.group(1))


def bundled_themes() -> set[str]:
    return set(re.findall(r'id: "([^"]+)"', _read(REGISTRY_SOURCE)))


def key_is_parseable(spelling: str) -> bool:
    return (
        spelling in NAMED_KEYS
        or spelling in ARROW_KEYS
        or bool(CHORD_RE.match(spelling))
        or bool(BARE_RE.match(spelling))
    )


def build_model() -> Model:
    manifest_path = ROOT / MANIFEST_REL
    parse_error = None
    manifest: dict = {}
    try:
        manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
    except OSError as error:
        parse_error = f"{MANIFEST_REL} could not be read: {error}"
    except tomllib.TOMLDecodeError as error:
        parse_error = f"{MANIFEST_REL} does not parse: {error}"

    harness = _read(HARNESS_SOURCE)
    return Model(
        manifest=manifest,
        dialog_surfaces=enum_variants(_read(DIALOG_SOURCE), "DialogType"),
        panel_surfaces=enum_variants(_read(PANEL_SOURCE), "SidePanelId"),
        fixture_surfaces=set(re.findall(r'surface: "([^"]+)"', _read(FIXTURE_SOURCE))),
        contrast_bar=contrast_bar(),
        bundled_themes=bundled_themes(),
        hint_producers=quoted_const(harness, "HINT_PRODUCERS"),
        exit_key=one_quoted_const(harness, "EXIT_KEY"),
        hint_budgets=usize_const(harness, "HINT_BUDGETS"),
        unreadable_spellings=quoted_const(harness, "UNREADABLE_SPELLINGS"),
        parse_error=parse_error,
    )


# ── The rules ───────────────────────────────────────────────────────


def _channel(value: int) -> float:
    scaled = value / 255.0
    if scaled <= 0.03928:
        return scaled / 12.92
    return ((scaled + 0.055) / 1.055) ** 2.4


def luminance(hex_colour: str) -> float:
    red = int(hex_colour[1:3], 16)
    green = int(hex_colour[3:5], 16)
    blue = int(hex_colour[5:7], 16)
    return 0.2126 * _channel(red) + 0.7152 * _channel(green) + 0.0722 * _channel(blue)


def contrast_ratio(one: str, other: str) -> float:
    """WCAG ratio, the same arithmetic `Rgb::contrast_ratio` performs."""
    first, second = luminance(one), luminance(other)
    lighter, darker = max(first, second), min(first, second)
    return (lighter + 0.05) / (darker + 0.05)


def _entries(manifest: dict, section: str) -> list[dict]:
    entries = manifest.get(section, [])
    return entries if isinstance(entries, list) else []


def _surface_resolves(model: Model, surface: str) -> bool:
    if APP_SURFACE_RE.match(surface):
        return True
    if surface.startswith("panel:"):
        return surface[len("panel:") :] in model.panel_surfaces
    return surface in model.dialog_surfaces


def rule_shape(model: Model, violations: list[Violation]) -> None:
    totals = model.manifest.get("totals")
    if not isinstance(totals, dict):
        violations.append(Violation(1, f"`[totals]` is missing from {MANIFEST_REL}"))
        return
    for section in SECTIONS:
        if not isinstance(totals.get(section), int):
            violations.append(Violation(1, f"`[totals].{section}` is not an integer"))
        declared_total = totals.get(section)
        present_section = model.manifest.get(section)
        if present_section is None and declared_total == 0:
            # TOML cannot express an empty array of tables, so a section whose
            # banked total is zero legitimately has no table at all. Rule 2
            # still holds it to that zero, so an absent section cannot hide an
            # entry.
            continue
        if not isinstance(present_section, list):
            violations.append(Violation(1, f"`{section}` is not an array of tables"))
            continue
        seen: set[tuple] = set()
        for index, entry in enumerate(_entries(model.manifest, section)):
            if not isinstance(entry, dict):
                violations.append(Violation(1, f"{section}[{index}] is not a table"))
                continue
            present = set(entry)
            expected = set(SECTION_FIELDS[section])
            if present != expected:
                missing = sorted(expected - present)
                extra = sorted(present - expected)
                violations.append(
                    Violation(
                        1,
                        f"{section}[{index}] carries {sorted(present)}; missing {missing}, "
                        f"unexpected {extra}",
                    )
                )
                continue
            if any(not str(entry[field]).strip() for field in expected):
                violations.append(Violation(1, f"{section}[{index}] has an empty field"))
                continue
            key = tuple(entry[field] for field in SECTION_IDENTITY[section])
            if key in seen:
                violations.append(Violation(1, f"{section} holds {key} twice"))
            seen.add(key)


def rule_totals(model: Model, violations: list[Violation]) -> None:
    totals = model.manifest.get("totals")
    if not isinstance(totals, dict):
        return
    for section in SECTIONS:
        recorded = totals.get(section)
        if not isinstance(recorded, int):
            continue
        live = len(_entries(model.manifest, section))
        if recorded > live:
            violations.append(
                Violation(
                    2,
                    f"[totals].{section} banks {recorded} entries but {live} are recorded: "
                    f"the banked count names a failure the manifest no longer holds",
                )
            )
        elif recorded < live:
            violations.append(
                Violation(
                    2,
                    f"{live} {section} entries are recorded but [totals].{section} banks "
                    f"{recorded}: a growth that was not banked",
                )
            )


def rule_surface(model: Model, violations: list[Violation]) -> None:
    for section in SECTIONS:
        if section == "hint_truncation":
            continue
        for entry in _entries(model.manifest, section):
            if not isinstance(entry, dict):
                continue
            surface = entry.get("surface")
            if not isinstance(surface, str):
                continue
            if not _surface_resolves(model, surface):
                violations.append(
                    Violation(3, f"{section}: `{surface}` is not a surface the source defines")
                )


def rule_measured_surface(model: Model, violations: list[Violation]) -> None:
    for entry in _entries(model.manifest, "advertised_keys"):
        if not isinstance(entry, dict):
            continue
        surface = entry.get("surface")
        key = entry.get("key")
        if isinstance(surface, str) and surface not in model.fixture_surfaces:
            violations.append(
                Violation(
                    4,
                    f"advertised_keys: `{surface}` is not a surface the harness builds, "
                    f"so the entry is not measured",
                )
            )
        if isinstance(key, str) and not key_is_parseable(key):
            violations.append(
                Violation(
                    4,
                    f"advertised_keys: `{surface}` advertises `{key}`, which the "
                    f"harness's key parser cannot read",
                )
            )
        if isinstance(key, str) and key in model.unreadable_spellings:
            violations.append(
                Violation(
                    4,
                    f"advertised_keys: `{surface}` advertises `{key}`, which the "
                    f"harness documents as unreadable (`UNREADABLE_SPELLINGS`), so the "
                    f"entry cannot be reproduced",
                )
            )


def rule_reproduction(model: Model, violations: list[Violation]) -> None:
    for entry in _entries(model.manifest, "contrast"):
        if not isinstance(entry, dict):
            continue
        surface = entry.get("surface")
        theme = entry.get("theme")
        fg = entry.get("fg")
        bg = entry.get("bg")
        for field, value in (("fg", fg), ("bg", bg)):
            if isinstance(value, str) and not HEX_RE.match(value):
                violations.append(
                    Violation(1, f"contrast: `{surface}`/{theme} has {field} `{value}`, "
                                 f"which is not a lowercase #rrggbb colour")
                )
        if not all(isinstance(value, str) and HEX_RE.match(value) for value in (fg, bg)):
            continue
        if isinstance(theme, str) and theme not in model.bundled_themes:
            violations.append(
                Violation(
                    5,
                    f"contrast: `{theme}` is not a bundled theme, so the entry cannot be "
                    f"reproduced",
                )
            )
            continue
        ratio = contrast_ratio(fg, bg)
        if ratio >= model.contrast_bar:
            violations.append(
                Violation(
                    5,
                    f"contrast: `{surface}`/`{theme}` records {fg} on {bg}, which reads "
                    f"{ratio:.2f}:1 — at or above the {model.contrast_bar:.1f}:1 bar, so the "
                    f"entry no longer reproduces and must be removed",
                )
            )


def rule_producer(model: Model, violations: list[Violation]) -> None:
    for entry in _entries(model.manifest, "hint_truncation"):
        if not isinstance(entry, dict):
            continue
        producer = entry.get("producer")
        budget = entry.get("budget")
        exit_key = entry.get("exit_key")
        if isinstance(producer, str) and producer not in model.hint_producers:
            violations.append(
                Violation(
                    6,
                    f"hint_truncation: `{producer}` is not a producer the harness measures "
                    f"({', '.join(model.hint_producers)})",
                )
            )
        if isinstance(budget, int) and budget not in model.hint_budgets:
            violations.append(
                Violation(
                    6,
                    f"hint_truncation: `{producer}` records budget {budget}, which the harness "
                    f"does not measure ({model.hint_budgets})",
                )
            )
        if isinstance(exit_key, str) and exit_key != model.exit_key:
            violations.append(
                Violation(
                    6,
                    f"hint_truncation: `{producer}` records exit key `{exit_key}` but the "
                    f"harness looks for `{model.exit_key}`",
                )
            )


def analyse(model: Model) -> list[Violation]:
    if model.parse_error:
        return [Violation(1, model.parse_error)]
    violations: list[Violation] = []
    rule_shape(model, violations)
    rule_totals(model, violations)
    rule_surface(model, violations)
    rule_measured_surface(model, violations)
    rule_reproduction(model, violations)
    rule_producer(model, violations)
    return violations


# ── The self-test ───────────────────────────────────────────────────


def _advertised(surface: str, key: str) -> dict:
    return {"surface": surface, "key": key, "reason": "a recorded decision"}


def _clean_manifest() -> dict:
    return {
        "totals": {"advertised_keys": 1, "contrast": 1, "hint_truncation": 1},
        "advertised_keys": [_advertised("ShellShow", "i")],
        "contrast": [
            {"surface": "ShellShow", "theme": "acton", "fg": "#7f7f7f", "bg": "#8f8f8f"}
        ],
        "hint_truncation": [{"producer": "composer_insert", "budget": 5, "exit_key": "Esc"}],
    }


def _model(manifest: dict) -> Model:
    return Model(
        manifest=manifest,
        dialog_surfaces={"ShellShow", "Logs", "Cost"},
        panel_surfaces={"ShellShow", "Logs"},
        fixture_surfaces={"ShellShow", "Logs", "Cost"},
        contrast_bar=3.0,
        bundled_themes={"acton", "bam"},
        hint_producers=["composer_insert"],
        exit_key="Esc",
        hint_budgets=[0, 1, 5, 20, 80],
        unreadable_spellings=["Home", "End", "PageUp", "PageDown", "Backspace", "⌫", "[", "]"],
    )


def _counts(manifest: dict) -> dict:
    totals = manifest["totals"]
    for section in SECTIONS:
        totals[section] = len(manifest.get(section, []))
    return manifest


@dataclass
class Expectation:
    rules: tuple
    note: str


def _cases() -> list[tuple[str, Model, Expectation]]:
    clean = _clean_manifest()

    growth = _clean_manifest()
    growth["advertised_keys"].append(_advertised("Logs", "z"))

    shrink = _clean_manifest()
    shrink["advertised_keys"] = []

    stale = _clean_manifest()
    stale["contrast"] = [
        {"surface": "ShellShow", "theme": "acton", "fg": "#000000", "bg": "#ffffff"}
    ]

    duplicate = _clean_manifest()
    duplicate["advertised_keys"].append(_advertised("ShellShow", "i"))
    _counts(duplicate)

    no_reason = _clean_manifest()
    no_reason["advertised_keys"] = [{"surface": "ShellShow", "key": "i"}]
    _counts(no_reason)

    empty_reason = _clean_manifest()
    empty_reason["advertised_keys"] = [{"surface": "ShellShow", "key": "i", "reason": "  "}]
    _counts(empty_reason)

    unknown_surface = _clean_manifest()
    unknown_surface["advertised_keys"] = [_advertised("Removed", "i")]
    _counts(unknown_surface)

    panel_advertisement = _clean_manifest()
    panel_advertisement["advertised_keys"] = [_advertised("panel:ShellShow", "i")]
    _counts(panel_advertisement)

    unparseable_key = _clean_manifest()
    unparseable_key["advertised_keys"] = [_advertised("ShellShow", "F5")]
    _counts(unparseable_key)

    documented_unreadable = _clean_manifest()
    documented_unreadable["advertised_keys"] = [_advertised("ShellShow", "Home")]
    _counts(documented_unreadable)

    unknown_theme = _clean_manifest()
    unknown_theme["contrast"] = [
        {"surface": "ShellShow", "theme": "not-bundled", "fg": "#7f7f7f", "bg": "#8f8f8f"}
    ]
    _counts(unknown_theme)

    malformed_colour = _clean_manifest()
    malformed_colour["contrast"] = [
        {"surface": "ShellShow", "theme": "acton", "fg": "grey", "bg": "#8f8f8f"}
    ]
    _counts(malformed_colour)

    unknown_producer = _clean_manifest()
    unknown_producer["hint_truncation"] = [
        {"producer": "panel_hint", "budget": 5, "exit_key": "Esc"}
    ]
    _counts(unknown_producer)

    unmeasured_budget = _clean_manifest()
    unmeasured_budget["hint_truncation"] = [
        {"producer": "composer_insert", "budget": 7, "exit_key": "Esc"}
    ]
    _counts(unmeasured_budget)

    return [
        ("a clean manifest finds nothing", _model(clean), Expectation((), "the record matches")),
        (
            "an entry added without banking is rule 2",
            _model(growth),
            Expectation((2,), "the totals still say one entry"),
        ),
        (
            "an entry removed without banking is rule 2",
            _model(shrink),
            Expectation((2,), "the totals still say one entry"),
        ),
        (
            "an entry that no longer reproduces is rule 5",
            _model(stale),
            Expectation((5,), "#000000 on #ffffff reads 21:1"),
        ),
        (
            "the same entry twice is rule 1",
            _model(duplicate),
            Expectation((1,), "the section holds one key twice"),
        ),
        (
            "an advertisement without a reason is rule 1",
            _model(no_reason),
            Expectation((1,), "a row with no justification is an unexplained exemption"),
        ),
        (
            "an advertisement with a blank reason is rule 1",
            _model(empty_reason),
            Expectation((1,), "whitespace is not a reason"),
        ),
        (
            "a surface the source no longer defines is rules 3 and 4",
            _model(unknown_surface),
            Expectation((3, 4), "`Removed` is neither a DialogType nor a fixture"),
        ),
        (
            "an advertisement against a panel is rule 4",
            _model(panel_advertisement),
            Expectation((4,), "the panel exists, but I4 measures dialogs"),
        ),
        (
            "a key spelling the harness cannot parse is rule 4",
            _model(unparseable_key),
            Expectation((4,), "`F5` is not a spelling the parser reads"),
        ),
        (
            "a key spelling the harness documents as unreadable is rule 4",
            _model(documented_unreadable),
            Expectation((4,), "`Home` is in the harness's UNREADABLE_SPELLINGS"),
        ),
        (
            "a theme that is not bundled is rule 5",
            _model(unknown_theme),
            Expectation((5,), "the entry cannot be reproduced"),
        ),
        (
            "a colour that is not #rrggbb is rule 1",
            _model(malformed_colour),
            Expectation((1,), "`grey` is not a colour, so no ratio can be computed"),
        ),
        (
            "a producer the harness does not measure is rule 6",
            _model(unknown_producer),
            Expectation((6,), "only `composer_insert` is measured"),
        ),
        (
            "a budget the harness does not measure is rule 6",
            _model(unmeasured_budget),
            Expectation((6,), "budget 7 is not in the measured set"),
        ),
    ]


def run_self_test() -> int:
    failures = 0
    cases = _cases()
    for name, model, expected in cases:
        violations = analyse(model)
        rules = tuple(sorted({violation.rule for violation in violations}))
        if rules != tuple(sorted(expected.rules)):
            failures += 1
            print(
                f"self-test: {name}\n"
                f"  expected rules {sorted(expected.rules)}, got {rules}\n"
                + "\n".join(f"  {violation.message}" for violation in violations),
                file=sys.stderr,
            )
        else:
            print(f"self-test: {name} — {expected.note}")
    if failures:
        print(f"self-test: {failures} of {len(cases)} cases failed", file=sys.stderr)
        return 1
    print(f"self-test: {len(cases)} cases passed")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="run the synthetic cases instead of checking the tree",
    )
    args = parser.parse_args(argv)
    if args.self_test:
        return run_self_test()
    try:
        model = build_model()
    except (OSError, LookupError) as error:
        print(
            f"check_tui_render_invariant_baseline: cannot resolve the tree: {error}",
            file=sys.stderr,
        )
        return 2
    violations = analyse(model)
    if violations:
        for violation in violations:
            print(
                f"check_tui_render_invariant_baseline: rule {violation.rule}: "
                f"{violation.message}",
                file=sys.stderr,
            )
        print(
            f"check_tui_render_invariant_baseline: {len(violations)} violation(s)",
            file=sys.stderr,
        )
        return 1
    counts = model.manifest.get("totals", {})
    print(
        "check_tui_render_invariant_baseline: "
        f"{counts.get('advertised_keys', 0)} advertised-key, "
        f"{counts.get('contrast', 0)} contrast and "
        f"{counts.get('hint_truncation', 0)} hint-truncation entries recorded; "
        "every one reproduces and none was added unbanked"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
