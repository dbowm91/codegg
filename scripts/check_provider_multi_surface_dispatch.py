#!/usr/bin/env python3
"""Guard the shared provider-profile and multi-surface dispatch contract (M011).

OpenCode Go serves one provider identity across three wire surfaces
(`/chat/completions`, `/responses`, `/messages`). Which surface a model uses is
not discoverable from the public `/models` response, so it comes from the shared
EggPool provider-profile contract. That split creates failure modes which
compile cleanly and are wrong at runtime, so this guard makes the load-bearing
invariants mechanically checkable instead of conventional:

1. The shared profile dependency must be pinned at an immutable revision, and
   `eggpool-wire` must be pinned at the *same* revision. The profile declares
   `eggpool-wire` as a path dependency, and Cargo resolves a git dependency's
   path deps against the parent's revision — so mismatched revs link two copies
   of `eggpool-wire` and give the workspace two `WireSurface` types, breaking the
   single wire-vocabulary invariant.
2. Multi-surface resolution must fail closed. An unresolved model must never be
   defaulted to Chat Completions, and no prefix/model-family inference may enter
   the resolution path.
3. Per-surface credential ownership must not regress: Messages uses
   `x-api-key`, Chat/Responses use Bearer, the stable OpenCode session header
   must remain, and a missing session must stay a local pre-network failure.
4. A logical request must resolve exactly one surface: re-resolving after a send
   would be speculative cross-surface negotiation, which the plan forbids.
5. The durable OpenCode Go base-URL mirror must stay pinned to the shared profile
   so a stored connection cannot name an endpoint the runtime never uses.
6. The direct stateless Responses bridge must not route through the hosted
   Responses program subsystem.

Run directly (`python3 scripts/check_provider_multi_surface_dispatch.py`) or with
`--self-test` to verify the guard itself still detects regressions.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

PROVIDERS_MANIFEST = REPO_ROOT / "crates" / "codegg-providers" / "Cargo.toml"
PROFILE_RS = REPO_ROOT / "crates" / "codegg-providers" / "src" / "provider_profile.rs"
OPENCODE_GO_RS = REPO_ROOT / "crates" / "codegg-providers" / "src" / "opencode_go.rs"
SETUP_CATALOG_RS = REPO_ROOT / "crates" / "codegg-providers" / "src" / "setup_catalog.rs"
WIRE_RS = REPO_ROOT / "crates" / "codegg-providers" / "src" / "wire.rs"


class GuardFailure(Exception):
    """A contract invariant was violated."""


def read(path: Path) -> str:
    if not path.exists():
        raise GuardFailure(f"required file is missing: {path.relative_to(REPO_ROOT)}")
    return path.read_text(encoding="utf-8")


def git_rev(manifest: str, crate: str) -> str | None:
    match = re.search(
        rf'^{re.escape(crate)}\s*=.*?rev\s*=\s*"([0-9a-f]{{7,40}})"',
        manifest,
        re.MULTILINE | re.DOTALL,
    )
    return match.group(1) if match else None


def check_single_wire_vocabulary() -> str:
    manifest = read(PROVIDERS_MANIFEST)
    wire_rev = git_rev(manifest, "eggpool-wire")
    profile_rev = git_rev(manifest, "eggpool-provider-profile")

    if wire_rev is None:
        raise GuardFailure(
            "eggpool-wire must stay a pinned git dependency; an unpinned or removed "
            "pin would let the shared profile and the wire kernel drift apart"
        )
    if profile_rev is None:
        raise GuardFailure("eggpool-provider-profile must be pinned at an immutable git revision")
    if wire_rev != profile_rev:
        raise GuardFailure(
            "eggpool-wire and eggpool-provider-profile must share one rev "
            f"(wire={wire_rev}, profile={profile_rev}). The profile declares eggpool-wire "
            "as a path dependency, which Cargo resolves against the parent's revision; "
            "mismatched revs link two copies of eggpool-wire and yield two WireSurface types."
        )
    return f"eggpool-wire and eggpool-provider-profile share rev {wire_rev}"


def check_resolution_fails_closed() -> str:
    source = read(PROFILE_RS)
    if "fn resolve_route" not in source:
        raise GuardFailure("provider_profile.rs must expose resolve_route()")

    body = source.split("pub fn resolve_route", 1)[1]
    body = body.split("\npub fn ", 1)[0]
    for pattern in (r"unwrap_or\w*\(\s*WireSurface::", r"\.or\(\s*WireSurface::"):
        if re.search(pattern, body):
            raise GuardFailure(
                "resolve_route must not fall back to any WireSurface "
                f"(matched {pattern!r}); an unresolved model must stay unresolved"
            )

    if not re.search(r"model_wire_preference\(\s*model\s*\)", source):
        raise GuardFailure(
            "resolution must use the shared profile's exact model_wire_preference lookup "
            "so prefix or model-family inference cannot creep in"
        )
    if "WireUnresolved" not in source:
        raise GuardFailure(
            "provider_profile.rs must keep an explicit WireUnresolved failure so "
            "resolution fails closed rather than guessing"
        )
    return "wire resolution fails closed with no surface default"


def check_per_surface_auth_ownership() -> str:
    source = read(OPENCODE_GO_RS)
    if 'const SESSION_HEADER: &str = "x-opencode-session";' not in source:
        raise GuardFailure("the multi-surface provider must keep the stable x-opencode-session header")
    if "RouteAuth::ApiKeyHeader" not in source:
        raise GuardFailure("the Messages credential shape (RouteAuth::ApiKeyHeader) must be applied explicitly")
    if "authorization_header_value" not in source:
        raise GuardFailure(
            "Chat/Responses must apply the Bearer credential via Credential::authorization_header_value"
        )
    if "missing_session_context" not in source:
        raise GuardFailure("a missing session context must remain a local, pre-network failure")
    return "per-surface auth and session header ownership are explicit"


def check_no_cross_surface_retry() -> str:
    source = read(OPENCODE_GO_RS)
    if "async fn stream" not in source:
        raise GuardFailure("opencode_go.rs must implement stream()")
    body = source.split("async fn stream", 1)[1]
    body = body.split("\n    /// ", 1)[0]
    if body.count("self.resolve(") > 1:
        raise GuardFailure(
            "stream() must resolve exactly one surface before any network I/O; "
            "re-resolving after a send would introduce speculative cross-surface retry"
        )
    return "stream() resolves exactly one surface before network I/O"


def check_base_url_mirror_is_pinned() -> str:
    catalog = read(SETUP_CATALOG_RS)
    if "OPENCODE_GO_BASE_URL" not in catalog:
        raise GuardFailure("setup_catalog.rs must keep the durable OpenCode Go base-URL mirror")
    if not re.search(r"fn opencode_go_base_url_mirrors_the_shared_profile", catalog):
        raise GuardFailure(
            "the durable base-URL mirror must be pinned to the shared profile by a test, so a "
            "stored connection cannot name an endpoint the runtime provider does not use"
        )
    return "durable base-URL mirror is pinned to the shared profile"


def check_direct_responses_is_stateless() -> str:
    wire = read(WIRE_RS)
    if not re.search(r"fn encode_openai_responses\b", wire):
        raise GuardFailure("wire.rs must expose the direct stateless Responses encoder")
    if "OpenaiResponsesSse" not in wire:
        raise GuardFailure(
            "direct Responses streaming must decode through the shared kernel's OpenaiResponsesSse adapter"
        )
    if "responses_api::" in wire:
        raise GuardFailure(
            "the direct stateless Responses bridge must not call the hosted/stateful responses_api subsystem"
        )
    return "direct Responses stays stateless and off the hosted program path"


CHECKS = [
    ("shared profile + wire kernel resolve to one crate", check_single_wire_vocabulary),
    ("unresolved models fail closed (no surface default)", check_resolution_fails_closed),
    ("per-surface auth and session header ownership", check_per_surface_auth_ownership),
    ("no speculative cross-surface retry", check_no_cross_surface_retry),
    ("durable base-URL mirror pinned to shared profile", check_base_url_mirror_is_pinned),
    ("direct Responses is stateless, not the hosted program path", check_direct_responses_is_stateless),
]


def run(verbose: bool = False) -> list[str]:
    notes = []
    for index, (label, check) in enumerate(CHECKS, start=1):
        try:
            notes.append(check())
        except GuardFailure as failure:
            raise GuardFailure(f"[{index}/{len(CHECKS)}] {label}: {failure}") from failure
        if verbose:
            print(f"  ok {index}/{len(CHECKS)} {label}")
    return notes


def self_test() -> int:
    """Inject regressions in place, confirm each is caught, then restore."""

    mutations = [
        (
            "mismatched eggpool revs",
            PROVIDERS_MANIFEST,
            lambda text: re.sub(
                r'^eggpool-wire\s*=.*$',
                'eggpool-wire = { git = "https://github.com/eggstack/eggpool", rev = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef" }',
                text,
                count=1,
                flags=re.MULTILINE,
            ),
        ),
        (
            "Chat Completions fallback in resolve_route",
            PROFILE_RS,
            lambda text: text.replace(
                "let surface = preference.preferred_surface;",
                "let surface = preference.preferred_surface;\n"
                "    let surface = fallback_surface().unwrap_or(WireSurface::OpenaiChatCompletions);",
            ),
        ),
        (
            "session header removed",
            OPENCODE_GO_RS,
            lambda text: text.replace(
                'const SESSION_HEADER: &str = "x-opencode-session";',
                'const SESSION_HEADER: &str = "x-other";',
            ),
        ),
        (
            "second resolve inside stream()",
            OPENCODE_GO_RS,
            lambda text: text.replace(
                "let route = self.resolve(request)?;",
                "let route = self.resolve(request)?;\n        let _again = self.resolve(request)?;",
            ),
        ),
        (
            "base-URL mirror test removed",
            SETUP_CATALOG_RS,
            lambda text: text.replace(
                "fn opencode_go_base_url_mirrors_the_shared_profile",
                "fn renamed_opencode_go_mirror_test",
            ),
        ),
        (
            "direct Responses routed through the hosted subsystem",
            WIRE_RS,
            lambda text: text.replace(
                "    let mut canonical = canonical_request(request, None);\n    if request.tools.is_none() {\n        // The shared codec validates that a selected tool choice has a tool\n        // collection; without a tools field there is nothing to select from.\n        canonical.tool_choice = None;\n    }\n    encode(&canonical, WireSurface::OpenaiResponses, include_stream_usage)",
                "    let _ = crate::responses_api::MAX_SSE_BUFFER_SIZE;\n    let mut canonical = canonical_request(request, None);\n    if request.tools.is_none() {\n        // The shared codec validates that a selected tool choice has a tool\n        // collection; without a tools field there is nothing to select from.\n        canonical.tool_choice = None;\n    }\n    encode(&canonical, WireSurface::OpenaiResponses, include_stream_usage)",
            ),
        ),
    ]

    failures = 0
    originals = {path: path.read_text(encoding="utf-8") for _, path, _ in mutations}
    for name, path, mutate in mutations:
        original = originals[path]
        mutated = mutate(original)
        if mutated == original:
            print(f"  FAIL self-test could not inject regression: {name}")
            failures += 1
            continue
        try:
            path.write_text(mutated, encoding="utf-8")
            try:
                run(verbose=False)
            except GuardFailure:
                print(f"  ok  self-test caught regression: {name}")
            else:
                print(f"  FAIL self-test missed regression: {name}")
                failures += 1
        finally:
            path.write_text(original, encoding="utf-8")

    # Every touched file must be byte-identical to its pre-self-test state.
    for path, original in originals.items():
        if path.read_text(encoding="utf-8") != original:
            print(f"  FAIL self-test did not restore {path.name}")
            failures += 1

    if failures:
        print(f"self-test FAILED with {failures} problem(s)")
        return 1
    print(f"self-test ok ({len(mutations)} regressions caught)")
    return 0


def main() -> int:
    args = sys.argv[1:]
    if "--self-test" in args:
        return self_test()
    verbose = "--verbose" in args or "-v" in args
    try:
        notes = run(verbose=verbose)
    except GuardFailure as failure:
        print(f"FAIL {failure}", file=sys.stderr)
        return 1
    print(f"provider multi-surface dispatch guard: {len(CHECKS)}/{len(CHECKS)} checks passed")
    for note in notes:
        print(f"  - {note}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())