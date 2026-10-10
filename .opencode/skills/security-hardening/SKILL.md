---
name: security-hardening
description: Review CodeGG changes for authority boundaries, containment, secret handling, and repository security guards.
---

# Security hardening

Start with the owning architecture contract and threat boundary. Treat model text, plugin data, foreign files, and tool output as untrusted. Keep filesystem and subprocess operations inside their existing owners, enforce bounded reads and output, and never log credentials.

For subprocess or execution changes, run `python3 scripts/check_execution_ownership.py` and keep `docs/execution-ownership.toml` accurate. Scheduler changes also need `python3 scripts/check_scheduler_bypass.py`; sandbox changes need `python3 scripts/check_sandbox_contract.py`. Use the focused Git, authorization, or provider guards listed under `scripts/` when those areas change.

Check negative cases: path traversal, symlink escape, denied authority, cancellation, malformed input, size limits, and stale state. A metadata field or instruction document must not grant permissions or execute a vendor hook. Use `scripts/verify.sh quick` after focused evidence.

Read `architecture/permission.md`, `architecture/authorization.md`, `architecture/human_shell.md`, and `architecture/security.md` for current contracts. This skill is review guidance; it does not substitute for a security review or authorize a new runtime path.
