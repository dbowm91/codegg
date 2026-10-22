#!/usr/bin/env python3
"""Static guard: production sandbox policy wiring and containment invariants.

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
5. Effective credential denies survive broad roots and writable re-allows,
   and managed launches use private scoped scratch instead of shared temp/cache
   grants.

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

    # Required constrained commands cannot be routed around the helper when
    # a capability probe fails. Python uses the same backend registry on macOS
    # and Linux; portable post-run checks are not an execution boundary.
    managed_process = ROOT / "src/managed_process.rs"
    if "DegradedUncontained" in managed_process.read_text():
        errors.append("managed_process.rs: degraded uncontained launch path is forbidden")
    bash_process = ROOT / "src/tool/bash/process.rs"
    if not re.search(r"SandboxExecutionPath::Unavailable\s*\{\s*reason\s*\}[^}]*return Err", bash_process.read_text(), re.DOTALL):
        errors.append("bash/process.rs: unavailable containment must refuse before launch")
    python_sandbox = ROOT / "src/python_script/sandbox.rs"
    python_text = python_sandbox.read_text()
    if re.search(r"#\[cfg\(target_os\s*=\s*\"linux\"\)\]\s*\{\s*let capability", python_text):
        errors.append("python_script/sandbox.rs: backend negotiation must not be Linux-only")
    if "SandboxBackend::Unavailable" not in python_text:
        errors.append("python_script/sandbox.rs: missing unavailable containment disposition")
    python_executor = ROOT / "src/python_script/executor.rs"
    if not re.search(
        r"sandbox_requirement\s*!=\s*super::types::SandboxRequirement::None"
        r"[\s\S]{0,180}!matches!\(policy\.enforcement_backend,\s*SandboxBackend::Os",
        python_executor.read_text(),
    ):
        errors.append("python_script/executor.rs: unavailable containment must refuse Python")

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
    policy_text = policy.read_text()
    if not all(token in policy_text for token in ("fn carve_denied", "read_roots,", "read_dir_roots,", "write_roots: Self::carve_denied")):
        errors.append("policy.rs: Landlock grants must be carved around denied descendants")
    if "fn normalized_denies" not in policy_text or "nearest existing canonical parent" not in policy_text:
        errors.append("policy.rs: absent deny paths must be preserved relative to an existing parent")
    if not re.search(r"pub fn tool_write_allowances\(\)[\s\S]*?\{\s*Vec::new\(\)", policy_text):
        errors.append("policy.rs: shared temp and global HOME caches must not be writable grants")

    seatbelt = (sandbox_dir / "seatbelt.rs").read_text()
    if not re.search(
        r"for path in &policy\.write_roots[\s\S]*?for path in &policy\.denied_roots",
        seatbelt,
    ):
        errors.append("seatbelt.rs: credential denies must follow writable-root re-allows")

    managed = managed_process.read_text()
    linux_launch = sandbox_dir / "linux_launch.rs"
    for pattern, what in [
        (r"pub struct LinuxLaunchRequest", "typed Linux launch request"),
        (r"parent_ceiling", "immutable parent ceiling"),
        (r"pub fn probe_linux_host_features", "read-only Linux host probe"),
        (r"pub struct LinuxObtainedEnforcement", "typed obtained enforcement facts"),
        (r"EnhancedUnavailable", "unavailable enhanced backend outcome"),
    ]:
        err = check_contains(linux_launch, pattern, what)
        if err:
            errors.append(err)
    if not re.search(
        r"ProductionLinuxLaunchAdapter[\s\S]{0,160}\.plan\(&launch_request\)[\s\S]{0,500}"
        r"LegacyPrivateHelper",
        managed,
    ):
        errors.append(
            "managed_process.rs: Linux launch contract must gate the canonical helper path"
        )
    if re.search(
        r"BackendId::(?:BUBBLEWRAP|NAMESPACE)|name:\s*\"(?:Bubblewrap|namespace)\"",
        (sandbox_dir / "backend.rs").read_text(),
    ):
        errors.append(
            "backend.rs: enhanced Linux backend cannot be registered before native proof"
        )
    if not re.search(
        r"tempdir\(\)[\s\S]{0,900}spec\.write_paths\.push\(scratch_path\.clone\(\)\)"
        r"[\s\S]{0,500}if let Some\(session_scratch\) = session_scratch\.as_ref\(\)"
        r"[\s\S]{0,180}spec\.write_paths\.push\(session_scratch\.clone\(\)\)",
        managed,
    ):
        errors.append(
            "managed_process.rs: Required launches must grant private staging and authenticated session scratch roots"
        )
    if not re.search(
        r"if let Some\(path\) = session_scratch\.as_ref\(\)[\s\S]{0,350}"
        r"with_var\(\"TMPDIR\", path\.as_os_str\(\)\)",
        managed,
    ) or not re.search(
        r"else \{[\s\S]{0,250}with_var\(\"TMPDIR\", scratch_path\.as_os_str\(\)\)",
        managed,
    ):
        errors.append(
            "managed_process.rs: model session TMPDIR must be trusted and other constrained launches must retain private scoped TMPDIR"
        )
    if not re.search(r"validate_denied_paths\(&spec\.deny_paths\)", managed):
        errors.append("managed_process.rs: hardlinked denied files must fail before launch")
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
    containment = ROOT / "tests/security_permission/sandbox_containment.inc"
    if not containment.exists():
        errors.append(
            "tests/security_permission/sandbox_containment.inc: missing end-to-end containment test; "
            "a new backend must inherit the enforcement matrix"
        )
    return errors


def check_network_intent_owner_wiring() -> list[str]:
    """The M005 network intent must reach every launch owner, and no owner may
    quietly drop it.

    The failure this guards against is a *silent* widening: a launch owner
    reads the principal's authenticated network intent, but the request that
    reaches the process owner carries a hardcoded unrestricted default, so an
    offline principal still gets host networking. Nothing looks broken; the
    principal simply is not contained.

    Every assertion below names a specific owner and the specific way that
    owner is allowed to honor or refuse the intent.
    """
    errors: list[str] = []

    # 1. The intent type and the monotonic ceiling live in core.
    approval_rs = ROOT / "crates/codegg-core/src/approval.rs"
    for pattern, what in [
        (r"enum NetworkIntent", "NetworkIntent"),
        (r"OfflineRequired", "NetworkIntent::OfflineRequired"),
        (r"fn with_network_intent", "ExecutionPolicySnapshot::with_network_intent"),
    ]:
        err = check_contains(approval_rs, pattern, what)
        if err:
            errors.append(err)

    # 2. The process owner reads the intent from the authorization snapshot
    #    (the ceiling), and refuses rather than falling back.
    managed_rs = ROOT / "src/managed_process.rs"
    for pattern, what in [
        (r"authorization\s*\n?\s*\.as_ref\(\)\s*\n?\s*\.map\(\|authorization\| authorization\.snapshot\.network_intent\(\)\)",
         "process owner reads the network intent from the authorization snapshot"),
        (r"OfflineRequired currently requires a constrained filesystem profile",
         "process owner refuses OfflineRequired under an unconstrained profile"),
        (r"accepted OfflineRequired policy cannot run without a required sandbox launch",
         "process owner refuses OfflineRequired without a required sandbox launch"),
        (r"OfflineRequired is unavailable on this platform",
         "process owner refuses OfflineRequired on an unsupported platform"),
    ]:
        err = check_contains(managed_rs, pattern, what)
        if err:
            errors.append(err)

    # 3. The durable payloads that carry a command to the scheduler must
    #    carry the intent and the accepted profile ceiling with it.
    jobs_rs = ROOT / "crates/codegg-core/src/jobs/mod.rs"
    for variant in ("ManagedArgv", "Shell", "Test", "Python"):
        body = job_payload_variant(jobs_rs.read_text(), variant)
        if body is None:
            errors.append(f"crates/codegg-core/src/jobs/mod.rs: missing JobPayload::{variant}")
            continue
        for pattern, what in [
            (r"network_intent\s*:\s*crate::approval::NetworkIntent", f"{variant} network intent"),
            (r"sandbox_profile\s*:\s*Option<crate::approval::SandboxProfile>", f"{variant} profile ceiling"),
        ]:
            if not re.search(pattern, body, re.DOTALL):
                errors.append(
                    f"crates/codegg-core/src/jobs/mod.rs: JobPayload::{variant} is missing {what}"
                )

    # 4. The bash dispatch threads the authenticated intent into the
    #    durable payload, and the shell executor consumes it.
    bash_process_rs = ROOT / "src/tool/bash/process.rs"
    for pattern, what in [
        (r"snapshot\.network_intent\(\)", "bash dispatch reads the network intent"),
        (r"JobPayload::Shell \{[^}]*network_intent", "bash dispatch carries the intent into the Shell payload"),
        (r"JobPayload::ManagedArgv \{[^}]*network_intent", "bash dispatch carries the intent into the ManagedArgv payload"),
    ]:
        err = check_contains(bash_process_rs, pattern, what)
        if err:
            errors.append(err)

    executors_rs = ROOT / "src/scheduler/executors.rs"
    for pattern, what in [
        (r"request\.required_network_intent\s*=\s*network_intent", "shell executor applies the carried network intent"),
        (r"request\.required_sandbox_profile\s*=\s*sandbox_profile", "shell executor applies the carried profile ceiling"),
        (r"sandbox_config_for_profile", "shell executor rebuilds the accepted containment"),
    ]:
        err = check_contains(executors_rs, pattern, what)
        if err:
            errors.append(err)

    # 5. The other launch owners apply the intent to their sandbox config.
    for relative, what in [
        ("src/interactive_process.rs", "PTY owner"),
        ("src/lsp/m004.rs", "LSP owner"),
    ]:
        path = ROOT / relative
        err = check_contains(
            path,
            r"NetworkIntent::OfflineRequired\s*=>\s*\{[^}]*with_linux_offline_network\(true\)",
            f"{what} applies the offline network candidate",
        )
        if err:
            errors.append(err)

    # 6. The durable Test/Python owners apply the carried intent.
    for relative, what in [
        ("src/test_runner/runner.rs", "Test owner"),
        ("src/python_script/executor.rs", "Python owner"),
    ]:
        path = ROOT / relative
        err = check_contains(
            path,
            r"required_network_intent\s*=\s*network_intent",
            f"{what} applies the carried network intent",
        )
        if err:
            errors.append(err)

    # 7. No production owner may hardcode the unrestricted intent into a
    #    payload or spawn spec. A hardcoded `LegacyUnrestricted` is exactly
    #    the silent widening this guard exists to catch: the principal asked
    #    for offline execution and the field says otherwise.
    #
    #    A producer with no authenticated intent in scope may use the
    #    unrestricted default, but only when the daemon stamps the
    #    principal's own policy onto the job before it runs. Each such site
    #    is listed with that reason, so the allowlist cannot grow silently.
    no_intent_in_scope = {
        "src/tui/commands/test.rs": (
            "the TUI has no authenticated intent in scope; the daemon stamps the "
            "principal's policy onto the payload at job admission"
        ),
        "src/run_rerun.rs": (
            "the rerun builder has no authenticated intent in scope; the daemon "
            "stamps the principal's policy onto the child job at admission"
        ),
        "src/interactive_process.rs": (
            "SpawnSpec::new is a constructor default; the daemon-owned spawn path "
            "sets the intent on every spec it launches"
        ),
        "src/scheduler/tool_program_executor.rs": (
            "the tool-program child-job builder has no authenticated intent in "
            "scope; ChildJobRequest does not yet carry one"
        ),
    }
    for relative in (
        "src/tool/bash/process.rs",
        "src/scheduler/executors.rs",
        "src/scheduler/tool_program_executor.rs",
        "src/test_runner/runner.rs",
        "src/python_script/executor.rs",
        "src/interactive_process.rs",
        "src/lsp/m004.rs",
        "src/run_rerun.rs",
        "src/tui/commands/test.rs",
    ):
        path = ROOT / relative
        try:
            text = path.read_text()
        except Exception:
            continue
        production = text.split("#[cfg(test)]", 1)[0]
        for idx, line in enumerate(production.splitlines(), start=1):
            # Only a field initializer is a widening. A `match` arm that names
            # the unrestricted variant is how an owner *handles* the intent,
            # and a bare default argument is a convenience wrapper.
            if not re.search(
                r"network_intent\s*:\s*\S*NetworkIntent::LegacyUnrestricted", line
            ):
                continue
            reason = no_intent_in_scope.get(relative)
            if reason is None:
                errors.append(
                    f"{relative}:{idx}: production owner hardcodes the unrestricted network intent"
                )
            else:
                print(
                    f"note: {relative}:{idx} uses the unrestricted default because {reason}",
                    file=sys.stderr,
                )

    return errors


def job_payload_variant(text: str, variant: str) -> str | None:
    """Return the body of the `JobPayload::<variant>` *definition*.

    A plain search for `Variant {` also matches a match-arm pattern such as
    `JobPayload::Python { source_hash, .. }`, whose body carries no fields to
    check and which appears earlier in the file than the declaration. The
    definition is the variant declared at the enum's own indentation, so the
    search is anchored to that indentation.
    """
    m = re.search(
        rf"^    {re.escape(variant)} \{{(.*?)^    \}}",
        text,
        re.DOTALL | re.MULTILINE,
    )
    return m.group(1) if m else None


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
    errors.extend(check_network_intent_owner_wiring())

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
