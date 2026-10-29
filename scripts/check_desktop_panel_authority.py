#!/usr/bin/env python3
"""Guard: the desktop renderer's panel layer holds no authority (M007-B).

The desktop's renderer is a Tauri front end with **no** Tauri capability: the
host's `capabilities/main.json` carries an empty permission list, the CSP is
local-only, and every value the renderer shows arrives over the one reviewed
bridge surface in `apps/desktop/src/bridge.ts`.

The panel layer — the modules under `apps/desktop/src/panels/` and
`apps/desktop/src/regions/` — sits inside that renderer, and it is the layer
most likely to grow a second authority by accident: a panel that wants fresh
data is one line away from calling the bridge, and a panel that wants to be
clever is one line away from reaching for the filesystem. The M007-B plan
(`plans/implementation/desktop-frontend-ide-foundation/012-desktop-chat-region-and-side-panel.md`
§6) makes both of those stop conditions rather than tasks, and
`scripts/check-desktop-boundary.sh` stays the *host*-side authority. This
guard is the renderer-panel-layer half of the same boundary, and it is the
mechanical form of the plan's §6 "Documentation and static guards" clause.

The controls this guard enforces:

- **G1 — the recorded bridge baseline still matches the bridge.** The
  desktop's `invoke` targets are a **recorded** set, held in this file: the
  25 targets `apps/desktop/src/bridge.ts` names and the 25 commands the
  host's `generate_handler!` table registers. If either side grows a
  target, the guard fails until the recorded set here is deliberately
  edited. A baseline measured from the tree at run time is a baseline that
  grows by itself, and "no new bridge target" would then be a statement
  about nothing.
- **G2 — the panel layer calls no bridge.** No module under the panel or
  region layer may call `invoke`, import the bridge module, or name a
  bridge target that is not in the recorded baseline. The panels render the
  props they are given; they do not fetch.
- **G3 — the panel layer holds no Tauri capability.** No `@tauri-apps/api`
  import, no `@tauri-apps/plugin-*` import, no `__TAURI` global, no Tauri
  permission string (`shell:allow-execute`), and no capability
  declaration. A renderer capability is exactly what ADR-0010's containment
  refuses.
- **G4 — the panel layer calls no filesystem, shell or process API.** No
  `node:fs`/`fs`, no `child_process`, no `exec`/`spawn`, no `Deno.*`, no
  `fetch` to a non-local origin, and no `WebSocket`.
- **G5 — the panel layer holds no durable frontend-owned store.** No
  `localStorage`, no `sessionStorage`, no `indexedDB`, and no
  `document.cookie` write. Panel placement stays renderer-local: a
  persisted panel layout is a storage surface this milestone has no
  migration for.
- **G6 — this guard is wired into both automatic lanes.** A guard that
  nobody runs automatically protects nothing.

Deliberate scope choices, so that they are not mistaken for accidents:

- **The layer is the shipped renderer, so test modules are excluded.** A
  `*.test.tsx` file is not bundled into the renderer and may legitimately
  mention what it asserts is absent.
- **Comments are not code.** The scanner blanks comments before it reads a
  module, because a comment that says "no filesystem path crosses the
  boundary" must not be read as a filesystem call, and a `//` inside a URL
  string must not start a comment.
- **A `fetch` to a local origin is not a finding.** The local-only CSP is
  the boundary that makes a local read harmless, and this guard does not
  re-litigate it; a remote origin is.
- **An absent layer is not a finding.** The region layer does not exist
  until the milestone that adds it lands. A violating module that appears
  in it — before or after that — is caught by the same scan.

Modes:
    python3 scripts/check_desktop_panel_authority.py             # static guard
    python3 scripts/check_desktop_panel_authority.py --self-test # non-vacuity

`--self-test` builds a compliant fixture tree in a temporary directory,
requires the guard to accept it, then injects each defect class in turn and
requires the guard to reject every one of them **for the expected reason**.
The fixture is the guard's own: the self-test proves the guard's
discrimination, while the ordinary run is the verdict on the repository's
own renderer. The self-test never mutates the repository's files.
"""

from __future__ import annotations

import argparse
import re
import shutil
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# ── What the guard owns ──────────────────────────────────────────────────

# The renderer layers this guard owns. The region layer is created by the
# milestone that adds the chat region and the side-panel frame; an absent
# directory holds no authority, and a violating module that appears in it
# later is caught by the same scan.
PANEL_LAYER_DIRS = (
    Path("apps/desktop/src/panels"),
    Path("apps/desktop/src/regions"),
)

# The renderer's one bridge surface, and the desktop host's command table.
BRIDGE_MODULE = Path("apps/desktop/src/bridge.ts")
HOST_COMMAND_TABLE = Path("apps/desktop/src-tauri/src/lib.rs")

# The recorded bridge baseline: the desktop's 44 `invoke` targets, which are
# the same 44 commands the host registers. The set is recorded **here**
# rather than measured from the tree at run time, because a baseline that is
# measured is a baseline that grows by itself.
#
# The set is the union of three deliberate additions, each named in the
# change that added it:
#
# * M007-D adds `desktop_command_catalog` and `desktop_command_execute`: the
#   daemon's command catalog read and the daemon-effect command executor
#   (ADR-0052). They are named commands, never a generic request passthrough,
#   so they are deliberate additions to this baseline in the same change that
#   adds them.
# * M007-E adds the fifteen `desktop_document_*` commands: the editor's
#   bounded document operations, which the host owns and the renderer names.
# * M001 adds `desktop_document_diagnostics_subscribe` and
#   `desktop_document_diagnostics_unsubscribe`: the bounded loss-aware LSP
#   stream lifecycle, kept as two named read/cleanup commands.
# * M007-F adds the named LSP preview and checked-apply commands for the
#   desktop editor's review surface.
#
# The baseline was re-measured once all four additions were present: 44
# targets, and the same 44 the host's `generate_handler!` table registers.
# The sets are disjoint and the union is what the guard holds.
RECORDED_BRIDGE_TARGETS = frozenset(
    {
        "desktop_artifact_read",
        "desktop_command_catalog",
        "desktop_command_execute",
        "desktop_connect",
        "desktop_connection_snapshot",
        "desktop_control_refresh",
        "desktop_disconnect",
        "desktop_document_close",
        "desktop_document_completion",
        "desktop_document_cursor",
        "desktop_document_diagnostics",
        "desktop_document_diagnostics_subscribe",
        "desktop_document_diagnostics_unsubscribe",
        "desktop_document_edit",
        "desktop_document_navigate",
        "desktop_document_open",
        "desktop_document_preview",
        "desktop_document_preview_apply",
        "desktop_document_redo",
        "desktop_document_reload",
        "desktop_document_resync",
        "desktop_document_save",
        "desktop_document_state",
        "desktop_document_tree",
        "desktop_document_undo",
        "desktop_document_viewport",
        "desktop_git_read",
        "desktop_permission_respond",
        "desktop_project_detail",
        "desktop_project_list",
        "desktop_projection_current",
        "desktop_projection_start",
        "desktop_projection_stop",
        "desktop_projection_subscribe",
        "desktop_prompt_submit",
        "desktop_question_respond",
        "desktop_session_create",
        "desktop_session_list",
        "desktop_session_open",
        "desktop_skill_catalogue",
        "desktop_subscribe_events",
        "desktop_todo_refresh",
        "desktop_unsubscribe_events",
        "desktop_workspace_select",
    }
)

# The lanes this guard has to run in.
GUARD_REL = "scripts/check_desktop_panel_authority.py"
VERIFY_SH_REL = Path("scripts/verify.sh")
CI_WORKFLOW_REL = Path(".github/workflows/ci.yml")

# `python3 scripts/check_desktop_panel_authority.py` — how both lanes invoke
# it.
GUARD_INVOCATION = re.compile(r"python3\s+" + re.escape(GUARD_REL))

# The renderer modules this guard reads. The desktop renderer is TypeScript.
RENDERER_SUFFIXES = (".ts", ".tsx")

# A test module is not bundled into the renderer.
TEST_SUFFIXES = (".test.ts", ".test.tsx", ".spec.ts", ".spec.tsx")

# ── Reading a renderer module ───────────────────────────────────────────


def strip_comments(text: str) -> str:
    """Blank out the comments in a JS/TS source, keeping every offset.

    A comment that says "no filesystem path crosses the boundary" must not
    be read as a filesystem call, and a `//` inside a URL string must not
    start a comment. The scanner therefore tracks string literals: a comment
    only begins outside a string, and every blanked character becomes a
    space, so the line numbering of the original survives.
    """
    out = list(text)
    index = 0
    length = len(text)
    quote: str | None = None
    while index < length:
        ch = text[index]
        if quote is not None:
            if ch == "\\":
                index += 2
                continue
            if ch == quote:
                quote = None
            index += 1
            continue
        if ch in "'\"`":
            quote = ch
            index += 1
            continue
        if ch == "/" and index + 1 < length:
            nxt = text[index + 1]
            if nxt == "/":
                cursor = index
                while cursor < length and text[cursor] != "\n":
                    out[cursor] = " "
                    cursor += 1
                index = cursor
                continue
            if nxt == "*":
                cursor = index
                while cursor < length and not (
                    text[cursor] == "*" and cursor + 1 < length and text[cursor + 1] == "/"
                ):
                    if text[cursor] != "\n":
                        out[cursor] = " "
                    cursor += 1
                for position in range(index, min(cursor + 2, length)):
                    if text[position] != "\n":
                        out[position] = " "
                index = cursor + 2
                continue
        index += 1
    return "".join(out)


# The module specifiers a source names.
_SPECIFIER_PATTERNS = (
    re.compile(r"""\bfrom\s*['"]([^'"]+)['"]"""),
    re.compile(r"""\brequire\s*\(\s*['"]([^'"]+)['"]\s*\)"""),
    re.compile(r"""\bimport\s*\(\s*['"]([^'"]+)['"]\s*\)"""),
)


def module_specifiers(code: str) -> list[tuple[str, int]]:
    """Every module specifier in `code`, with its offset."""
    found: list[tuple[str, int]] = []
    for pattern in _SPECIFIER_PATTERNS:
        for match in pattern.finditer(code):
            found.append((match.group(1), match.start()))
    return found


# ── The patterns each rule reads ────────────────────────────────────────

# `invoke(...)`, `invoke<T>(...)`. A member access is not a bare call — a
# panel may legitimately receive a callback prop — so a call through a known
# authority handle is caught by `_AUTHORITY_INVOKE` instead.
_INVOKE_CALL = re.compile(r"(?<![\w.])invoke\s*(?:<[^<>()]*>)?\s*\(")

# A call through a handle that *is* an authority.
_AUTHORITY_INVOKE = re.compile(r"(?<![\w])(?:bridge|tauri)\s*\.\s*invoke\s*\(")

# The target of an `invoke` call.
_INVOKE_TARGET = re.compile(r"invoke\s*(?:<[^<>()]*>)?\s*\(\s*['\"]([A-Za-z0-9_]+)['\"]")

# Any bridge target name.
_BRIDGE_TOKEN = re.compile(r"\bdesktop_[a-z0-9_]+\b")

# The host's command table.
_HANDLER_BLOCK = re.compile(r"generate_handler!\[(.*?)\]", re.DOTALL)
_HANDLER_NAME = re.compile(r"\b([a-z][a-z0-9_]*)\b")

# A process launcher. The `.` is excluded so `RegExp.exec(...)` is not read
# as a process launch.
_PROCESS_CALL = re.compile(
    r"(?<![\w.$])(?:exec|execSync|execFile|execFileSync|spawn|spawnSync|fork)\s*\("
)

# The Deno namespace.
_DENO_CALL = re.compile(r"\bDeno\s*\.")

# A WebSocket.
_WEBSOCKET = re.compile(r"\bWebSocket\b")

# A fetch, and the absolute URL it may name.
_FETCH_CALL = re.compile(r"(?<![\w.$])fetch\s*\(")
_URL_LITERAL = re.compile(r"""['"](https?://([^'"/]+)(?:/[^'"]*)?)['"]""")

# Hosts a local fetch may name.
LOCAL_HOSTS = frozenset({"localhost", "127.0.0.1", "0.0.0.0", "::1", "[::1]"})

# The Node modules that are a filesystem or a process.
FILESYSTEM_MODULES = frozenset(
    {"fs", "node:fs", "fs/promises", "node:fs/promises"}
)
PROCESS_MODULES = frozenset(
    {"child_process", "node:child_process", "node:process"}
)

# The Tauri plugin prefixes a permission string can carry.
TAURI_PERMISSION_PREFIXES = frozenset(
    {
        "core",
        "shell",
        "fs",
        "process",
        "http",
        "updater",
        "clipboard-manager",
        "global-shortcut",
        "os",
        "dialog",
        "notification",
        "opener",
        "deep-link",
        "window",
        "webview",
    }
)
_PERMISSION_STRING = re.compile(r"""['"]([a-z][a-z0-9-]*):([a-z][a-z0-9-]*)['"]""")

# A capability or permission list declared in code.
_CAPABILITY_DECLARATION = re.compile(r"(?<![\w$])(?:capabilities|permissions)\s*:\s*[\[{]")

# The Tauri global the renderer may reach for when no import is needed.
_TAURI_GLOBAL = re.compile(r"\b__TAURI")

# The Rust-side plugin identifiers, for a module that names one.
_RUST_PLUGIN = re.compile(r"\btauri[-_]plugin[-_][a-z0-9_]+")

# The durable frontend-owned stores.
_DURABLE_STORE = re.compile(r"(?<![\w])(?:localStorage|sessionStorage|indexedDB)\b")

# A cookie write.
_COOKIE_WRITE = re.compile(r"\bdocument\s*\.\s*cookie\s*=(?!=)")


# ── The controls ─────────────────────────────────────────────────────────


def _line_of(code: str, offset: int) -> int:
    """The 1-based line number of an offset in `code`."""
    return code.count("\n", 0, offset) + 1


def _host_of(url: str) -> str:
    """The host an absolute URL names, without its port."""
    authority = url.split("://", 1)[-1].split("/", 1)[0]
    if authority.startswith("["):
        return authority.split("]", 1)[0] + "]"
    return authority.split(":", 1)[0]


def _is_test_module(path: Path) -> bool:
    """Whether a renderer module is a test module."""
    if path.name.endswith(TEST_SUFFIXES):
        return True
    return any(part == "__tests__" for part in path.parts)


def _compare_baseline(where: Path, measured: set[str]) -> list[str]:
    """Compare a measured bridge target set with the recorded baseline."""
    findings: list[str] = []
    added = sorted(measured - RECORDED_BRIDGE_TARGETS)
    removed = sorted(RECORDED_BRIDGE_TARGETS - measured)
    if added:
        findings.append(
            f"{where}: the bridge names {len(added)} target(s) outside the "
            f"recorded desktop bridge baseline: {', '.join(added)}. The recorded "
            f"baseline is {len(RECORDED_BRIDGE_TARGETS)} targets; a new bridge "
            "target is a deliberate edit to this guard, in the same change that "
            "adds it."
        )
    if removed:
        findings.append(
            f"{where}: {len(removed)} recorded bridge target(s) are gone: "
            f"{', '.join(removed)}. The recorded desktop bridge baseline is "
            f"{len(RECORDED_BRIDGE_TARGETS)} targets, and a removed one is a "
            "deliberate edit to this guard."
        )
    return findings


def check_bridge_baseline(root: Path) -> list[str]:
    """G1 — the recorded bridge baseline still matches the bridge."""
    findings: list[str] = []

    bridge = root / BRIDGE_MODULE
    if not bridge.is_file():
        findings.append(
            f"{BRIDGE_MODULE}: the renderer's bridge surface is missing, so the "
            "recorded bridge baseline cannot be proved."
        )
    else:
        code = strip_comments(bridge.read_text(encoding="utf-8"))
        measured = {match.group(1) for match in _INVOKE_TARGET.finditer(code)}
        findings.extend(_compare_baseline(BRIDGE_MODULE, measured))

    host = root / HOST_COMMAND_TABLE
    if not host.is_file():
        findings.append(
            f"{HOST_COMMAND_TABLE}: the desktop host's command table cannot be "
            "read, so the recorded bridge baseline cannot be proved against the "
            "commands the host actually registers."
        )
    else:
        text = strip_comments(host.read_text(encoding="utf-8"))
        block = _HANDLER_BLOCK.search(text)
        if block is None:
            findings.append(
                f"{HOST_COMMAND_TABLE}: the desktop host registers no "
                "`generate_handler!` table, so the recorded bridge baseline "
                "cannot be proved against the commands the host actually "
                "registers."
            )
        else:
            measured = set(_HANDLER_NAME.findall(block.group(1)))
            findings.extend(_compare_baseline(HOST_COMMAND_TABLE, measured))

    return findings


def _fetch_argument(code: str, start: int) -> str:
    """The argument of a `fetch` call, from just after its `(`."""
    depth = 1
    index = start
    while index < len(code) and depth > 0 and index - start < 200:
        if code[index] == "(":
            depth += 1
        elif code[index] == ")":
            depth -= 1
            if depth == 0:
                break
        index += 1
    return code[start:index]


def check_module(path: Path, root: Path) -> list[str]:
    """Every rule, against one renderer module."""
    rel = path.relative_to(root).as_posix()
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        return [f"{rel}: the module could not be read: {error}"]
    code = strip_comments(text)
    findings: list[str] = []

    # G2 — no bridge call.
    for match in _INVOKE_CALL.finditer(code):
        findings.append(
            f"{rel}:{_line_of(code, match.start())}: the panel layer calls "
            "`invoke`. The panels render the props they are given; they do not "
            "fetch. A panel that needs daemon data names the existing variant "
            "or stops."
        )
    for match in _AUTHORITY_INVOKE.finditer(code):
        findings.append(
            f"{rel}:{_line_of(code, match.start())}: the panel layer calls "
            "`invoke` through an authority handle. The panels render the props "
            "they are given; they do not fetch."
        )
    for spec, offset in module_specifiers(code):
        leaf = spec.rsplit("/", 1)[-1]
        if leaf == "bridge":
            findings.append(
                f"{rel}:{_line_of(code, offset)}: the panel layer imports the "
                f"bridge module (`{spec}`). The bridge belongs to the "
                "composition root, and a panel that imports it can fetch."
            )
        if spec.startswith("@tauri-apps/api"):
            findings.append(
                f"{rel}:{_line_of(code, offset)}: the panel layer imports the "
                f"Tauri API (`{spec}`). The renderer holds no Tauri capability, "
                "and a capability is what ADR-0010's containment refuses."
            )
        if spec.startswith("@tauri-apps/plugin-"):
            plugin = spec.split("@tauri-apps/plugin-", 1)[1]
            findings.append(
                f"{rel}:{_line_of(code, offset)}: the panel layer imports the "
                f"Tauri plugin `{plugin}` (`{spec}`). The renderer holds no "
                "Tauri capability."
            )
        if spec in FILESYSTEM_MODULES or leaf in {"fs", "fs/promises"}:
            findings.append(
                f"{rel}:{_line_of(code, offset)}: the panel layer imports the "
                f"filesystem module (`{spec}`). A panel reads the projection it "
                "is given; it does not read the disk."
            )
        if spec in PROCESS_MODULES or leaf in {"child_process"}:
            findings.append(
                f"{rel}:{_line_of(code, offset)}: the panel layer imports the "
                f"process module (`{spec}`). The renderer launches no process."
            )
    for match in _BRIDGE_TOKEN.finditer(code):
        target = match.group(0)
        if target not in RECORDED_BRIDGE_TARGETS:
            findings.append(
                f"{rel}:{_line_of(code, match.start())}: the panel layer names "
                f"the bridge target `{target}`, which is not in the recorded "
                "bridge baseline. A panel layer adds no bridge target."
            )

    # G3 — no Tauri capability.
    for match in _PERMISSION_STRING.finditer(code):
        prefix = match.group(1)
        if prefix not in TAURI_PERMISSION_PREFIXES:
            continue
        findings.append(
            f"{rel}:{_line_of(code, match.start())}: the panel layer names the "
            f"Tauri permission `{match.group(1)}:{match.group(2)}`. The renderer "
            "holds no Tauri capability."
        )
    for match in _CAPABILITY_DECLARATION.finditer(code):
        findings.append(
            f"{rel}:{_line_of(code, match.start())}: the panel layer declares a "
            "Tauri capability or permission list. The renderer holds no Tauri "
            "capability."
        )
    for match in _TAURI_GLOBAL.finditer(code):
        findings.append(
            f"{rel}:{_line_of(code, match.start())}: the panel layer "
            "reaches for the Tauri global (`__TAURI`). The renderer holds no "
            "Tauri capability."
        )
    for match in _RUST_PLUGIN.finditer(code):
        findings.append(
            f"{rel}:{_line_of(code, match.start())}: the panel layer names the "
            f"Tauri plugin `{match.group(0)}`. The renderer holds no Tauri "
            "capability."
        )

    # G4 — no filesystem, shell or process call.
    for match in _PROCESS_CALL.finditer(code):
        findings.append(
            f"{rel}:{_line_of(code, match.start())}: the panel layer calls a "
            f"process API (`{match.group(0).rstrip('(').strip()}`). The "
            "renderer launches no process."
        )
    for match in _DENO_CALL.finditer(code):
        findings.append(
            f"{rel}:{_line_of(code, match.start())}: the panel layer calls the "
            "`Deno` namespace. The renderer runs under the WebView, not Deno."
        )
    for match in _WEBSOCKET.finditer(code):
        findings.append(
            f"{rel}:{_line_of(code, match.start())}: the panel layer opens a "
            "`WebSocket`. The renderer talks to the daemon over the reviewed "
            "bridge, not over a socket of its own."
        )
    for match in _FETCH_CALL.finditer(code):
        argument = _fetch_argument(code, match.end())
        for url in _URL_LITERAL.finditer(argument):
            host = _host_of(url.group(2))
            if host.lower() in LOCAL_HOSTS:
                continue
            findings.append(
                f"{rel}:{_line_of(code, match.start())}: the panel layer fetches "
                f"the remote origin `{host}`. The renderer's CSP is local-only, "
                "and a panel fetches nothing."
            )

    # G5 — no durable frontend-owned store.
    for match in _DURABLE_STORE.finditer(code):
        findings.append(
            f"{rel}:{_line_of(code, match.start())}: the panel layer touches "
            f"`{match.group(0)}`. Panel placement stays renderer-local; a "
            "durable frontend-owned store is a storage surface this milestone "
            "has no migration for."
        )
    for match in _COOKIE_WRITE.finditer(code):
        findings.append(
            f"{rel}:{_line_of(code, match.start())}: the panel layer writes a "
            "document cookie. Panel placement stays renderer-local; a durable "
            "frontend-owned store is a storage surface this milestone has no "
            "migration for."
        )

    return findings


def check_panel_layer(root: Path) -> list[str]:
    """G2–G5 — the panel and region layers, module by module."""
    findings: list[str] = []
    for relative in PANEL_LAYER_DIRS:
        directory = root / relative
        if not directory.is_dir():
            # An absent layer holds no authority. A violating module that
            # appears in it is caught by the same scan.
            continue
        for path in sorted(directory.rglob("*")):
            if not path.is_file() or path.suffix not in RENDERER_SUFFIXES:
                continue
            if _is_test_module(path):
                continue
            findings.extend(check_module(path, root))
    return findings


def _run_guards_body(verify_sh: str) -> str:
    """The body of `run_guards()` in `scripts/verify.sh`.

    The guard list lives in that function and must not be duplicated
    elsewhere, so the wiring check reads the function itself.
    """
    lines = verify_sh.splitlines()
    collected: list[str] = []
    inside = False
    for line in lines:
        if not inside:
            if line.startswith("run_guards()"):
                inside = True
            continue
        if line.startswith("}"):
            break
        collected.append(line)
    return "\n".join(collected)


def check_wiring(root: Path) -> list[str]:
    """G6 — this guard is wired into both automatic lanes."""
    findings: list[str] = []
    verify_sh = root / VERIFY_SH_REL
    if not verify_sh.is_file():
        findings.append(
            f"{VERIFY_SH_REL}: the file that wires the guards cannot be read, "
            "so this guard may not be wired at all."
        )
    else:
        body = _run_guards_body(verify_sh.read_text(encoding="utf-8"))
        if not GUARD_INVOCATION.search(body):
            findings.append(
                f"{VERIFY_SH_REL}: `run_guards()` does not invoke `{GUARD_REL}`. "
                "A guard that is not run automatically protects nothing."
            )
    ci_workflow = root / CI_WORKFLOW_REL
    if not ci_workflow.is_file():
        findings.append(
            f"{CI_WORKFLOW_REL}: the CI workflow cannot be read, so this guard "
            "may not run in CI at all."
        )
    elif not GUARD_INVOCATION.search(ci_workflow.read_text(encoding="utf-8")):
        findings.append(
            f"{CI_WORKFLOW_REL}: no step invokes `{GUARD_REL}`. A guard that CI "
            "does not run protects nothing."
        )
    return findings


def check_tree(root: Path) -> list[str]:
    """Every control, against the tree under `root`."""
    findings: list[str] = []
    findings.extend(check_bridge_baseline(root))
    findings.extend(check_panel_layer(root))
    findings.extend(check_wiring(root))
    return findings


# ── The self-test fixture ────────────────────────────────────────────────

FIXTURE_PANEL = """import type { TodoListView } from '../bridge-types';

export function TaskPanel({ todos }: { todos: TodoListView }) {
  return (
    <section className="surface" data-testid="task-surface">
      <ul>
        {todos.items.map((item) => (
          <li key={item.id}>{item.content}</li>
        ))}
      </ul>
    </section>
  );
}
"""

FIXTURE_REGION = """import type { TranscriptMessageView } from '../bridge-types';

export function ChatRegion({ messages }: { messages: TranscriptMessageView[] }) {
  return (
    <section className="surface" data-testid="chat-surface">
      <ul>
        {messages.map((message) => (
          <li key={message.messageId}>{message.text}</li>
        ))}
      </ul>
    </section>
  );
}
"""

FIXTURE_VERIFY_SH = """#!/usr/bin/env bash
run_guards() {
    echo "==> python3 scripts/check_desktop_panel_authority.py"
    (cd "$REPO_ROOT" && python3 scripts/check_desktop_panel_authority.py)
}
"""

FIXTURE_CI_WORKFLOW = """name: CI

on:
  pull_request:

jobs:
  verify:
    runs-on: ubuntu-latest
    steps:
      - name: Desktop panel authority guard
        run: python3 scripts/check_desktop_panel_authority.py
"""


def _fixture_bridge() -> str:
    """A bridge surface that names exactly the recorded targets."""
    names = sorted(RECORDED_BRIDGE_TARGETS)
    lines = ["import { invoke } from '@tauri-apps/api/core';", "", "export const bridge = {"]
    for index, name in enumerate(names):
        comma = "," if index < len(names) - 1 else ""
        lines.append(f"  call{index}: () => invoke('{name}'){comma}")
    lines.append("};")
    return "\n".join(lines) + "\n"


def _fixture_host() -> str:
    """A host command table that registers exactly the recorded targets."""
    names = ", ".join(sorted(RECORDED_BRIDGE_TARGETS))
    return (
        "fn build() -> tauri::Builder {\n"
        "    tauri::Builder::default().invoke_handler(tauri::generate_handler![\n"
        f"        {names}\n"
        "    ])\n"
        "}\n"
    )


def _write_fixture(root: Path, with_regions: bool = True) -> None:
    """A compliant renderer tree under `root`."""
    bridge = root / BRIDGE_MODULE
    bridge.parent.mkdir(parents=True, exist_ok=True)
    bridge.write_text(_fixture_bridge(), encoding="utf-8")
    host = root / HOST_COMMAND_TABLE
    host.parent.mkdir(parents=True, exist_ok=True)
    host.write_text(_fixture_host(), encoding="utf-8")
    panel = root / PANEL_LAYER_DIRS[0] / "TaskPanel.tsx"
    panel.parent.mkdir(parents=True, exist_ok=True)
    panel.write_text(FIXTURE_PANEL, encoding="utf-8")
    if with_regions:
        region = root / PANEL_LAYER_DIRS[1] / "ChatRegion.tsx"
        region.parent.mkdir(parents=True, exist_ok=True)
        region.write_text(FIXTURE_REGION, encoding="utf-8")
    verify_sh = root / VERIFY_SH_REL
    verify_sh.parent.mkdir(parents=True, exist_ok=True)
    verify_sh.write_text(FIXTURE_VERIFY_SH, encoding="utf-8")
    ci = root / CI_WORKFLOW_REL
    ci.parent.mkdir(parents=True, exist_ok=True)
    ci.write_text(FIXTURE_CI_WORKFLOW, encoding="utf-8")


def _copy_fixture(root: Path, destination: Path) -> Path:
    """A copy of the fixture tree, so a case cannot leak into the next."""
    shutil.copytree(root, destination)
    return destination


def _append(path: Path, text: str) -> None:
    """Append text to a file."""
    with path.open("a", encoding="utf-8") as handle:
        handle.write(text)


def _replace(path: Path, old: str, new: str) -> None:
    """Replace `old` with `new` in `path`, once."""
    text = path.read_text(encoding="utf-8")
    if old not in text:
        raise SystemExit(f"self-test could not find the anchor text in {path}: {old!r}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def _self_test() -> int:
    """Prove the guard rejects each defect class it is meant to catch."""
    failures: list[str] = []
    with tempfile.TemporaryDirectory(prefix="codegg-desktop-panel-") as raw:
        fixture = Path(raw) / "fixture"
        _write_fixture(fixture)

        findings = check_tree(fixture)
        if findings:
            failures.append(
                "the guard did not accept a compliant renderer: " + "; ".join(findings)
            )
            _report(failures)
            return 1
        print("  ok: a compliant renderer is accepted")

        # The region layer may not exist yet. An absent layer is not a
        # finding.
        with tempfile.TemporaryDirectory(prefix="codegg-desktop-panel-noregion-") as raw_absent:
            absent = Path(raw_absent) / "fixture"
            _write_fixture(absent, with_regions=False)
            absent_findings = check_tree(absent)
            if absent_findings:
                failures.append(
                    "the guard did not accept a renderer whose region layer does "
                    "not exist yet: " + "; ".join(absent_findings)
                )
                _report(failures)
                return 1
            print("  ok: a renderer without the region layer is accepted")

        cases: list[tuple[str, str, str]] = [
            (
                "a panel module that calls `invoke`",
                "calls `invoke`",
                "",
            ),
            (
                "a panel module that calls `invoke` through an authority handle",
                "calls `invoke` through an authority handle",
                "",
            ),
            (
                "a panel module that imports the bridge module",
                "imports the bridge module",
                "",
            ),
            (
                "a region module that names a bridge target outside the baseline",
                "which is not in the recorded bridge baseline",
                "",
            ),
            (
                "a panel module that imports the Tauri API",
                "imports the Tauri API",
                "",
            ),
            (
                "a panel module that names a Tauri permission",
                "names the Tauri permission",
                "",
            ),
            (
                "a region module that imports the filesystem module",
                "imports the filesystem module",
                "",
            ),
            (
                "a region module that calls a process API",
                "calls a process API",
                "",
            ),
            (
                "a region module that fetches a remote origin",
                "fetches the remote origin",
                "",
            ),
            (
                "a region module that opens a WebSocket",
                "opens a `WebSocket`",
                "",
            ),
            (
                "a panel module that writes a durable store",
                "touches `localStorage`",
                "",
            ),
            (
                "a bridge surface that grew a target",
                "the recorded desktop bridge baseline",
                "",
            ),
            (
                "a host command table that grew a command",
                "the recorded desktop bridge baseline",
                "",
            ),
            (
                "a guard that `quick` does not run",
                "`run_guards()` does not invoke",
                "",
            ),
            (
                "a guard that CI does not run",
                "no step invokes",
                "",
            ),
        ]

        for label, needle, second_needle in cases:
            with tempfile.TemporaryDirectory(prefix="codegg-desktop-panel-case-") as raw_case:
                case = _copy_fixture(fixture, Path(raw_case) / "case")
                panel = case / PANEL_LAYER_DIRS[0] / "TaskPanel.tsx"
                region = case / PANEL_LAYER_DIRS[1] / "ChatRegion.tsx"

                if label == "a panel module that calls `invoke`":
                    _append(panel, "\nconst snapshot = () => invoke('desktop_connect');\n")
                elif label == (
                    "a panel module that calls `invoke` through an authority handle"
                ):
                    _append(panel, "\nconst snapshot = () => bridge.invoke('desktop_connect');\n")
                elif label == "a panel module that imports the bridge module":
                    _append(panel, "\nimport { bridge } from '../bridge';\n")
                elif label == (
                    "a region module that names a bridge target outside the baseline"
                ):
                    _append(region, "\nconst target = 'desktop_panel_placeholder';\n")
                elif label == "a panel module that imports the Tauri API":
                    _append(panel, "\nimport { invoke as tauriInvoke } from '@tauri-apps/api/core';\n")
                elif label == "a panel module that names a Tauri permission":
                    _append(panel, "\nconst granted = 'shell:allow-execute';\n")
                elif label == "a region module that imports the filesystem module":
                    _append(region, "\nimport { readFileSync } from 'node:fs';\n")
                elif label == "a region module that calls a process API":
                    _append(region, "\nconst child = spawn('sh', ['-c', 'ls']);\n")
                elif label == "a region module that fetches a remote origin":
                    _append(region, "\nconst response = await fetch('https://example.com/data');\n")
                elif label == "a region module that opens a WebSocket":
                    _append(region, "\nconst socket = new WebSocket('wss://example.com');\n")
                elif label == "a panel module that writes a durable store":
                    _append(panel, "\nwindow.localStorage.setItem('panel', 'wide');\n")
                elif label == "a bridge surface that grew a target":
                    _append(
                        case / BRIDGE_MODULE,
                        "\nconst extra = () => invoke('desktop_panel_extra');\n",
                    )
                elif label == "a host command table that grew a command":
                    _replace(
                        case / HOST_COMMAND_TABLE,
                        "desktop_workspace_select",
                        "desktop_workspace_select,\n        desktop_panel_extra",
                    )
                elif label == "a guard that `quick` does not run":
                    _replace(
                        case / VERIFY_SH_REL,
                        "    echo \"==> python3 scripts/check_desktop_panel_authority.py\"\n"
                        "    (cd \"$REPO_ROOT\" && python3 scripts/check_desktop_panel_authority.py)\n",
                        "",
                    )
                else:
                    _replace(
                        case / CI_WORKFLOW_REL,
                        "      - name: Desktop panel authority guard\n"
                        "        run: python3 scripts/check_desktop_panel_authority.py\n",
                        "",
                    )

                findings = check_tree(case)
                if not findings:
                    failures.append(f"the guard did not reject {label}")
                    continue
                if not any(needle in finding for finding in findings):
                    failures.append(
                        f"the guard rejected {label}, but not for the expected "
                        f"reason ({needle!r}): {findings}"
                    )
                    continue
                if second_needle and not any(second_needle in f for f in findings):
                    failures.append(
                        f"the guard rejected {label}, but did not report "
                        f"{second_needle!r}: {findings}"
                    )
                    continue
                print(f"  ok: rejected {label}")

        # A region module that appears in a tree whose region layer was
        # absent is caught by the same scan.
        with tempfile.TemporaryDirectory(prefix="codegg-desktop-panel-newregion-") as raw_new:
            new = Path(raw_new) / "case"
            _write_fixture(new, with_regions=False)
            region = new / PANEL_LAYER_DIRS[1] / "ChatRegion.tsx"
            region.parent.mkdir(parents=True, exist_ok=True)
            _append(region, FIXTURE_REGION)
            _append(region, "\nconst snapshot = () => invoke('desktop_connect');\n")
            findings = check_tree(new)
            if not any("calls `invoke`" in finding for finding in findings):
                failures.append(
                    "the guard did not reject a region module that appears in a "
                    "tree whose region layer was absent: " + "; ".join(findings)
                )
            else:
                print("  ok: rejected a region module that appears in an absent layer")

    if failures:
        _report(failures)
        return 1

    # The repository's own renderer is the guard's ordinary verdict, not the
    # self-test's: a finding there is a fact about the tree, and this
    # self-test must keep measuring the guard rather than the tree.
    findings = check_tree(ROOT)
    if findings:
        print(f"  note: the repository's own renderer has {len(findings)} finding(s):")
        for finding in findings:
            print(f"    {finding}")
    else:
        print("  ok: the repository's own renderer holds no panel-layer authority")
    print("desktop panel authority guard self-test passed")
    return 0


def _report(failures: list[str]) -> None:
    for failure in failures:
        print(f"ERROR: {failure}", file=sys.stderr)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard rejects each defect class it is meant to catch",
    )
    args = parser.parse_args()
    if args.self_test:
        return _self_test()
    findings = check_tree(ROOT)
    if findings:
        for finding in findings:
            print(f"ERROR: {finding}", file=sys.stderr)
        print(f"{len(findings)} desktop panel authority violation(s)", file=sys.stderr)
        return 1
    print("desktop panel authority guard passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
