#!/usr/bin/env python3
"""Keep provider catalog/discovery authority converged (C002)."""

from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
PROVIDERS = ROOT / "crates/codegg-providers/src"


def main() -> int:
    failures = []
    setup = (PROVIDERS / "setup_catalog.rs").read_text()
    eggpool = (PROVIDERS / "eggpool.rs").read_text()
    compatible = (PROVIDERS / "openai_compatible.rs").read_text()

    if 'pub const OPENCODE_GO_BASE_URL: &str = "https://opencode.ai/zen/go/v1"' not in setup:
        failures.append("OPENCODE_GO_BASE_URL must be https://opencode.ai/zen/go/v1")
    if 'pub const TOGETHER_BASE_URL: &str = "https://api.together.xyz/v1"' not in setup:
        failures.append("TOGETHER_BASE_URL must remain https://api.together.xyz/v1")
    if "fn normalize_compatible_base_url" not in eggpool:
        failures.append("eggpool.rs must expose normalize_compatible_base_url()")
    if "pub struct CompatibleProbe" not in eggpool:
        failures.append("eggpool.rs must define provider-neutral CompatibleProbe")
    if "pub use CompatibleProbe as CompatibleModelsProbe" not in eggpool:
        failures.append("CompatibleModelsProbe must point at generic CompatibleProbe")
    if "fn parse_compatible_models_response" not in eggpool:
        failures.append("eggpool.rs must expose shared parse_compatible_models_response()")
    if "parse_compatible_models_response" not in compatible:
        failures.append("openai_compatible.rs must reuse the shared bounded parser")
    if "max_decoded_body_size" not in compatible:
        failures.append("best-effort models() must enforce the shared response-byte bound")
    if failures:
        sys.stderr.write("\n".join(failures) + "\n")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
