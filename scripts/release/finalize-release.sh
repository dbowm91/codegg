#!/usr/bin/env bash
#
# finalize-release.sh — generate the deterministic checksums.txt manifest.
#
# Usage:
#   scripts/release/finalize-release.sh --dir <release-dir>
#
# Hashes every supported archive present in the directory
# (codegg-<target>.tar.gz for the required Linux/macOS set plus the
# optional best-effort Windows target), sorts entries deterministically by
# basename (LC_ALL=C), and atomically replaces checksums.txt only after all
# hashes succeed. A generation failure never leaves a partially written
# manifest. Completeness (all four required targets) is enforced by
# verify-release.sh, not here, so per-host packaging can be finalized and
# inspected before the full set is assembled.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/lib-release.sh"

usage() {
    cat <<EOF
Usage: finalize-release.sh --dir <release-dir>

Generates <release-dir>/checksums.txt with lines:
  <sha256>  <basename>
sorted by basename. Only supported archive names are hashed; unrelated
files are never folded into the manifest (verify-release.sh rejects them).
EOF
}

DIR=""

while [ $# -gt 0 ]; do
    case "$1" in
        --dir)
            [ $# -ge 2 ] || { printf 'error: --dir needs a value\n' >&2; exit 1; }
            DIR="$2"; shift 2
            ;;
        --help|-h)
            usage; exit 0
            ;;
        --)
            shift; break
            ;;
        -*)
            printf 'error: unknown option: %s\n' "$1" >&2; usage >&2; exit 1
            ;;
        *)
            printf 'error: unexpected positional argument: %s\n' "$1" >&2; usage >&2; exit 1
            ;;
    esac
done

[ -n "$DIR" ] || { printf 'error: --dir is required\n' >&2; usage >&2; exit 1; }
codegg_release_reject_option_like "--dir value" "$DIR" || exit 1

if [ ! -d "$DIR" ]; then
    printf 'error: --dir is not a directory: %s\n' "$DIR" >&2
    exit 1
fi
if [ -L "$DIR/$CODEGG_CHECKSUM_FILE" ]; then
    printf 'error: existing %s is a symlink, refusing to replace\n' "$CODEGG_CHECKSUM_FILE" >&2
    exit 1
fi

PRESENT=""
COUNT=0
t=""
for t in $CODEGG_SUPPORTED_TARGETS; do
    name="$(codegg_release_archive_name "$t")"
    path="$DIR/$name"
    if [ -e "$path" ]; then
        if [ -L "$path" ]; then
            printf 'error: archive is a symlink, refusing to hash: %s\n' "$path" >&2
            exit 1
        fi
        if [ ! -f "$path" ]; then
            printf 'error: archive path is not a regular file: %s\n' "$path" >&2
            exit 1
        fi
        PRESENT="$PRESENT $name"
        COUNT=$((COUNT + 1))
    fi
done

if [ "$COUNT" -eq 0 ]; then
    printf 'error: no supported archives found in %s\n' "$DIR" >&2
    printf 'supported names: ' >&2
    for t in $CODEGG_SUPPORTED_TARGETS; do
        printf '%s ' "$(codegg_release_archive_name "$t")" >&2
    done
    printf '\n' >&2
    exit 1
fi

TMP_MANIFEST=""
cleanup() {
    if [ -n "$TMP_MANIFEST" ] && [ -e "$TMP_MANIFEST" ]; then
        rm -f -- "$TMP_MANIFEST"
    fi
}
trap cleanup EXIT HUP INT TERM

TMP_MANIFEST="$(mktemp "$DIR/.checksums.tmp.XXXXXX")"
rm -f -- "$TMP_MANIFEST"

# Build the manifest fully before touching the final file.
: > "$TMP_MANIFEST"
name=""
# Deterministic order regardless of allowlist declaration order.
for name in $(printf '%s\n' $PRESENT | LC_ALL=C sort); do
    sum="$(codegg_release_sha256_file "$DIR/$name")" || exit 1
    case "$sum" in
        *[!0-9a-f]*|"")
            printf 'error: unexpected hash output for %s\n' "$name" >&2
            exit 1
            ;;
    esac
    if [ "${#sum}" -ne 64 ]; then
        printf 'error: unexpected hash length for %s\n' "$name" >&2
        exit 1
    fi
    printf '%s  %s\n' "$sum" "$name" >> "$TMP_MANIFEST"
done

chmod 644 "$TMP_MANIFEST"
mv -- "$TMP_MANIFEST" "$DIR/$CODEGG_CHECKSUM_FILE"
TMP_MANIFEST=""
trap - EXIT HUP INT TERM

printf 'finalized %s with %s entr%s:\n' "$DIR/$CODEGG_CHECKSUM_FILE" "$COUNT" "$([ "$COUNT" -eq 1 ] && printf 'y' || printf 'ies')"
cat -- "$DIR/$CODEGG_CHECKSUM_FILE"
