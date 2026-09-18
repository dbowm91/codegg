#!/usr/bin/env python3
"""
Static guard for the M001 first-run credential-key bootstrap.

Enforces the canonical master-key resolver ownership:

  * Rule `direct-master-key-env`: production code must never read the
    master-key environment chain (`CODEGG_MASTER_KEY`,
    `CODEGG_ENCRYPTION_KEY`, `OPENCODE_ENCRYPTION_KEY`) or the managed-key
    path override (`CODEGG_MASTER_KEY_FILE`) directly. The only production
    owner is `crates/codegg-config/src/encryption.rs`. Test code (anything
    under `tests/` or inside `#[cfg(test)]` modules) is exempt because
    tests legitimately pin and isolate the environment.
  * Rule `write-bypasses-canonical-resolver`: inside the secret-store
    write files, `get_master_key()` (read-only) must only appear in an
    explicit allowlist of read functions. Every other enclosing function
    — current or future — must go through `get_or_create_master_key*`,
    so a new credential write cannot silently bypass the bootstrap and
    reintroduce the manual `CODEGG_MASTER_KEY` prerequisite.
  * Rule `managed-key-expose-boundary`: `.expose()` on a resolved managed
    key (`master_key.expose()` / `resolved.expose()`) may only appear in
    the two approved crypto call sites or test code, keeping the key
    value out of logs, errors, and snapshots.

Exits 0 when all checks pass, 1 otherwise. Each finding includes the
file, line, and a one-line rationale.
"""

from __future__ import annotations

import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional, Tuple


ROOT = Path(__file__).resolve().parent.parent

MASTER_ENV_VARS = (
    "CODEGG_MASTER_KEY",
    "CODEGG_ENCRYPTION_KEY",
    "OPENCODE_ENCRYPTION_KEY",
    "CODEGG_MASTER_KEY_FILE",
)

# Production owner of direct master-key environment access.
ENV_OWNER = "crates/codegg-config/src/encryption.rs"

# Files whose write paths are covered by the resolver rule.
WRITE_FILES = (
    "crates/codegg-providers/src/auth_types.rs",
    "crates/codegg-providers/src/connection.rs",
    "crates/codegg-providers/src/provider_core.rs",
    "src/mcp/auth.rs",
    "src/core/eggpool.rs",
    "src/auth/cli.rs",
)

# Read-only functions allowed to call `get_master_key()` per file. Any
# other enclosing function using it is a finding, so future writes fail
# closed until they adopt `get_or_create_master_key*`.
READ_ALLOWLIST = {
    "crates/codegg-providers/src/auth_types.rs": {
        "resolve",  # AuthResolver::resolve (encrypted_value read)
        "get_plaintext",  # CredentialStore read
        "get_credential",  # CredentialStore read
        "resolve_write_key",  # fast-path existence probe, then get_or_create
    },
    "crates/codegg-providers/src/connection.rs": {
        "resolve",  # CredentialStoreAdapter::resolve (read)
    },
    "crates/codegg-providers/src/provider_core.rs": {
        "resolve_provider_credential",  # legacy encrypted_api_key read
    },
    "src/mcp/auth.rs": {
        "load_tokens_sync",  # legacy migration read probe
        "decode_token_store",  # token-store read
        "resolve_token_write_key",  # fast-path existence probe
    },
    "src/core/eggpool.rs": set(),
    "src/auth/cli.rs": set(),
}

# Files allowed to call `.expose()` on a resolved managed key.
EXPOSE_ALLOWLIST = {
    "crates/codegg-providers/src/auth_types.rs",
    "src/mcp/auth.rs",
}


@dataclass
class Finding:
    rule: str
    file: str
    line: int
    message: str


def rg(pattern: str, root: Path, include: Optional[str] = None) -> List[Tuple[Path, int, str]]:
    args = [
        "rg",
        "--line-number",
        "--no-heading",
        "--color=never",
        "--no-messages",
    ]
    if include is not None:
        args.extend(["-g", include])
    args.extend([pattern, str(root)])
    try:
        result = subprocess.run(args, capture_output=True, text=True, check=False)
    except FileNotFoundError:
        print("error: ripgrep (`rg`) not found on PATH", file=sys.stderr)
        sys.exit(2)
    out: List[Tuple[Path, int, str]] = []
    for raw in result.stdout.splitlines():
        parts = raw.split(":", 2)
        if len(parts) < 3:
            continue
        path_str, lineno_str, content = parts
        try:
            lineno = int(lineno_str)
        except ValueError:
            continue
        out.append((Path(path_str), lineno, content))
    return out


def _in_test_module(path: Path, lineno: int) -> bool:
    """Heuristic: True if `lineno` falls inside a `#[cfg(test)]` module."""
    try:
        text = path.read_text()
    except (FileNotFoundError, IsADirectoryError):
        return False
    lines = text.splitlines()
    depth = 0
    in_test = False
    for i, line in enumerate(lines, start=1):
        stripped = line.strip()
        if depth == 0 and "mod tests" in stripped and stripped.endswith("{"):
            for j in range(i - 2, max(-1, i - 5), -1):
                if j < 0:
                    break
                prev = lines[j].strip()
                if not prev:
                    continue
                if "cfg(test)" in prev:
                    in_test = True
                    break
                if prev.endswith(";") or prev.endswith("{"):
                    continue
                break
        if stripped.endswith("{") and not stripped.startswith("#"):
            depth += 1
        if stripped.startswith("}"):
            depth -= 1
            if depth <= 0:
                in_test = False
                depth = 0
        if i == lineno:
            return in_test
    return False


def _is_test_code(path: Path, lineno: int) -> bool:
    path_str = path.relative_to(ROOT).as_posix() if path.is_absolute() else path.as_posix()
    if "/tests/" in f"/{path_str}" or path_str.startswith("tests/"):
        return True
    if path.suffix == ".rs" and _in_test_module(path, lineno):
        return True
    return False


def _is_doc(path: Path) -> bool:
    path_str = path.relative_to(ROOT).as_posix() if path.is_absolute() else path.as_posix()
    return any(
        sub in path_str
        for sub in ("docs/", "plans/", "architecture/", "AGENTS.md", "README.md", "examples/")
    )


def check_direct_env() -> List[Finding]:
    findings: List[Finding] = []
    pattern = r'env::(var|set_var|remove_var)\s*\(\s*"(?:' + "|".join(MASTER_ENV_VARS) + r')"'
    for path, lineno, _content in rg(pattern, ROOT, "*.rs"):
        rel = path.relative_to(ROOT).as_posix() if path.is_absolute() else path.as_posix()
        if rel == ENV_OWNER:
            continue
        if _is_doc(path):
            continue
        if _is_test_code(path, lineno):
            continue
        findings.append(
            Finding(
                rule="direct-master-key-env",
                file=rel,
                line=lineno,
                message="production master-key env access must go through codegg-config::encryption",
            )
        )
    return findings


def _enclosing_fn(path: Path, lineno: int) -> Optional[str]:
    """Best-effort enclosing `fn name` for a line (brace-depth scan)."""
    try:
        lines = path.read_text().splitlines()
    except (FileNotFoundError, IsADirectoryError):
        return None
    fn_re = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z0-9_]+)")
    depth = 0
    # Walk forward tracking depth is unreliable without full parsing, so
    # scan backwards for the nearest `fn` whose block contains the line:
    # approximate by finding the closest preceding `fn` at a lower brace
    # depth than the target line's depth.
    brace_depths = [0] * (len(lines) + 1)
    d = 0
    for i, line in enumerate(lines, start=1):
        brace_depths[i] = d
        d += line.count("{") - line.count("}")
    target_depth = brace_depths[lineno] if lineno <= len(lines) else 0
    for i in range(lineno - 1, 0, -1):
        m = fn_re.match(lines[i - 1])
        if m and brace_depths[i] <= target_depth:
            return m.group(1)
    return None


def check_write_resolver() -> List[Finding]:
    findings: List[Finding] = []
    for path, lineno, _content in rg(r"get_master_key\(\)", ROOT, "*.rs"):
        rel = path.relative_to(ROOT).as_posix() if path.is_absolute() else path.as_posix()
        if rel == ENV_OWNER:
            continue
        if _is_doc(path):
            continue
        if _is_test_code(path, lineno):
            continue
        if rel not in WRITE_FILES:
            continue
        allowed = READ_ALLOWLIST.get(rel, set())
        enclosing = _enclosing_fn(path, lineno)
        if enclosing in allowed:
            continue
        findings.append(
            Finding(
                rule="write-bypasses-canonical-resolver",
                file=rel,
                line=lineno,
                message=(
                    f"get_master_key() in non-read fn '{enclosing}'; "
                    "writes must use get_or_create_master_key*"
                ),
            )
        )
    return findings


def check_expose_boundary() -> List[Finding]:
    findings: List[Finding] = []
    for path, lineno, content in rg(r"(master_key|resolved)\.expose\(\)", ROOT, "*.rs"):
        rel = path.relative_to(ROOT).as_posix() if path.is_absolute() else path.as_posix()
        if rel in EXPOSE_ALLOWLIST:
            continue
        if _is_doc(path):
            continue
        if _is_test_code(path, lineno):
            continue
        # Error/display impls that only name the method in a comment-free
        # check would false-positive; only flag real call expressions.
        if ".expose()" not in content:
            continue
        findings.append(
            Finding(
                rule="managed-key-expose-boundary",
                file=rel,
                line=lineno,
                message="ManagedMasterKey::expose() outside approved crypto call sites",
            )
        )
    return findings


def main() -> int:
    findings: List[Finding] = []
    findings.extend(check_direct_env())
    findings.extend(check_write_resolver())
    findings.extend(check_expose_boundary())
    if findings:
        for finding in sorted(findings, key=lambda f: (f.file, f.line)):
            print(f"{finding.file}:{finding.line}: [{finding.rule}] {finding.message}")
        print(f"\n{finding_count(len(findings))} — see rationale above.", file=sys.stderr)
        return 1
    print("master-key resolver guard passed")
    return 0


def finding_count(n: int) -> str:
    return f"{n} forbidden master-key pattern(s) found"


if __name__ == "__main__":
    sys.exit(main())
