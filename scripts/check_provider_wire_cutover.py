#!/usr/bin/env python3
"""Keep standard provider grammar and stream ownership in the shared wire bridge."""

from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
PROVIDERS = ROOT / "crates/codegg-providers/src"
MIGRATED = ("openai.rs", "openai_compatible.rs", "azure.rs", "openrouter.rs", "opencode_zen.rs")
FAMILIES = {"anthropic.rs": "AnthropicMessagesSse", "google.rs": "GeminiGenerateContentSse"}
FORBIDDEN = (
    "parse_openai_buffer", "parse_openai_chunk", "struct OpenAiToolState", "parse_openai_line",
    "parse_anthropic_buffer", "parse_google_buffer", "parse_google_chunk",
)


def main() -> int:
    failures = []
    for name in MIGRATED:
        source = (PROVIDERS / name).read_text()
        if "crate::wire::encode_openai_chat" not in source:
            failures.append(f"{name} does not encode through the shared OpenAI Chat bridge")
        if "crate::wire::openai_chat_stream" not in source:
            failures.append(f"{name} does not decode through the shared OpenAI Chat bridge")
    for name, adapter in FAMILIES.items():
        source = (PROVIDERS / name).read_text()
        if "crate::wire::shared_stream" not in source:
            failures.append(f"{name} does not use the shared stream bridge")
        if adapter not in source:
            failures.append(f"{name} does not select {adapter}")
    for path in (*(PROVIDERS / name for name in MIGRATED), *(PROVIDERS / name for name in FAMILIES)):
        source = path.read_text()
        for marker in FORBIDDEN:
            if marker in source:
                failures.append(f"{path.relative_to(ROOT)} retains duplicate OpenAI parser marker {marker!r}")
    if failures:
        sys.stderr.write("\n".join(failures) + "\n")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
