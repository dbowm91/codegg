#!/usr/bin/env python3
"""Fail if a shipped model catalog is compiled in.

Product contract: CodeGG ships no per-provider model configuration.
Model discovery is the canonical way a model becomes usable and is
always attempted; operator-declared config models are additive on top
of what discovery returns.

Why this is enforced rather than documented: a compiled-in catalog
silently replaces a failed discovery, so the operator is shown models
the upstream provider does not actually offer, and one of them gets
adopted as the session's model. That is exactly how a stale placeholder
(`opencode_zen/big-pickle`) ended up persisted into a user's tab
manifest.

What is forbidden:
  * a module whose purpose is an embedded/static model list
  * a literal `vec![ModelInfo { .. }]` returned from a provider's
    `models()` / `discover_models()` — discovery must actually run
  * a frontend constructor seeding a non-empty model list

What is allowed:
  * `vec![ModelInfo { .. }]` inside `#[cfg(test)]` modules
  * `ModelInfo` values synthesized from a real discovery response
  * config-declared models threaded through as additive seeds

Every file is scanned through `rust_test_scan.production_view`, never through
a regex over raw source: a `#[cfg(test)]` module is removed by the scanner's
literal-aware brace matching, so a fixture's `{` cannot decide where the
module ends.
"""

from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(Path(__file__).resolve().parent))

import rust_test_scan  # noqa: E402  (path is set above)

PROVIDERS = ROOT / "crates" / "codegg-providers" / "src"
TUI = ROOT / "src" / "tui"

# Modules whose entire reason for existing is a shipped model list.
FORBIDDEN_MODULES = (
    "models.rs",
    "embedded_models.rs",
)

# Names that mean "here is a built-in catalog".
FORBIDDEN_FNS = (
    "embedded_models",
    "seed_embedded",
    "builtin_models",
    "static_models",
    "xai_models",
    "seed_models",
    "default_models",
)

# A local seed list built from ModelInfo literals and handed to a
# provider constructor. These were not caught by the `models()` scan
# because they live in a `let models = vec![...]` local, not in the
# impl body that returns them — which is exactly how a compiled-in
# catalog hides from a shallow check.
SEED_LIST_PATTERNS = (
    (r"let\s+(?:mut\s+)?models(?:\s*:\s*Vec<ModelInfo>)?\s*=\s*vec!\s*\[\s*ModelInfo", "seeded ModelInfo list"),
    (
        r"models\s*:\s*vec!\s*\[\s*ModelInfo",
        "config-seeded ModelInfo list",
    ),
    (
        r"fn\s+\w*models\w*\s*\(\s*\)\s*->\s*Vec<ModelInfo>\s*\{[^}]*vec!\s*\[\s*ModelInfo",
        "function returning a compiled-in ModelInfo catalog",
    ),
)

# Frontend constructors that must not seed a model list.
TUI_MODEL_SEEDS = (
    (r'current_model\s*=\s*models\[0\]\.clone\(\)', "current_model must not default to models[0]"),
    (
        r'let\s+models\s*(?::\s*Vec<String>)?\s*=\s*vec!\s*\[',
        "TUI constructor must not seed a literal model list",
    ),
    (
        r'let\s+current_model\s*=\s*models\[0\]',
        "current_model must not be derived from a seeded list",
    ),
)


def strip_test_modules(src: str) -> str:
    """The file's production view: its `#[cfg(test)]` modules removed.

    Test fixtures legitimately construct `ModelInfo` literals; this is the
    only place we are allowed to. The view is `rust_test_scan.production_view`
    rather than a hand-rolled brace counter.

    The counter this replaced treated *any* `#[cfg(test)]` as a module gate
    and blanked forward to the next `{`, so a `#[cfg(test)] use` erased the
    production item that followed it. Measured on this tree, before the fix:
    44 production lines erased in `crates/codegg-providers/src/opencode_go.rs`
    (the `#[cfg(test)] use std::time::Duration;` at line 30 made the next `{`
    the provider struct's), and 67 production lines across 4 files. A
    compiled-in catalogue declared in one of those regions would have been
    invisible to this guard.
    """
    return rust_test_scan.production_view(src)


def scan_models_impls(src: str) -> list[str]:
    """Find `models()` impls whose body is a literal ModelInfo list."""
    hits = []
    for m in re.finditer(r"async fn (?:discover_)?models\s*\(", src):
        brace = src.find("{", m.end())
        if brace == -1:
            continue
        depth = 0
        j = brace
        while j < len(src):
            if src[j] == "{":
                depth += 1
            elif src[j] == "}":
                depth -= 1
                if depth == 0:
                    break
            j += 1
        body = src[brace : j + 1]
        # A discovery call means the provider is really asking upstream.
        if "discover" in body or "models_endpoint" in body or "probe" in body:
            continue
        if re.search(r"vec!\s*\[\s*ModelInfo", body):
            hits.append(
                f"{m.group(0)} returns a literal ModelInfo list instead of discovering"
            )
    return hits


def check_provider_source(rel: str, src: str, filename: str = "") -> list[str]:
    """The provider-side rules, over one file's production view."""
    failures: list[str] = []
    for fn in FORBIDDEN_FNS:
        if re.search(rf"fn\s+{fn}\s*\(", src):
            failures.append(f"{rel}: defines {fn}(); shipped model catalogs are forbidden")
    if filename in FORBIDDEN_MODULES:
        return failures
    for hit in scan_models_impls(src):
        failures.append(f"{rel}: {hit}")
    for pattern, what in SEED_LIST_PATTERNS:
        if re.search(pattern, src, re.DOTALL):
            failures.append(f"{rel}: {what} (use discovery or additive config instead)")
    return failures


def check_tui_source(rel: str, src: str) -> list[str]:
    """The frontend rules, over one file's production view."""
    failures: list[str] = []
    for pattern, msg in TUI_MODEL_SEEDS:
        if re.search(pattern, src):
            failures.append(f"{rel}: {msg}")
    return failures


def main() -> int:
    failures: list[str] = []

    for name in FORBIDDEN_MODULES:
        if (PROVIDERS / name).exists():
            failures.append(
                f"crates/codegg-providers/src/{name} must not exist: "
                "CodeGG ships no compiled-in model catalog"
            )

    for rs in sorted(PROVIDERS.rglob("*.rs")):
        rel = rs.relative_to(ROOT)
        src = strip_test_modules(rs.read_text())
        failures.extend(check_provider_source(rel, src, rs.name))

    for rs in sorted(TUI.rglob("*.rs")):
        rel = rs.relative_to(ROOT)
        src = strip_test_modules(rs.read_text())
        failures.extend(check_tui_source(rel, src))

    if failures:
        sys.stderr.write(
            "hardcoded model catalog detected:\n  "
            + "\n  ".join(failures)
            + "\n\nModel discovery is canonical; config models are additive. "
            "See architecture/provider.md.\n"
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())