#!/usr/bin/env python3
"""Re-measure the architecture/overview.md 'Verified counts' rows against source.

The counting method per row is the one recorded in overview.md's Source column:
attributes and comments are stripped before enum variants are counted, and
struct variants are counted separately from unit variants.

This script is the *re-measurement* side of the overview contract: overview.md
publishes the numbers and names the source for each row; this script derives
them again so a drift is visible rather than silently published.
"""
from __future__ import annotations

import re
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def strip_comments(text: str) -> str:
    """Remove line comments and doc comments, keeping code intact."""
    out = []
    for line in text.splitlines():
        s = line.strip()
        if s.startswith("//"):
            continue
        out.append(re.sub(r"//.*$", "", line))
    return "\n".join(out)


def enum_body(text: str, enum_name: str) -> str:
    """Return the brace-balanced body of `enum <enum_name>`."""
    m = re.search(rf"\benum\s+{re.escape(enum_name)}\b[^{{]*\{{", text)
    if not m:
        raise SystemExit(f"enum {enum_name} not found")
    depth = 0
    for j in range(m.end() - 1, len(text)):
        if text[j] == "{":
            depth += 1
        elif text[j] == "}":
            depth -= 1
            if depth == 0:
                return text[m.end() : j]
    raise SystemExit(f"unbalanced enum {enum_name}")


def count_variants(text: str, enum_name: str) -> tuple[int, int, int]:
    """Return (total, struct_variants, unit_variants) for an enum."""
    body = strip_comments(enum_body(text, enum_name))
    total = struct = unit = 0
    for raw in body.splitlines():
        line = raw.strip()
        if not line:
            continue
        m = re.match(r"^([A-Z][A-Za-z0-9_]*)\s*(\{|\(|,|=>|$)", line)
        if not m:
            continue
        follow = m.group(2)
        if follow in ("{", "("):
            struct += 1
        else:
            unit += 1
        total += 1
    return total, struct, unit


def fn_body(text: str, pattern: str) -> str:
    """Return the brace-balanced body of the function matching `pattern`."""
    m = re.search(pattern, text)
    if not m:
        raise SystemExit(f"function not found: {pattern}")
    depth = 0
    for j in range(m.end() - 1, len(text)):
        if text[j] == "{":
            depth += 1
        elif text[j] == "}":
            depth -= 1
            if depth == 0:
                return text[m.end() : j]
    raise SystemExit(f"unbalanced function: {pattern}")


def row(label: str, value: str, source: str) -> None:
    print(f"{label}\t{value}\t{source}")


def tool_name_measurement() -> None:
    """Distinct production tool names.

    An `impl Tool for` block inside a `#[cfg(test)]` module is a test-only
    mock, not a production tool. The trait path may be written `Tool`,
    `crate::tool::Tool`, or `super::super::Tool`, so the prefix is matched
    generically.
    """
    # The whole `src/` tree, not just `src/tool/` + `src/context/`: a production
    # `Tool` impl may live with the subsystem that owns the tool's behaviour
    # (`src/python_script/tool.rs` is one). Scoping the scan to the registry
    # directory alone undercounts the production surface.
    tool_files = sorted(ROOT.glob("src/**/*.rs"))
    impl_re = re.compile(r"impl\s+(?:(?:crate|super|self)(?:::\w+)*::)?Tool\s+for\s+(\w+)")
    prod: list[tuple[str, str, str | None]] = []
    test: list[tuple[str, str, str | None]] = []
    for path in tool_files:
        text = path.read_text()
        ranges: list[tuple[int, int]] = []
        for tm in re.finditer(r"#\[cfg\(test\)\]", text):
            brace = text.find("{", tm.end())
            if brace == -1:
                continue
            depth = 0
            end = brace
            for j in range(brace, len(text)):
                if text[j] == "{":
                    depth += 1
                elif text[j] == "}":
                    depth -= 1
                    if depth == 0:
                        end = j
                        break
            ranges.append((tm.start(), end))
        for m in impl_re.finditer(text):
            is_test = any(s <= m.start() <= e for s, e in ranges)
            tail = text[m.end() :]
            nm = re.search(r"fn\s+name\s*\(&self\)[^{]*\{(.*?)\n\s{0,8}\}", tail, re.S)
            lit = re.search(r'"([^"]+)"', nm.group(1)) if nm else None
            entry = (str(path), m.group(1), lit.group(1) if lit else None)
            (test if is_test else prod).append(entry)
    names = sorted({e[2] for e in prod if e[2]})
    runtime = [e for e in prod if e[2] is None]

    # The literal-name scan cannot see the eggsact deterministic family: its
    # `impl Tool for EggsactTool` returns `self.codegg_name`, so the name
    # lives at the constructor call site instead. Those 13 names are real
    # production tools the model can call, so they belong in the total.
    det_src = (ROOT / "src/tool/deterministic.rs").read_text()
    eggsact_names = sorted(
        set(re.findall(r'EggsactTool::new\(\s*\n\s*"([a-z0-9_]+)"', det_src))
    )

    # A runtime-named family that only *substitutes* for an already-counted
    # tool adds no new distinct name. `DisabledTool` carries the name of the
    # tool it replaces (`lsp`, `lsp_read`, `security`), and `UnrootedTool`
    # forwards its inner tool's name, so neither widens the set.
    total_names = sorted(set(names) | set(eggsact_names))
    row(
        "Distinct production tool names",
        f"{len(total_names)} names "
        f"({len(names)} literal + {len(eggsact_names)} eggsact runtime-named; "
        f"{len(prod)} production impl blocks, {len(runtime)} runtime-resolved; "
        f"{len(test)} test-only mocks excluded)",
        "impl Tool for … name() across src/ (whole tree) + EggsactTool::new() names",
    )
    row("  tool names", ", ".join(total_names), "literal + eggsact names")
    row("  eggsact tool names", ", ".join(eggsact_names), "src/tool/deterministic.rs::EggsactTool::new")


def main() -> None:
    protocol_core = (ROOT / "crates/codegg-protocol/src/core.rs").read_text()
    projection_event = (ROOT / "crates/codegg-protocol/src/projection/event.rs").read_text()
    core_events = (ROOT / "crates/codegg-core/src/bus/events.rs").read_text()

    for label, text, name in (
        ("`CoreRequest` variants", protocol_core, "CoreRequest"),
        ("`CoreResponse` variants", protocol_core, "CoreResponse"),
        ("`CoreEvent` variants", protocol_core, "CoreEvent"),
        ("`ProjectionEvent` variants", projection_event, "ProjectionEvent"),
        ("`AppEvent` variants", core_events, "AppEvent"),
    ):
        total, struct, unit = count_variants(text, name)
        row(label, f"{total} ({struct} struct + {unit} unit)", "enum body")

    tool_mod = (ROOT / "src/tool/mod.rs").read_text()
    body = fn_body(tool_mod, r"fn\s+with_options\s*\([^)]*\)[^{]*\{")
    regs = len(re.findall(r"registry\.register\s*\(", body))
    regs_boxed = len(re.findall(r"registry\.register_boxed\s*\(", body))
    row(
        "Tool registrations in `with_options()`",
        f"{regs + regs_boxed} ({regs} register + {regs_boxed} register_boxed)",
        "src/tool/mod.rs::with_options",
    )

    tool_name_measurement()

    lsp_server = (ROOT / "crates/egglsp/src/server.rs").read_text()
    lsp_body = fn_body(lsp_server, r"fn\s+server_definitions\s*\([^)]*\)[^{]*\{")
    row(
        "LSP server definitions",
        str(len(re.findall(r"\bLspServerDef\s*\{", lsp_body))),
        "crates/egglsp/src/server.rs::server_definitions",
    )

    core_lib = (ROOT / "crates/codegg-core/src/lib.rs").read_text()
    row(
        "`codegg-core` modules",
        str(len(re.findall(r"^pub mod ", core_lib, re.M))),
        "crates/codegg-core/src/lib.rs",
    )

    policy = (ROOT / "crates/codegg-core/src/authorization/policy.rs").read_text()
    ops = set(re.findall(r'OperationDescriptor::new\(\s*"([^"]+)"', policy))
    row("Authorized operations", str(len(ops)), "authorization/policy.rs::operation_descriptor")

    identity = (ROOT / "crates/codegg-core/src/identity.rs").read_text()
    row(
        "Identity newtypes",
        str(len(re.findall(r"typed_identity!\s*\(", identity))),
        "identity.rs typed_identity!",
    )

    cmd = (ROOT / "src/tui/command.rs").read_text()
    m = re.search(r"assert_eq!\(\s*CommandRegistry::built_in_commands\(\)\.len\(\),\s*(\d+)", cmd)
    row("Built-in slash commands", m.group(1) if m else "?", "src/tui/command.rs tripwire")

    schema = (ROOT / "crates/codegg-core/src/session/schema.rs").read_text()
    names = re.findall(r"CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?(\w+)", schema, re.I)
    c = Counter(n.lower() for n in names)
    # The table publishes both numbers in one row, so emit both here.
    tables = len([n for n in c if n != "origin_attribution_new"])
    row(
        "Database tables (`CREATE TABLE`)",
        f"{len(names)} statements / {tables} tables",
        "session/schema.rs",
    )

    git_op = (ROOT / "crates/codegg-git/src/operation.rs").read_text()
    total, struct, unit = count_variants(git_op, "GitOperation")
    row("Git operation variants", f"{total} ({struct} struct + {unit} unit)", "codegg-git/src/operation.rs")

    git_risk = (ROOT / "crates/codegg-git/src/risk.rs").read_text()
    t2, s2, u2 = count_variants(git_risk, "GitRiskClass")
    row("Git risk classes", f"{t2} ({s2} struct + {u2} unit)", "codegg-git/src/risk.rs")

    cargo = (ROOT / "Cargo.toml").read_text()
    members = re.search(r"members\s*=\s*\[(.*?)\]", cargo, re.S)
    member_list = [x for x in re.findall(r'"([^"]+)"', members.group(1)) if x]
    # The root package is itself a member ("."), so the member list holds the
    # root plus the member crates. The published row counts the *crates*.
    row(
        "Workspace member crates",
        f"{len(member_list) - 1} ({len(member_list) - 1} + root)",
        "root Cargo.toml members",
    )

    row(
        "Integration test files",
        str(len(list(ROOT.glob("tests/*.rs"))) + len(list(ROOT.glob("tests/*/*.inc")))),
        "tests/*.rs + tests/*/*.inc",
    )
    # Two distinct guard populations. `scripts/check_*` is every guard the
    # repository ships; the CI workflow names a strict subset of them. They
    # must not share a label — publishing the total under a "CI" label would
    # tell a reader that CI runs guards it does not run.
    row("Guard scripts (all)", str(len(list(ROOT.glob("scripts/check_*")))), "scripts/check_*")
    ci_workflow = (ROOT / ".github/workflows/ci.yml").read_text()
    ci_guards = sorted(
        {
            m.group(0)
            for m in re.finditer(r"scripts/check[-_][a-z0-9_.-]+\.(?:py|sh)", ci_workflow)
        }
    )
    row("CI guard scripts", str(len(ci_guards)), ".github/workflows/ci.yml")
    row("Architecture docs", str(len(list(ROOT.glob("architecture/*.md")))), "architecture/*.md")

    # `src/tool/**.rs` (incl. `bash/`) — the nested `bash/tests/` fixture
    # directory holds test support, not tool source.
    tool_src = [
        p
        for p in ROOT.glob("src/tool/**/*.rs")
        if "tests" not in p.relative_to(ROOT / "src/tool").parts[:-1]
    ]
    row("Tool source files", str(len(tool_src)), "src/tool/**.rs (incl. bash/)")

    row("Built-in agents", str(len(list(ROOT.glob("assets/agents/*.toml")))), "assets/agents/*.toml")
    row("Bundled themes", str(len(list(ROOT.glob("assets/themes/**/*.toml")))), "assets/themes/**/*.toml")
    row(
        "TUI state modules",
        f"{len(list(ROOT.glob('src/tui/app/state/*.rs'))) - 1} "
        f"({len(list(ROOT.glob('src/tui/app/state/*.rs')))} .rs files incl. mod.rs)",
        "src/tui/app/state/",
    )
    storage = (ROOT / "crates/codegg-core/src/storage/mod.rs").read_text()
    m = re.search(r"STORAGE_LAYOUT_VERSION:\s*u32\s*=\s*(\d+)", storage)
    row("Storage layout version", m.group(1) if m else "?", "storage/mod.rs::STORAGE_LAYOUT_VERSION")


if __name__ == "__main__":
    main()
