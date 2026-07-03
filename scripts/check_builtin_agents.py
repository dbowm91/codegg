#!/usr/bin/env python3
"""
Verify TOML agent definitions in assets/agents/ match hardcoded values
in src/agent/mod.rs builtin_agents().

Exit 0 if all match, 1 if any mismatch.
"""

import re
import sys
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
GENERATED_RS = REPO_ROOT / "src" / "agent" / "builtins" / "generated.rs"
AGENTS_DIR = REPO_ROOT / "assets" / "agents"
PROMPT_DIR = REPO_ROOT / "assets" / "prompts" / "agents"


# ---------------------------------------------------------------------------
# Rust source parsing
# ---------------------------------------------------------------------------

def _parse_rust_string(s: str) -> str:
    """Decode a Rust string literal body (without surrounding quotes).

    Handles \\n, \\t, \\\\, \\" and line-continuation (backslash at end of
    source line, which eats the newline and all leading whitespace on the
    next line).
    """
    out: list[str] = []
    i = 0
    while i < len(s):
        if s[i] == "\\" and i + 1 < len(s):
            c = s[i + 1]
            if c == "n":
                out.append("\n")
                i += 2
            elif c == "t":
                out.append("\t")
                i += 2
            elif c == "\\":
                out.append("\\")
                i += 2
            elif c == '"':
                out.append('"')
                i += 2
            elif c == "\n":
                # Line continuation: backslash + real newline.
                # Skip the backslash, the newline, and all following
                # whitespace (spaces / tabs / newlines).
                i += 2
                while i < len(s) and s[i] in " \t\n\r":
                    i += 1
            else:
                out.append(s[i])
                i += 1
        else:
            out.append(s[i])
            i += 1
    return "".join(out)


def _extract_blocks(text: str, start_marker: str) -> list[str]:
    """Return the raw bodies of all ``Thing { … }`` blocks after *start_marker*."""
    idx = text.find(start_marker)
    if idx == -1:
        raise ValueError(f"marker not found: {start_marker}")

    # Find opening brace of the vec![ / function body
    brace = text.index("{", idx)
    depth = 0
    end = brace
    for i in range(brace, len(text)):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                end = i
                break
    body = text[brace + 1 : end]

    blocks: list[str] = []
    pos = 0
    while True:
        marker = body.find("Agent {", pos)
        if marker == -1:
            break
        bopen = body.index("{", marker)
        depth = 0
        bclose = bopen
        for i in range(bopen, len(body)):
            if body[i] == "{":
                depth += 1
            elif body[i] == "}":
                depth -= 1
                if depth == 0:
                    bclose = i
                    break
        blocks.append(body[bopen + 1 : bclose])
        pos = bclose + 1
    return blocks


def _field_str(block: str, name: str) -> str | None:
    m = re.search(rf'{name}:\s*"((?:[^"\\]|\\.)*)"\.to_string\(\)', block)
    return m.group(1) if m else None


def _field_opt_str(block: str, name: str) -> str | None:
    """Extract Some(\"…\".to_string()) or None."""
    m = re.search(rf"{name}:\s*Some\(\s*\"((?:[^\"\\]|\\.)*)\"\.to_string\(\)\s*\)", block)
    if m:
        return m.group(1)
    if re.search(rf"{name}:\s*None", block):
        return None
    return None


def _field_bool(block: str, name: str) -> bool | None:
    m = re.search(rf"{name}:\s*(true|false)", block)
    return m.group(1) == "true" if m else None


def _field_mode(block: str) -> str | None:
    m = re.search(r"mode:\s*AgentMode::(\w+)", block)
    return m.group(1).lower() if m else None


def _field_f64(block: str, name: str) -> float | None:
    """Extract Some(1.0) or None for f64 fields."""
    m = re.search(rf"{name}:\s*Some\(\s*([\d.]+)\s*\)", block)
    if m:
        return float(m.group(1))
    if re.search(rf"{name}:\s*None", block):
        return None
    return None


def _field_usize(block: str, name: str) -> int | None:
    """Extract Some(N) or None for usize fields."""
    m = re.search(rf"{name}:\s*Some\(\s*(\d+)\s*\)", block)
    if m:
        return int(m.group(1))
    if re.search(rf"{name}:\s*None", block):
        return None
    return None


def _field_runtime_kind(block: str) -> str | None:
    """Extract AgentRuntimeKind::Variant or None, mapping to TOML snake_case."""
    # Map Rust CamelCase variants to TOML snake_case values
    RUST_TO_TOML = {
        "Standard": "standard",
        "SecurityReview": "security_review",
        "Research": "research",
        "Compaction": "compaction",
        "Title": "title",
        "Summary": "summary",
    }
    m = re.search(r"runtime_kind:\s*Some\(\s*AgentRuntimeKind::(\w+)\s*\)", block)
    if m:
        variant = m.group(1)
        return RUST_TO_TOML.get(variant, variant.lower())
    if re.search(r"runtime_kind:\s*None", block):
        return None
    return None


def _field_permissions(block: str) -> dict[str, str]:
    m = re.search(r"permissions:\s*HashMap::from\(\[(.*?)\]\)", block, re.DOTALL)
    if not m:
        return {}
    perms: dict[str, str] = {}
    for pm in re.finditer(
        r'\("([^"]+)"\.to_string\(\),\s*"([^"]+)"\.to_string\(\)\)', m.group(1)
    ):
        perms[pm.group(1)] = pm.group(2)
    return perms


def _field_system_prompt(block: str) -> str | None:
    """Extract system_prompt field – either the full multi-line ``Some(…)``
    literal or ``None``."""
    if re.search(r"system_prompt:\s*None", block):
        return None

    # The opening Some( may be followed by whitespace/newline then the quote.
    m = re.search(
        r"system_prompt:\s*Some\(\s*\n?\s*\"((?:[^\"\\]|\\.)*)\"",
        block,
        re.DOTALL,
    )
    if m:
        return _parse_rust_string(m.group(1))
    return None


def _parse_agent_block(block: str) -> dict:
    name = _field_str(block, "name") or ""
    return {
        "name": name,
        "role": _field_opt_str(block, "role"),
        "description": _field_str(block, "description") or "",
        "mode": _field_mode(block) or "primary",
        "hidden": _field_bool(block, "hidden") or False,
        "permissions": _field_permissions(block),
        "system_prompt": _field_system_prompt(block),
        "model": _field_opt_str(block, "model"),
        "fallback_model": _field_opt_str(block, "fallback_model"),
        "temperature": _field_f64(block, "temperature"),
        "steps": _field_usize(block, "steps"),
        "color": _field_opt_str(block, "color"),
        "runtime_kind": _field_runtime_kind(block),
    }


def parse_mod_rs() -> dict[str, dict]:
    """Return ``{name: agent_dict}`` for every agent in generated_builtin_agents()."""
    content = GENERATED_RS.read_text()
    blocks = _extract_blocks(content, "pub fn generated_builtin_agents()")
    agents = [_parse_agent_block(b) for b in blocks]
    return {a["name"]: a for a in agents}


# ---------------------------------------------------------------------------
# TOML parsing
# ---------------------------------------------------------------------------

def parse_toml_agents() -> dict[str, dict]:
    """Return ``{name: agent_dict}`` for every ``assets/agents/*.toml``."""
    agents: dict[str, dict] = {}
    for path in sorted(AGENTS_DIR.glob("*.toml")):
        with open(path, "rb") as f:
            data = tomllib.load(f)
        sec = data.get("agent", {})
        name = sec.get("name", path.stem)

        # Load prompt from .md file (same convention as the generator)
        prompt_content = None
        prompt_file_rel = sec.get("prompt_file")
        if prompt_file_rel:
            prompt_path = REPO_ROOT / "assets" / prompt_file_rel
        else:
            prompt_path = PROMPT_DIR / f"{name}.md"
        if prompt_path.is_file():
            text = prompt_path.read_text(encoding="utf-8").strip()
            # Strip heading
            text = re.sub(r"^#\s+\S+.*$", "", text, count=1, flags=re.MULTILINE).strip()
            lower = text.lower()
            default_patterns = ("uses the default", "no custom prompt", "default system prompt")
            if text and not any(pat in lower for pat in default_patterns):
                prompt_content = text

        # Map runtime_kind from TOML string to lowercase (matches Rust enum variant names)
        runtime_kind_raw = sec.get("runtime_kind")
        runtime_kind = runtime_kind_raw.lower() if runtime_kind_raw else None

        agents[name] = {
            "name": name,
            "role": sec.get("role"),
            "description": sec.get("description", ""),
            "mode": sec.get("mode", "Primary").lower(),
            "hidden": sec.get("hidden", False),
            "permissions": dict(sec.get("permissions", {})),
            "system_prompt": prompt_content,
            "model": sec.get("model"),
            "fallback_model": sec.get("fallback_model"),
            "temperature": sec.get("temperature"),
            "steps": sec.get("steps"),
            "color": sec.get("color"),
            "runtime_kind": runtime_kind,
            "_file": path.name,
        }
    return agents


# ---------------------------------------------------------------------------
# Comparison
# ---------------------------------------------------------------------------

def _diff(rust: dict, toml: dict) -> list[str]:
    """Return a list of human-readable mismatch strings."""
    diffs: list[str] = []

    def _cmp(field: str, r_val, t_val):
        if r_val != t_val:
            diffs.append(f"{field}: rust={r_val!r}  toml={t_val!r}")

    _cmp("name", rust["name"], toml["name"])
    _cmp("role", rust["role"], toml["role"])
    _cmp("description", rust["description"], toml["description"])
    _cmp("mode", rust["mode"], toml["mode"])
    _cmp("hidden", rust["hidden"], toml["hidden"])
    _cmp("model", rust["model"], toml["model"])
    _cmp("fallback_model", rust["fallback_model"], toml["fallback_model"])
    _cmp("temperature", rust["temperature"], toml["temperature"])
    _cmp("steps", rust["steps"], toml["steps"])
    _cmp("color", rust["color"], toml["color"])
    _cmp("runtime_kind", rust["runtime_kind"], toml["runtime_kind"])

    # Permissions – compare key-by-key for a readable diff
    rp, tp = rust["permissions"], toml["permissions"]
    if rp != tp:
        for k in sorted(set(rp) | set(tp)):
            rv, tv = rp.get(k), tp.get(k)
            if rv != tv:
                diffs.append(f"permissions.{k}: rust={rv!r}  toml={tv!r}")

    # system_prompt
    rp = rust.get("system_prompt")
    tp = toml.get("system_prompt")
    if rp is None and tp is not None:
        diffs.append("system_prompt: present in toml, missing in rust")
    elif rp is not None and tp is None:
        diffs.append("system_prompt: present in rust, missing in toml")
    elif rp is not None and tp is not None and rp != tp:
        diffs.append(
            f"system_prompt: content differs "
            f"(rust {len(rp)} chars, toml {len(tp)} chars)"
        )

    return diffs


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> int:
    rust_agents = parse_mod_rs()
    toml_agents = parse_toml_agents()

    all_names = sorted(set(rust_agents) | set(toml_agents))
    checked = 0
    mismatches = 0

    for name in all_names:
        in_rust = name in rust_agents
        in_toml = name in toml_agents

        if not in_toml:
            print(f"  {name}: MISMATCH (in mod.rs but not in TOML)")
            mismatches += 1
            checked += 1
            continue
        if not in_rust:
            src = toml_agents[name].get("_file", "?")
            print(f"  {name}: MISMATCH (in TOML [{src}] but not in mod.rs)")
            mismatches += 1
            checked += 1
            continue

        diffs = _diff(rust_agents[name], toml_agents[name])
        checked += 1
        if diffs:
            mismatches += 1
            print(f"  {name}: MISMATCH")
            for d in diffs:
                print(f"    - {d}")
        else:
            print(f"  {name}: OK")

    print(f"\n{checked} agents checked, {mismatches} mismatch(es)")
    return 1 if mismatches else 0


if __name__ == "__main__":
    sys.exit(main())
