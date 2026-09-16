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
    for pattern, what in [
        (r"sandbox_profile\s*:\s*Option<.*SandboxProfile", "SessionToolContext.sandbox_profile"),
        (r"sandbox_profile,?\s*\n\s*\}\);", "factory passes sandbox_profile to ToolRegistryOptions"),
    ]:
        err = check_contains(factory_rs, pattern, what)
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
