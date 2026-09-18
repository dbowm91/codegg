#!/usr/bin/env bash
# lib-release.sh — shared artifact-contract constants for CodeGG manual releases.
#
# Sourced by scripts/release/package-binary.sh, finalize-release.sh,
# verify-release.sh, and test-release-tools.sh. Not intended for direct
# execution.
#
# Contract (self-contained-installation M001):
#   release/
#     codegg-x86_64-unknown-linux-gnu.tar.gz
#     codegg-aarch64-unknown-linux-gnu.tar.gz
#     codegg-x86_64-apple-darwin.tar.gz
#     codegg-aarch64-apple-darwin.tar.gz
#     checksums.txt
#
# Each Unix archive carries the exact managed runfile set:
#   `codegg`, `codegg-sandbox-helper`, `codegg-eggsearch`
# plus an optional fixed third-party notice `THIRD-PARTY-NOTICES.txt`.
# The Windows archive carries the corresponding `.exe` names, defined
# explicitly below rather than by Unix assumption.
# checksums.txt uses conventional two-column SHA-256 lines:
#   "<sha256>  <basename>"
# sorted by basename (LC_ALL=C), compatible with `sha256sum -c`.
#
# Upstream eggsearch provenance (M001 pin):
#   source  https://github.com/eggstack/eggsearch (stable MCP/CLI contract)
#   tag     v0.3.9 / version 0.3.9 (baseline; bump by changing the pin below
#           together with RELEASING.md and the release notices)
#   license MIT (as published upstream; redistributed under upstream terms)
# The sidecar is installed under the CodeGG-owned name `codegg-eggsearch`
# so a managed install never resolves an unrelated PATH `eggsearch`.

# Guard against double-sourcing under `set -u`.
if [ -n "${CODEGG_RELEASE_LIB_LOADED:-}" ]; then
    return 0 2>/dev/null || exit 0
fi
CODEGG_RELEASE_LIB_LOADED=1

# Four required Linux/macOS installer targets. Stable across versions;
# version identity comes from the GitHub release/tag, never the filename.
CODEGG_REQUIRED_TARGETS="x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin"

# Optional best-effort target. Documented separately; never required for
# release-set completeness. Unknown names outside required+optional are
# always rejected.
CODEGG_OPTIONAL_TARGETS="x86_64-pc-windows-msvc"

CODEGG_SUPPORTED_TARGETS="$CODEGG_REQUIRED_TARGETS $CODEGG_OPTIONAL_TARGETS"

CODEGG_ARCHIVE_PREFIX="codegg-"
CODEGG_ARCHIVE_SUFFIX=".tar.gz"
CODEGG_CHECKSUM_FILE="checksums.txt"
CODEGG_ARCHIVE_MEMBER="codegg"

# Pinned upstream eggsearch provenance for the managed sidecar.
# Baseline 0.3.9 per the self-contained-installation corrective addendum.
# Upstream documents MCP/CLI as the stable application contract; CodeGG
# must not compile against eggsearch internal modules to ease packaging.
CODEGG_EGGSEARCH_PINNED_VERSION="0.3.9"
CODEGG_EGGSEARCH_UPSTREAM_TAG="v0.3.9"
CODEGG_EGGSEARCH_SOURCE="https://github.com/eggstack/eggsearch"
CODEGG_EGGSEARCH_LICENSE="MIT"

# Installed sidecar name (CodeGG-owned; never bare `eggsearch`).
CODEGG_EGGSEARCH_SIDECAR="codegg-eggsearch"
CODEGG_SANDBOX_HELPER="codegg-sandbox-helper"

# Fixed minimal third-party notice member redistributed alongside the
# runfiles when eggsearch redistribution requires it. This is the only
# non-executable member ever permitted inside a release archive.
CODEGG_NOTICE_MEMBER="THIRD-PARTY-NOTICES.txt"

codegg_release_fail() {
    printf 'error: %s\n' "$1" >&2
    return 1
}

codegg_release_is_required_target() {
    local target="$1"
    local t
    for t in $CODEGG_REQUIRED_TARGETS; do
        if [ "$t" = "$target" ]; then
            return 0
        fi
    done
    return 1
}

codegg_release_is_supported_target() {
    local target="$1"
    local t
    for t in $CODEGG_SUPPORTED_TARGETS; do
        if [ "$t" = "$target" ]; then
            return 0
        fi
    done
    return 1
}

codegg_release_archive_name() {
    printf '%s%s%s\n' "$CODEGG_ARCHIVE_PREFIX" "$1" "$CODEGG_ARCHIVE_SUFFIX"
}

# Print the target triple for a supported archive basename, or nothing.
codegg_release_target_for_archive() {
    local base="$1"
    local t name
    for t in $CODEGG_SUPPORTED_TARGETS; do
        name="$(codegg_release_archive_name "$t")"
        if [ "$name" = "$base" ]; then
            printf '%s\n' "$t"
            return 0
        fi
    done
    return 1
}

codegg_release_is_supported_archive() {
    codegg_release_target_for_archive "$1" >/dev/null 2>&1
}

# Print lowercase hex SHA-256 of a regular file to stdout.
codegg_release_sha256_file() {
    local file="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum -- "$file" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 -- "$file" | awk '{print $1}'
    else
        printf 'error: no SHA-256 tool found (need sha256sum or shasum)\n' >&2
        return 1
    fi
}

# Reject option-like values so `--target -foo` style injection cannot be
# mistaken for a path/target.
codegg_release_reject_option_like() {
    local kind="$1"
    local value="$2"
    case "$value" in
        -*)
            printf 'error: %s must not start with "-": %s\n' "$kind" "$value" >&2
            return 1
            ;;
        *)
            return 0
            ;;
    esac
}

# True when the target uses Windows executable naming.
codegg_release_is_windows_target() {
    case "$1" in
        *-pc-windows-msvc) return 0 ;;
        *) return 1 ;;
    esac
}

# Print the exact required executable runfile names for a target, one per
# line, in canonical (sorted) order. Windows names carry the explicit
# `.exe` suffix; Unix names are bare. Never guess: unknown targets fail.
codegg_release_runfiles_for_target() {
    local target="$1"
    if ! codegg_release_is_supported_target "$target"; then
        return 1
    fi
    if codegg_release_is_windows_target "$target"; then
        printf '%s\n' "codegg.exe" "codegg-eggsearch.exe" "codegg-sandbox-helper.exe"
    else
        printf '%s\n' "codegg" "codegg-eggsearch" "codegg-sandbox-helper"
    fi
}

# Print the primary `codegg` member name for a target.
codegg_release_main_member_for_target() {
    local target="$1"
    if ! codegg_release_is_supported_target "$target"; then
        return 1
    fi
    if codegg_release_is_windows_target "$target"; then
        printf '%s\n' "codegg.exe"
    else
        printf '%s\n' "codegg"
    fi
}

# True when a member name is one of the three required runfiles.
codegg_release_is_runfile_for_target() {
    local target="$1" member="$2" f
    for f in $(codegg_release_runfiles_for_target "$target"); do
        if [ "$f" = "$member" ]; then
            return 0
        fi
    done
    return 1
}

# True when a member name is the fixed third-party notice.
codegg_release_is_notice_member() {
    [ "$1" = "$CODEGG_NOTICE_MEMBER" ]
}

# Validate an `eggsearch --version` style output against an expected
# version. The output must mention eggsearch and contain the exact
# expected dotted version; anything else fails closed.
codegg_release_check_eggsearch_version_output() {
    local output="$1" expected="$2"
    case "$output" in
        *eggsearch* | *Eggsearch* | *EGGSEARCH*) ;;
        *)
            printf 'error: eggsearch identity check failed (no eggsearch token): %s\n' "$output" >&2
            return 1
            ;;
    esac
    case "$output" in
        *"$expected"*) return 0 ;;
        *)
            printf 'error: eggsearch version mismatch: expected %s, got: %s\n' "$expected" "$output" >&2
            return 1
            ;;
    esac
}
