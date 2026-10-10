#!/usr/bin/env python3
"""Static guard: production sandbox policy wiring (M005).

M005 makes the resolved SandboxProfile a production execution property.
This guard ensures the wiring is not silently dropped:

1. ToolRegistryOptions carries a sandbox profile and with_options() maps it
   into BashTool Landlock configuration via sandbox_config_for_profile
   (constrained profiles must not use a bare BashTool::default() without
   applying the policy).
2. SessionToolContext threads the profile and turn_runtime passes it.
3. Child construction enforces the parent ceiling (resolve_child_sandbox)
   and threads the narrowed profile into the child registry/loop.
4. Legacy DangerFullAccess is compat-only: no new production construction
   outside src/security/sandbox.rs.

Run:
  python3 scripts/check_sandbox_policy_wiring.py
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def fail(msg: str) -> int:
    print(f"sandbox-policy-wiring guard failed: {msg}", file=sys.stderr)
    return 1


def check_contains(path: Path, pattern: str, what: str) -> str | None:
    try:
        text = path.read_text()
    except Exception as e:
        return f"cannot read {path.relative_to(ROOT)}: {e}"
    if not re.search(pattern, text, re.DOTALL):
        return f"{path.relative_to(ROOT)}: missing {what}"
    return None


def literal_body(text: str, opener: str) -> str | None:
    """Return the body of the first `{ ... }` literal opened by `opener`.

    `opener` must be a declaration-ish fragment (identifier followed by
    `{`), so a plain substring search cannot latch onto an import such
    as `use crate::tool::{ToolRegistry, ToolRegistryOptions};`.

    Balanced-brace aware, so the caller can assert a field is present
    anywhere inside a struct literal instead of depending on the field
    being the last one. A positional regex silently started failing the
    moment an unrelated field was appended after the one under test.
    """
    for m in re.finditer(rf"{re.escape(opener)}\s*\{{", text):
        start = text.index("{", m.start())
        depth = 0
        for i in range(start, len(text)):
            ch = text[i]
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    return text[start : i + 1]
    return None


def check_factory_passes_profile(factory_rs: Path) -> str | None:
    """`ToolRegistry::with_options(ToolRegistryOptions { .. })` must set sandbox_profile."""
    try:
        text = factory_rs.read_text()
    except Exception as e:
        return f"cannot read {factory_rs.relative_to(ROOT)}: {e}"
    body = literal_body(text, "ToolRegistryOptions")
    if body is None:
        return f"{factory_rs.relative_to(ROOT)}: no ToolRegistryOptions literal found"
    if not re.search(r"(?<![\w.])sandbox_profile\s*(?:,|:)", body):
        return (
            f"{factory_rs.relative_to(ROOT)}: factory does not pass sandbox_profile "
            "to ToolRegistryOptions"
        )
    return None


def check_backend_registry() -> list[str]:
    """Containment must stay backend-neutral, and stay additively extensible.

    The failure this guards against is silent re-hardcoding: someone resolves
    capability from a platform branch or calls one backend directly, and the
    next OS loses containment without anything looking broken.
    """
    errors: list[str] = []
    sandbox_dir = ROOT / "src/security/sandbox"

    registry = sandbox_dir / "backend.rs"
    for pattern, what in [
        (r"pub const BACKENDS:\s*&\[SandboxBackend\]", "backend registry table"),
        (r"pub struct SandboxBackend\b", "SandboxBackend entry type"),
        (r"pub fn select\(\)\s*->\s*Result<&'static SandboxBackend",
         "preference-ordered backend selection"),
        (r"pub probe:", "backend availability probe"),
        (r"pub apply:|pub apply_profile:", "backend enforcement entry point"),
    ]:
        err = check_contains(registry, pattern, what)
        if err:
            errors.append(err)

    # Both shipped backends must be registered, or one platform silently
    # degrades to uncontained.
    if not re.search(r"BackendId::LANDLOCK", registry.read_text(), re.DOTALL):
        errors.append("backend.rs: Landlock backend is not registered")
    if not re.search(r"BackendId::SEATBELT", registry.read_text(), re.DOTALL):
        errors.append("backend.rs: Seatbelt backend is not registered")

    # Policy must be backend-neutral: paths in, nothing platform-specific.
    policy = sandbox_dir / "policy.rs"
    err = check_contains(
        policy,
        r"pub fn tool_read_allowances\(\)\s*->\s*Vec<PathBuf>",
        "tool-compatible read allowances",
    )
    if err:
        errors.append(err)
    err = check_contains(
        policy,
        r"pub fn tool_write_allowances\(\)\s*->\s*Vec<PathBuf>",
        "tool-compatible write allowances",
    )
    if err:
        errors.append(err)
    err = check_contains(
        policy, r"pub fn sensitive_deny_paths\(\)", "sensitive deny list"
    )
    if err:
        errors.append(err)

    # Capability must come from the registry, never from a platform branch.
    sandbox_rs = ROOT / "src/security/sandbox.rs"
    err = check_contains(
        sandbox_rs,
        r"pub fn platform_sandbox_capability\(\)[\s\S]{0,400}?backend::select\(\)",
        "capability resolved through the backend registry",
    )
    if err:
        errors.append(err)
    if re.search(
        r"fn platform_sandbox_capability\(\)\s*->\s*SandboxCapability\s*\{[^}]*"
        r'cfg\(target_os\s*=\s*"linux"\)',
        sandbox_rs.read_text(),
        re.DOTALL,
    ):
        errors.append(
            "src/security/sandbox.rs: platform_sandbox_capability branches on the "
            "target OS instead of consulting the backend registry"
        )

    # The helper must dispatch through the registry, not one backend.
    helper = ROOT / "src/bin/codegg-sandbox-helper.rs"
    err = check_contains(helper, r"apply_backend\(&spec\)", "helper backend dispatch")
    if err:
        errors.append(err)
    if re.search(r"apply_landlock\(&spec\)", helper.read_text()):
        errors.append(
            "src/bin/codegg-sandbox-helper.rs: helper applies one backend directly; "
            "it must dispatch through the registry"
        )

    # The enforced status must carry the backend that produced it.
    err = check_contains(
        sandbox_rs,
        r"Enforced\s*\{\s*backend:\s*BackendId",
        "Enforced outcome carries backend identity",
    )
    if err:
        errors.append(err)

    # End-to-end mechanism test must exist and be platform-neutral.
    containment = ROOT / "tests/sandbox_containment.rs"
    if not containment.exists():
        errors.append(
            "tests/sandbox_containment.rs: missing end-to-end containment test; "
            "a new backend must inherit the enforcement matrix"
        )
    return errors


def main() -> int:
    errors: list[str] = []

    mod_rs = ROOT / "src/tool/mod.rs"
    for pattern, what in [
        (r"sandbox_profile\s*:\s*Option<.*SandboxProfile", "ToolRegistryOptions.sandbox_profile"),
        (r"sandbox_config_for_profile", "with_options sandbox_config_for_profile mapping"),
        (r"with_landlock_sandbox_custom", "with_options BashTool Landlock application"),
        (r"fn sandbox_profile", "ToolRegistry::sandbox_profile accessor"),
        (r"FullHost.*without CodeGG filesystem containment|FullHost.*explicit",
         "FullHost explicit no-containment comment"),
    ]:
        err = check_contains(mod_rs, pattern, what)
        if err:
            errors.append(err)

    factory_rs = ROOT / "src/tool/factory.rs"
    err = check_contains(
        factory_rs,
        r"sandbox_profile\s*:\s*Option<.*SandboxProfile",
        "SessionToolContext.sandbox_profile",
    )
    if err:
        errors.append(err)
    err = check_factory_passes_profile(factory_rs)
    if err:
        errors.append(err)

    turn_rs = ROOT / "src/agent/turn_runtime.rs"
    err = check_contains(turn_rs, r"sandbox_profile", "turn_runtime threads sandbox_profile")
    if err:
        errors.append(err)

    worker_rs = ROOT / "src/agent/worker.rs"
    for pattern, what in [
        (r"parent_sandbox_profile", "SubAgentRequest parent ceiling"),
        (r"resolve_child_sandbox", "worker child ceiling enforcement"),
        (r"sandbox_profile:\s*Some\(effective_child\)", "child registry narrowed profile"),
        (r"set_sandbox_profile\(effective_child\)", "child loop narrowed profile"),
    ]:
        err = check_contains(worker_rs, pattern, what)
        if err:
            errors.append(err)

    # Legacy DangerFullAccess must not be constructed outside the compat
    # owner (sandbox.rs), tests, docs, and architecture notes.
    for rust_file in sorted((ROOT / "src").rglob("*.rs")):
        rel = str(rust_file.relative_to(ROOT))
        if rel == "src/security/sandbox.rs":
            continue
        try:
            text = rust_file.read_text()
        except Exception:
            continue
        if "DangerFullAccess" in text and "#[cfg(test)]" not in text:
            # Allow comments/docs mentioning the name, but not construction.
            for idx, line in enumerate(text.splitlines(), start=1):
                s = line.strip()
                if s.startswith("//") or s.startswith("///") or s.startswith("//!"):
                    continue
                if "DangerFullAccess" in line:
                    errors.append(f"{rel}:{idx}: new DangerFullAccess use outside sandbox.rs compat")
                    break

    # BashTool must expose the M005 helpers.
    bash_rs = ROOT / "src/tool/bash.rs"
    for pattern, what in [
        (r"with_sandbox_profile", "BashTool::with_sandbox_profile"),
        (r"sandbox_enforcement", "BashTool::sandbox_enforcement"),
        (r"has_landlock_config", "BashTool::has_landlock_config"),
    ]:
        err = check_contains(bash_rs, pattern, what)
        if err:
            errors.append(err)

    # Core enforcement types must exist.
    approval_rs = ROOT / "crates/codegg-core/src/approval.rs"
    for pattern, what in [
        (r"enum FilesystemEnforcement", "FilesystemEnforcement"),
        (r"enum NetworkEnforcement", "NetworkEnforcement"),
        (r"struct SandboxEnforcement", "SandboxEnforcement"),
        (r"struct SandboxEscalationRequest", "SandboxEscalationRequest"),
        (r"fn resolve_child_sandbox", "resolve_child_sandbox"),
    ]:
        err = check_contains(approval_rs, pattern, what)
        if err:
            errors.append(err)

    errors.extend(check_backend_registry())

    if errors:
        print("sandbox-policy-wiring guard failed:", file=sys.stderr)
        for e in errors:
            print(f"  {e}", file=sys.stderr)
        print(
            "\nProduction ToolRegistry must receive the resolved SandboxProfile and "
            "WorkspaceWrite construction must apply it via sandbox_config_for_profile, "
            "never a bare BashTool::default().",
            file=sys.stderr,
        )
        return 1
    print("sandbox-policy-wiring guard passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
