#!/usr/bin/env python3
"""Static guard: the `dev` and `test` cargo profiles must resolve identically.

Cargo hashes the **resolved** profile into every compilation unit's identity,
so a difference between `[profile.dev]` and `[profile.test]` makes the same
crate compile twice — once for `cargo build` (dev) and once for `cargo test`
(test). The two profiles are separate *build modes* of the same source, so
the double compile buys nothing.

At the baseline `strip` was the only differing field (`dev` kept debuginfo,
`test` stripped it). The cost was a full second compile of the third-party
dependency graph *and* of the root library rlib in every verification
invocation: the hosted job's `cargo build --bin codegg-sandbox-helper` step
built the whole graph under `dev`, and `cargo nextest run` then rebuilt it
under `test`. The same double compile hit the local `cargo build` →
`cargo test` loop.

This guard resolves both profiles the way cargo does — `test` inherits `dev`,
then the manifest's own table is applied on top — and fails when any hashed
field, or any per-package override, differs. The two tables must be edited
together; a difference between them is a regression, not a cleanup.

Run with `--self-test` to prove the guard fails on an injected violation. A
guard that has never been shown to reject anything is a comment.

Exit code 0 if all checks pass, 1 if any fail.
"""

from __future__ import annotations

import sys
import tempfile

import tomllib

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST = REPO_ROOT / "Cargo.toml"

# Cargo's built-in defaults for the profiles this guard compares. `test`
# inherits `dev`, so cargo's own default `test` profile equals cargo's default
# `dev` profile; only the manifest can make them differ.
#
# `split-debuginfo` is left unset: its cargo default is platform-dependent,
# but it is the same platform default for both profiles, so "unset" on both
# sides compares equal. A manifest that sets it on one side only is caught.
PROFILE_DEFAULTS: dict[str, dict[str, object]] = {
    "dev": {
        "opt-level": 0,
        "debug": 2,
        "strip": "none",
        "debug-assertions": True,
        "overflow-checks": True,
        "codegen-units": 256,
        "incremental": True,
        "panic": "unwind",
        "rpath": False,
        "lto": False,
    },
    "test": {
        "opt-level": 0,
        "debug": 2,
        "strip": "none",
        "debug-assertions": True,
        "overflow-checks": True,
        "codegen-units": 256,
        "incremental": True,
        "panic": "unwind",
        "rpath": False,
        "lto": False,
    },
}

# The profile each profile inherits from, when it inherits at all.
PROFILE_PARENT = {"test": "dev"}

# The fields cargo hashes into a compilation unit's identity. A difference in
# any of them produces a distinct unit for the same source.
HASHED_FIELDS = (
    "opt-level",
    "debug",
    "strip",
    "debug-assertions",
    "overflow-checks",
    "codegen-units",
    "incremental",
    "panic",
    "rpath",
    "lto",
    "split-debuginfo",
)


def _read_manifest(path: Path) -> dict:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def _profile_table(manifest: dict, name: str) -> dict:
    return manifest.get("profile", {}).get(name, {}) or {}


def resolve_profile(manifest: dict, name: str) -> dict:
    """Resolve a profile the way cargo does: inherit, then apply the table."""
    resolved = dict(PROFILE_DEFAULTS[name])
    parent = PROFILE_PARENT.get(name)
    if parent:
        resolved.update(resolve_profile(manifest, parent))
    table = _profile_table(manifest, name)
    for key, value in table.items():
        if key in ("package", "inherits"):
            continue
        resolved[key] = value
    return resolved


def resolve_package_overrides(manifest: dict, name: str) -> dict[str, dict]:
    """Resolve the per-package profile overrides, applying the `*` fallback.

    Cargo looks a package up by its exact name first and then by the `*`
    wildcard, so a package named in one profile's table but absent from the
    other's still differs when the wildcard differs.
    """
    raw = _profile_table(manifest, name).get("package", {}) or {}
    wildcard = raw.get("*", {}) or {}
    resolved: dict[str, dict] = {}
    for key in list(raw.keys()) + ["*"]:
        if key == "*":
            continue
        resolved[key] = dict(raw.get(key, {}) or wildcard)
    resolved["*"] = dict(wildcard)
    return resolved


def _compare_profiles(manifest: dict) -> list[str]:
    violations: list[str] = []
    dev = resolve_profile(manifest, "dev")
    test = resolve_profile(manifest, "test")

    for field in HASHED_FIELDS:
        dev_value = dev.get(field)
        test_value = test.get(field)
        if dev_value != test_value:
            violations.append(
                f"profile.dev.{field} = {dev_value!r} but profile.test.{field} = "
                f"{test_value!r}. Cargo hashes the resolved profile into every unit's "
                f"identity, so this compiles the same crate twice — once for "
                f"`cargo build` and once for `cargo test`."
            )

    dev_packages = resolve_package_overrides(manifest, "dev")
    test_packages = resolve_package_overrides(manifest, "test")
    for package in sorted(set(dev_packages) | set(test_packages)):
        dev_override = dev_packages.get(package, {})
        test_override = test_packages.get(package, {})
        if dev_override != test_override:
            violations.append(
                f"per-package profile override for {package!r} differs: "
                f"profile.dev.package.{package} = {dev_override!r} but "
                f"profile.test.package.{package} = {test_override!r}. Dependencies "
                f"resolve to different units under the two profiles, so the "
                f"dependency graph is compiled twice."
            )

    return violations


def check_profile_alignment() -> bool:
    if not MANIFEST.is_file():
        print(f"  FAIL: workspace manifest not found at {MANIFEST}")
        return False
    try:
        manifest = _read_manifest(MANIFEST)
    except tomllib.TOMLDecodeError as error:
        print(f"  FAIL: {MANIFEST} is not valid TOML: {error}")
        return False
    violations = _compare_profiles(manifest)
    for message in violations:
        print(f"  FAIL: {message}")
    return not violations


def _self_test() -> int:
    """Prove the guard fails on injected violations."""
    original = MANIFEST.read_text(encoding="utf-8")
    baseline = MANIFEST.with_name("Cargo.toml.cargo-profile-alignment-self-test")
    try:
        baseline.write_text(original, encoding="utf-8")
    except OSError as error:
        print(f"  FAIL: self-test could not stage a manifest copy: {error}")
        return 1

    def rejects(text: str, label: str) -> bool:
        baseline.write_text(text, encoding="utf-8")
        try:
            manifest = _read_manifest(baseline)
        except tomllib.TOMLDecodeError:
            print(f"  ok: injected {label} rejected (manifest no longer parses)")
            return True
        violations = _compare_profiles(manifest)
        if violations:
            print(f"  ok: injected {label} rejected")
            return True
        print(
            f"  FAIL: self-test injected {label} and the guard still passed. "
            f"The guard is not load-bearing."
        )
        return False

    try:
        # Violation 1: the dev profile stops stripping, so the two profiles
        # differ again — the exact baseline regression.
        injected = original.replace(
            '[profile.dev]\ncodegen-units = 16\nstrip = "debuginfo"',
            "[profile.dev]\ncodegen-units = 16",
            1,
        )
        if injected == original:
            print("  FAIL: self-test could not find the dev profile to un-strip")
            return 1
        if not rejects(injected, "dev profile that no longer strips"):
            return 1

        # Violation 2: the test profile changes a different hashed field.
        injected = original.replace(
            "[profile.test]\nopt-level = 0\ncodegen-units = 16",
            "[profile.test]\nopt-level = 0\ncodegen-units = 32",
            1,
        )
        if injected == original:
            print("  FAIL: self-test could not find the test profile to change")
            return 1
        if not rejects(injected, "test profile with a different codegen-units"):
            return 1

        # Violation 3: a per-package override that exists on only one side.
        injected = original + '\n[profile.dev.package."*"]\nstrip = "none"\n'
        if not rejects(injected, "dev-only per-package profile override"):
            return 1

        # Violation 4: a malformed manifest must fail, not pass vacuously.
        if not rejects(original + "\n[profile.dev\nbroken = ", "malformed manifest"):
            return 1

        # And the unmodified tree must still pass, so the self-test proves the
        # guard rejects rather than being broken.
        baseline.write_text(original, encoding="utf-8")
        try:
            manifest = _read_manifest(baseline)
        except tomllib.TOMLDecodeError as error:
            print(f"  FAIL: self-test could not re-read the real manifest: {error}")
            return 1
        if _compare_profiles(manifest):
            print("  FAIL: self-test passed but the real tree has violations")
            return 1
        print("  ok: unmodified tree still passes")
        return 0
    finally:
        try:
            baseline.unlink()
        except OSError:
            pass


def main() -> int:
    if "--self-test" in sys.argv:
        print("check_cargo_profile_alignment --self-test")
        return _self_test()

    checks = [
        ("the dev and test cargo profiles resolve identically", check_profile_alignment),
    ]
    failed = []
    for name, check in checks:
        try:
            passed = check()
        except OSError as error:
            print(f"  FAIL: {name}: {error}")
            passed = False
        print(f"  {'ok' if passed else 'FAIL'}: {name}")
        if not passed:
            failed.append(name)
    if failed:
        print(f"check_cargo_profile_alignment: {len(failed)} failing")
        return 1
    print("check_cargo_profile_alignment: all checks pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
