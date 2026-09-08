#!/usr/bin/env bash
# lib-release.sh — shared artifact-contract constants for CodeGG manual releases.
#
# Sourced by scripts/release/package-binary.sh, finalize-release.sh,
# verify-release.sh, and test-release-tools.sh. Not intended for direct
# execution.
#
# Contract (M001):
#   release/
#     codegg-x86_64-unknown-linux-gnu.tar.gz
#     codegg-aarch64-unknown-linux-gnu.tar.gz
#     codegg-x86_64-apple-darwin.tar.gz
#     codegg-aarch64-apple-darwin.tar.gz
#     checksums.txt
#
# Archive payload is exactly one top-level regular executable named `codegg`.
# checksums.txt uses conventional two-column SHA-256 lines:
#   "<sha256>  <basename>"
# sorted by basename (LC_ALL=C), compatible with `sha256sum -c`.

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
