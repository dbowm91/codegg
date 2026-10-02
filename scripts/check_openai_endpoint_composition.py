#!/usr/bin/env python3
"""Prevent native OpenAI endpoint composition from duplicating the version prefix."""

from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
OPENAI = ROOT / "crates/codegg-providers/src/openai.rs"


def main() -> int:
    source = OPENAI.read_text()
    failures = []
    # Old defect: format!("{}/v1/chat/completions", base_url) on top of an
    # already-versioned prefix. The composer must append only /chat/completions.
    if "{}/v1/chat/completions" in source or "{}/v1/chat/completions\"" in source:
        failures.append(
            "openai.rs reintroduces duplicated /v1 composition; use chat_completions_url()"
        )
    if "/chat/completions" not in source:
        failures.append("openai.rs must compose /chat/completions from the API prefix")
    if "fn chat_completions_url" not in source:
        failures.append("openai.rs must expose chat_completions_url() as the single composer")
    # Legacy vendor helpers duplicated mixed host-root/API-prefix semantics;
    # they must stay removed in favor of additional.rs compatible factories.
    for legacy in ("fn groq(", "fn xai(", "fn mistral(", "fn cerebras("):
        # Only flag definitions inside the OpenAiConfig impl, not test data.
        if f"pub {legacy}" in source or f"    pub {legacy}" in source:
            failures.append(f"openai.rs must not restore legacy helper {legacy}")
    if failures:
        sys.stderr.write("\n".join(failures) + "\n")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
