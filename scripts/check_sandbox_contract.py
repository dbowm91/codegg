#!/usr/bin/env python3
"""Guard the child-only sandbox boundary and maintained Landlock backend."""

from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
FILES = [
    ROOT / "src/security/sandbox.rs",
    ROOT / "src/python_script/executor.rs",
    ROOT / "src/python_script/sandbox.rs",
    ROOT / "src/bin/codegg-sandbox-helper.rs",
    ROOT / "src/managed_process.rs",
    ROOT / "src/tool/bash.rs",
]
FORBIDDEN = (
    r"landlock_create_ruleset",
    r"landlock_add_rule",
    r"landlock_restrict_self",
    r"SYS_LANDLOCK_",
    r"PR_GET_LANDLOCK",
)

def violations(path: Path, content: str) -> list[str]:
    found: list[str] = []
    for pattern in FORBIDDEN:
        if re.search(pattern, content):
            found.append(f"forbidden handwritten Landlock symbol {pattern}")
    if path.name == "executor.rs" and ".pre_exec" in content:
        found.append("sandbox policy must not be built in pre_exec")
    if re.search(r"(?:std::env::var|std::env::var_os|env::var|env::var_os)\s*\(\s*['\"]CODEGG_SANDBOX_HELPER['\"]", content):
        found.append("production helper identity must not come from CODEGG_SANDBOX_HELPER")
    if re.search(r"SANDBOX_HELPER_(?:ENFORCED|ERROR)_PREFIX|parse_sandbox_(?:result|stderr)", content):
        found.append("sandbox setup status must not use stderr marker parsing")
    if re.search(r"NamedTempFile::new_in\(\s*cwd\s*\)", content):
        found.append("sandbox helper specification must not be created in target cwd")
    if path.name == "sandbox.rs":
        helper_match = re.search(
            r"pub fn sandbox_helper_path\(\).*?(?=\n(?:pub )?fn |\n#\[)",
            content,
            re.DOTALL,
        )
        if helper_match and re.search(r"PATH|split_paths|current_dir", helper_match.group(0)):
            found.append("sandbox helper resolution must use the installation-owned sibling")
    if path.name == "managed_process.rs" and "SandboxRequest::Required" in content:
        if "decode_sandbox_status" not in content or "SandboxFailed" not in content:
            found.append("required sandbox execution must fail closed without a typed status")
    return found


def run_guard() -> int:
    failures: list[str] = []
    for path in FILES:
        failures.extend(
            f"{path.relative_to(ROOT)}: {failure}"
            for failure in violations(path, path.read_text())
        )
    if failures:
        print("sandbox contract guard failed:", file=sys.stderr)
        print("\n".join(failures), file=sys.stderr)
        return 1
    print("sandbox contract guard passed")
    return 0


def self_test() -> int:
    fixtures = {
        "helper environment": 'std::env::var("CODEGG_SANDBOX_HELPER")',
        "stderr marker": 'const X: &str = SANDBOX_HELPER_ERROR_PREFIX;',
        "cwd spec": 'tempfile::NamedTempFile::new_in(cwd)',
        "missing status bypass": 'SandboxRequest::Required(spec) /* no typed status */',
    }
    checks = {
        "helper environment": lambda text: any(
            "CODEGG_SANDBOX_HELPER" in issue for issue in violations(ROOT / "sandbox.rs", text)
        ),
        "stderr marker": lambda text: any(
            "stderr marker" in issue for issue in violations(ROOT / "managed_process.rs", text)
        ),
        "cwd spec": lambda text: any(
            "target cwd" in issue for issue in violations(ROOT / "managed_process.rs", text)
        ),
        "missing status bypass": lambda text: any(
            "fail closed" in issue
            for issue in violations(ROOT / "managed_process.rs", text)
        ),
    }
    failures = [name for name, text in fixtures.items() if not checks[name](text)]
    if failures:
        print(f"sandbox contract guard self-test failed: {', '.join(failures)}", file=sys.stderr)
        return 1
    print("sandbox contract guard self-test passed")
    return 0


if __name__ == "__main__":
    if "--self-test" in sys.argv[1:]:
        raise SystemExit(self_test())
    raise SystemExit(run_guard())
