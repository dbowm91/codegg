#!/usr/bin/env python3
"""Tool-disclosure wire honesty guard (M004).

Provider-native tool search must be qualified, never inferred:

- per-turn disclosure uses the qualified `ToolDisclosureWireMode`
  (`qualify_tool_disclosure`), not a generic capability flag;
- native allowlists stay empty until a model/API combination completes
  byte-level + two-turn live qualification;
- native emission always carries its required provider-owned
  built-in/search variant and never confuses it with CodeGG's host
  `tool_search` function;
- unknown providers and aliases can never enable a native mode.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def read(rel: str) -> str:
    return (ROOT / rel).read_text()


def fail(message: str) -> int:
    print(f"FAIL: {message}")
    return 1


def main() -> int:
    failures = 0

    core = read("crates/codegg-providers/src/provider_core.rs")
    wire = read("crates/codegg-providers/src/wire.rs")
    responses = read("crates/codegg-providers/src/responses_api.rs")
    prep = read("src/agent/request_preparation.rs")

    # 1. Qualified mode representation exists with exact-match semantics.
    for token in (
        "enum ToolDisclosureWireMode",
        "FullCompatibility",
        "NativeOpenAiToolSearch",
        "NativeAnthropicToolSearch",
        "fn qualify_tool_disclosure",
        "fn qualify_tool_disclosure_with_allowlist",
    ):
        if token not in core:
            failures += fail(f"provider_core.rs lacks {token!r}")

    # 2. Native allowlists stay empty (gated until live-qualified).
    for constant in (
        "NATIVE_OPENAI_TOOL_SEARCH_MODELS",
        "NATIVE_ANTHROPIC_TOOL_SEARCH_MODELS",
    ):
        match = re.search(
            rf"{constant}:\s*&\[[^\]]*\]\s*=\s*&\[([^\]]*)\]", core
        )
        if not match or match.group(1).strip():
            failures += fail(f"{constant} must stay empty until live-qualified")

    # 3. Per-turn preparation qualifies the mode and reports it honestly.
    if "qualify_tool_disclosure" not in prep:
        failures += fail("request_preparation.rs does not qualify the disclosure mode")
    if "disclosure_mode" not in prep:
        failures += fail("request_preparation.rs lacks disclosure-mode telemetry")

    # 4. Native emission carries its required built-in / variant and
    # keeps the host search function distinct.
    for token in (
        "apply_native_openai_tool_search",
        "apply_native_anthropic_tool_search",
        "is_provider_tool_search_entry",
        "ANTHROPIC_NATIVE_TOOL_SEARCH_VARIANT",
    ):
        if token not in wire:
            failures += fail(f"wire.rs lacks {token!r}")
    if "native_tool_search_entry" not in responses:
        failures += fail("responses_api.rs lacks the provider builtin constructor")

    # 5. The host `tool_search` function is never emitted as a
    # provider-owned entry: exactly one constructor site
    # (`json!({"type": "tool_search"})`, the builtin) may hold the
    # shape, and the DTO conversion never invents one.
    production = responses.split("#[cfg(test)]")[0]
    if production.count('json!({"type": "tool_search"})') != 1:
        failures += fail("responses_api.rs emits provider search entries outside the builtin")
    if "from_tool_definition" in responses and '"tool_search"' in responses.split(
        "fn from_tool_definition"
    )[1].split("}")[0]:
        failures += fail("from_tool_definition must not emit a search entry")

    if failures:
        print(f"{failures} disclosure-wire violation(s)")
        return 1
    print("OK: Tool disclosure wire honesty holds.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
