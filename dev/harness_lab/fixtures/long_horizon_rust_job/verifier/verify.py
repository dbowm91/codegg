"""Hidden acceptance check; invoked by the harness outside the writable lane."""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path


def main() -> int:
    workspace = Path(sys.argv[1]).resolve()
    if not (workspace / "Cargo.toml").is_file():
        print("named failure: fixture manifest missing")
        return 2
    public_tests = workspace / "tests" / "public_contract.rs"
    if not public_tests.is_file():
        print("named failure: public contract tests were deleted")
        return 3
    baseline_tests = Path(__file__).with_name("public_contract.sha256").read_text().strip()
    import hashlib
    if hashlib.sha256(public_tests.read_bytes()).hexdigest() != baseline_tests:
        print("named failure: public contract tests were modified")
        return 4
    result = subprocess.run(["cargo", "test", "--offline", "--locked", "--manifest-path",
                             str(workspace / "Cargo.toml")], cwd=workspace,
                            text=True, capture_output=True, timeout=180)
    if result.returncode:
        print("named failure: hidden Rust contract verification failed")
        return 5
    print("hidden Rust contract passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
