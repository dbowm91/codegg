#!/usr/bin/env python3
"""Static lint: tool broker boundary guard.

The contract
------------
`ToolBroker::execute` is the only production path from a subsystem to a tool.
A subsystem that reaches a tool directly has bypassed the broker's
pre-execution validation, which is where the caller's authority grant, the
tool contract's effect class, the caller policy, and the sandbox decision
live.

Two *different* call shapes exist, and this guard tells them apart by the
method they call rather than by the file they live in. That distinction is
the whole repair: the previous version matched "`.execute_structured(`" and
"`.execute(&input`" everywhere, so five intra-tool delegations were reported
as boundary crossings and the guard could never be green.

1. **Registry dispatch** — `ToolRegistry::execute_capture(name, input, ctx)`.
   It looks a tool up *by name* and runs it. Outside the registry's own
   module this is always a boundary crossing, because the registry is the
   thing the broker wraps.

2. **Tool-instance dispatch** — `Tool::execute(input)` /
   `Tool::execute_structured(input, ctx)`. Inside `src/tool/` this is
   *intra-tool delegation*: a tool calling its own implementation, or a
   wrapper delegating to the inner tool it owns. The broker cannot
   intercept it — there is no registry lookup and no grant to check — and
   forbidding it would forbid tool composition outright. Outside `src/tool/`
   a production caller has no legitimate way to hold a tool instance, so
   there it is a boundary crossing.

`ToolBroker::execute(registry, name, args, ctx)` is the sanctioned path and is
never a finding.

Recorded exceptions
-------------------
One production caller reaches the registry outside the broker, and it has to:
`src/permission/reviewer.rs`. The Automatic approval reviewer adjudicates an
approval request, and it investigates that request with a bounded read-only
tool palette *without* re-entering the approval path. Routing its call
through the broker would require a verified authority grant — the product of
the very ApprovalRouter the reviewer is servicing — so the reviewer would
have to approve its own investigation. `scripts/check_approval_reviewer.py`
already forbids the reviewer module from naming `ApprovalRouter` at all.

The exception is therefore recorded here, with a named owner, rather than
solved by a routing change that would be an authorization change. It is
scoped to ONE call in ONE function of ONE file: widening it requires editing
this guard, which is a decision someone has to make on purpose.

Scope
-----
The root package's `src/` tree. The workspace crates do not hold the root's
tool registry or broker, so they have no boundary to cross.

Run:
  python3 scripts/check_tool_broker_boundary.py
  python3 scripts/check_tool_broker_boundary.py --self-test

Exit code 0 on success, 1 on failure.
"""

from __future__ import annotations

import re
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "src"

# The tool-module tree. Inside it, tools compose with each other and a
# tool-instance dispatch is intra-tool delegation, not a boundary crossing.
TOOL_MODULE = "src/tool"

# `ToolRegistry::execute_capture` — a registry dispatch. Always a finding in
# production code outside the registry's own module.
REGISTRY_DISPATCH = re.compile(r"\.execute_capture\s*\(")

# `Tool::execute` / `Tool::execute_structured` — a tool-instance dispatch.
# The argument shape is part of the signature: `Tool::execute` takes exactly
# one `serde_json::Value`, which the broker's own four-argument
# `ToolBroker::execute(registry, name, args, ctx)` does not.
TOOL_INSTANCE_DISPATCH = [
    re.compile(r"\.execute_structured\s*\("),
    re.compile(r"\.execute\s*\(\s*&?\s*input\b"),
]

TEST_ATTRIBUTES = (
    "#[cfg(test)]",
    "#[test]",
    "#[tokio::test]",
    "#[tokio::test(flavor = \"current_thread\")]",
)


class Exception_:
    """A recorded, owned exception to the broker boundary.

    `function` names the enclosing function the allowance is scoped to, and
    `max_calls` is how many dispatches it may contain. Both are checked, so
    the exception cannot widen silently: a second call in the same function,
    or a call anywhere else in the file, is a finding.
    """

    def __init__(self, owner: str, reason: str, function: str, max_calls: int = 1) -> None:
        self.owner = owner
        self.reason = reason
        self.function = function
        self.max_calls = max_calls


# The one recorded exception. See the module docstring for why routing the
# reviewer's call through the broker would be an authorization change.
RECORDED_EXCEPTIONS: dict[str, Exception_] = {
    "src/permission/reviewer.rs": Exception_(
        owner="M006 Automatic approval reviewer "
        "(src/permission/reviewer.rs, architecture/approval_reviewer.md, "
        "isolation guard scripts/check_approval_reviewer.py)",
        reason=(
            "the reviewer adjudicates an approval request and must investigate it "
            "WITHOUT re-entering the approval path; ToolBroker::execute requires a "
            "verified authority grant, which is the product of the ApprovalRouter "
            "the reviewer is servicing, so routing it through the broker would make "
            "the reviewer approve its own investigation"
        ),
        function="investigate",
        max_calls=1,
    ),
}

# The nearest preceding `fn NAME(` is the enclosing function of a call site.
ENCLOSING_FN = re.compile(r"(?:async\s+)?fn\s+(\w+)\s*[(<]")


def is_test_context(lines: list[str], line_idx: int) -> bool:
    """Check if a line is inside a test module or test function.

    Walk back to the nearest test attribute while tracking braces. The old
    fixed-line lookback misclassified longer test bodies as production code.
    """
    brace_depth = 0
    for i in range(line_idx, -1, -1):
        line = lines[i]
        brace_depth += line.count("{") - line.count("}")
        stripped = line.strip()
        if stripped.startswith("#[cfg(test)]"):
            return brace_depth > 0
        if stripped.startswith("#[test]") or stripped.startswith("#[tokio::test]"):
            return brace_depth > 0
    return False


def enclosing_function(lines: list[str], line_idx: int) -> str:
    """The nearest preceding `fn NAME(` — the enclosing function of a call."""
    for i in range(line_idx, -1, -1):
        match = ENCLOSING_FN.search(lines[i])
        if match:
            return match.group(1)
    return "<top-level>"


def classify(rel_path: str, line: str) -> str | None:
    """Return the dispatch shape of a line, or None when it is not one."""
    if REGISTRY_DISPATCH.search(line):
        return "registry"
    for pattern in TOOL_INSTANCE_DISPATCH:
        if pattern.search(line):
            return "tool-instance"
    return None


def find_violations(source_dir: Path, root: Path) -> list[str]:
    """Return one message per boundary crossing in `source_dir`.

    `root` is the directory the relative paths are resolved against, so a
    staged `--self-test` tree produces the same relative paths the real run
    compares against.
    """
    violations: list[str] = []
    for rust_file in sorted(source_dir.rglob("*.rs")):
        try:
            rel_str = str(rust_file.relative_to(root))
        except ValueError:
            # A staged tree outside the repository. Report the staged path
            # rather than crashing on it: a self-test that dies of an
            # unrelated exception has proved nothing either way.
            rel_str = str(rust_file)
        if "/tests/" in rel_str or rel_str.endswith("_test.rs"):
            continue
        try:
            lines = rust_file.read_text(encoding="utf-8").splitlines()
        except OSError:
            continue

        exception = RECORDED_EXCEPTIONS.get(rel_str)
        exception_used = 0

        for idx, line in enumerate(lines):
            stripped = line.strip()
            if stripped.startswith("//") or stripped.startswith("///") or stripped.startswith("//!"):
                continue
            shape = classify(rel_str, line)
            if shape is None:
                continue
            if is_test_context(lines, idx):
                continue

            if shape == "tool-instance" and rel_str.startswith(TOOL_MODULE + "/"):
                # Intra-tool delegation inside the tool module tree.
                continue

            if exception is not None:
                function = enclosing_function(lines, idx)
                if function == exception.function and exception_used < exception.max_calls:
                    exception_used += 1
                    continue
                violations.append(
                    f"  {rel_str}:{idx + 1}: {stripped} "
                    f"(in {function}(); the recorded exception for {exception.owner} "
                    f"covers {exception.function}() only)"
                )
                continue

            violations.append(f"  {rel_str}:{idx + 1}: {stripped}")

    return violations


def check_real_tree() -> list[str]:
    return find_violations(SRC, ROOT)


def _self_test() -> int:
    """Prove the guard rejects a genuine violation and accepts a compliant tree.

    A guard that has never been shown to reject anything is a comment.
    """
    print("check_tool_broker_boundary --self-test")

    with tempfile.TemporaryDirectory() as tmp:
        staged_root = Path(tmp)
        staged_src = staged_root / "src"
        (staged_src / "tool").mkdir(parents=True)
        (staged_src / "permission").mkdir(parents=True)

        # A compliant production subsystem: it reaches the broker, not a tool.
        (staged_src / "compliant.rs").write_text(
            "impl Sub {\n"
            "    async fn run(&self) {\n"
            "        self.tool_broker.execute(&self.registry, \"read\", args, ctx).await;\n"
            "    }\n"
            "}\n",
            encoding="utf-8",
        )
        # Intra-tool delegation inside the tool module tree is legitimate.
        (staged_src / "tool" / "wrapper.rs").write_text(
            "impl Wrapper {\n"
            "    async fn run(&self) {\n"
            "        self.inner.execute_structured(input, ctx).await\n"
            "    }\n"
            "}\n",
            encoding="utf-8",
        )
        if find_violations(staged_src, staged_root):
            print("  FAIL: self-test rejected a compliant tree. The guard is broken.")
            return 1
        print("  ok: compliant tree accepted (broker call, intra-tool delegation)")

        # Injected violation 1: a production subsystem reaching a tool
        # instance directly, outside the tool module tree.
        (staged_src / "compliant.rs").write_text(
            "impl Sub {\n"
            "    async fn run(&self) {\n"
            "        self.tool.execute(input).await;\n"
            "    }\n"
            "}\n",
            encoding="utf-8",
        )
        violations = find_violations(staged_src, staged_root)
        if not any("compliant.rs" in message for message in violations):
            print(
                "  FAIL: self-test injected a direct tool call outside src/tool/ and "
                f"the guard still passed: {violations}"
            )
            return 1
        print("  ok: injected tool-instance dispatch outside src/tool/ rejected")

        # Injected violation 2: a production subsystem reaching the registry.
        (staged_src / "compliant.rs").write_text(
            "impl Sub {\n"
            "    async fn run(&self) {\n"
            "        self.registry.execute_capture(\"read\", args, None).await;\n"
            "    }\n"
            "}\n",
            encoding="utf-8",
        )
        violations = find_violations(staged_src, staged_root)
        if not any("compliant.rs" in message for message in violations):
            print(
                "  FAIL: self-test injected a registry dispatch and the guard still "
                f"passed: {violations}"
            )
            return 1
        print("  ok: injected registry dispatch rejected")

        # Injected violation 3: the recorded exception widening. The reviewer
        # file may hold ONE registry dispatch, inside `investigate`(). A second
        # call — or a call in another function — is a finding.
        (staged_src / "compliant.rs").write_text(
            "impl Sub {\n"
            "    async fn run(&self) {\n"
            "        self.tool_broker.execute(&self.registry, \"read\", args, ctx).await;\n"
            "    }\n"
            "}\n",
            encoding="utf-8",
        )
        (staged_src / "permission" / "reviewer.rs").write_text(
            "impl Reviewer {\n"
            "    async fn investigate(&self) {\n"
            "        self.registry.execute_capture(\"read\", args, None).await;\n"
            "    }\n"
            "}\n",
            encoding="utf-8",
        )
        violations = find_violations(staged_src, staged_root)
        if violations:
            print(f"  FAIL: the recorded exception did not cover its own call: {violations}")
            return 1
        print("  ok: recorded exception covers its single call")

        (staged_src / "permission" / "reviewer.rs").write_text(
            "impl Reviewer {\n"
            "    async fn investigate(&self) {\n"
            "        self.registry.execute_capture(\"read\", args, None).await;\n"
            "    }\n"
            "    async fn widened(&self) {\n"
            "        self.registry.execute_capture(\"read\", args, None).await;\n"
            "    }\n"
            "}\n",
            encoding="utf-8",
        )
        violations = find_violations(staged_src, staged_root)
        if not any("widened()" in message for message in violations):
            print(
                "  FAIL: the recorded exception widened silently: "
                f"{violations}"
            )
            return 1
        print("  ok: widened recorded exception rejected")

    # And the unmodified tree must still pass, so the self-test proves the
    # guard rejects rather than being broken.
    if check_real_tree():
        print("  FAIL: self-test passed but the real tree has violations")
        return 1
    print("  ok: unmodified tree still passes")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        return _self_test()

    violations = check_real_tree()
    if violations:
        print("ERROR: Direct tool execution calls found outside the broker boundary.")
        print("All production tool calls must go through ToolBroker::execute().")
        print()
        for v in violations:
            print(v)
        print()
        print("To fix: route the call through self.tool_broker.execute() instead.")
        return 1

    print("OK: No direct tool execution calls outside the broker boundary.")
    for rel_path, exception in RECORDED_EXCEPTIONS.items():
        print(f"Recorded exception in force: {rel_path}")
        print(f"  owner:  {exception.owner}")
        print(f"  reason: {exception.reason}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
