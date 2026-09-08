#!/usr/bin/env bash
#
# package-binary.sh — package one already-built `codegg` binary into the
# canonical manual-release archive.
#
# Usage:
#   scripts/release/package-binary.sh \
#     --target <triple> --binary <path> --out-dir <dir> [--force]
#
# The script never builds. It validates an explicit target against the
# supported allowlist, validates the input is a regular executable file
# (never a symlink), stages it in a private temp directory as `codegg` with
# mode 755, creates the tarball at a temporary path, verifies the member
# list, then atomically renames it to:
#   <out-dir>/codegg-<target>.tar.gz
#
# Existing final archives are never overwritten unless --force is given.
# Concurrent packaging to the same target/out-dir is unsupported; callers
# serialize release packaging.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/lib-release.sh"

usage() {
    cat <<EOF
Usage: package-binary.sh --target <triple> --binary <path> --out-dir <dir> [--force]

Supported targets:
  $CODEGG_SUPPORTED_TARGETS
  (required for release completeness: $CODEGG_REQUIRED_TARGETS)

Options:
  --target    Rust target triple (must be in the supported allowlist)
  --binary    Path to an already-built regular executable file (symlinks rejected)
  --out-dir   Release directory receiving codegg-<target>.tar.gz
  --force     Replace an existing final archive (default: refuse to overwrite)
  --help      Print this message
EOF
}

TARGET=""
BINARY=""
OUT_DIR=""
FORCE=0

while [ $# -gt 0 ]; do
    case "$1" in
        --target)
            [ $# -ge 2 ] || { printf 'error: --target needs a value\n' >&2; exit 1; }
            TARGET="$2"; shift 2
            ;;
        --binary)
            [ $# -ge 2 ] || { printf 'error: --binary needs a value\n' >&2; exit 1; }
            BINARY="$2"; shift 2
            ;;
        --out-dir)
            [ $# -ge 2 ] || { printf 'error: --out-dir needs a value\n' >&2; exit 1; }
            OUT_DIR="$2"; shift 2
            ;;
        --force)
            FORCE=1; shift
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

[ -n "$TARGET" ] || { printf 'error: --target is required\n' >&2; usage >&2; exit 1; }
[ -n "$BINARY" ] || { printf 'error: --binary is required\n' >&2; usage >&2; exit 1; }
[ -n "$OUT_DIR" ] || { printf 'error: --out-dir is required\n' >&2; usage >&2; exit 1; }

codegg_release_reject_option_like "--target value" "$TARGET" || exit 1
codegg_release_reject_option_like "--binary value" "$BINARY" || exit 1
codegg_release_reject_option_like "--out-dir value" "$OUT_DIR" || exit 1

if ! codegg_release_is_supported_target "$TARGET"; then
    printf 'error: unsupported target: %s\n' "$TARGET" >&2
    printf 'supported targets: %s\n' "$CODEGG_SUPPORTED_TARGETS" >&2
    exit 1
fi

if [ ! -e "$BINARY" ]; then
    printf 'error: binary does not exist: %s\n' "$BINARY" >&2
    exit 1
fi
if [ -L "$BINARY" ]; then
    printf 'error: binary must be a regular file, not a symlink: %s\n' "$BINARY" >&2
    exit 1
fi
if [ ! -f "$BINARY" ]; then
    printf 'error: binary must be a regular file: %s\n' "$BINARY" >&2
    exit 1
fi
if [ ! -s "$BINARY" ]; then
    printf 'error: binary is empty: %s\n' "$BINARY" >&2
    exit 1
fi
if [ ! -x "$BINARY" ]; then
    printf 'error: binary is not executable (chmod +x the built codegg first): %s\n' "$BINARY" >&2
    exit 1
fi

mkdir -p -- "$OUT_DIR"
if [ ! -d "$OUT_DIR" ]; then
    printf 'error: --out-dir is not a directory: %s\n' "$OUT_DIR" >&2
    exit 1
fi

ARCHIVE="$(codegg_release_archive_name "$TARGET")"
FINAL="$OUT_DIR/$ARCHIVE"

if [ -L "$FINAL" ]; then
    printf 'error: existing output is a symlink, refusing to replace: %s\n' "$FINAL" >&2
    exit 1
fi
if [ -e "$FINAL" ] && [ "$FORCE" -ne 1 ]; then
    printf 'error: output already exists (pass --force to replace): %s\n' "$FINAL" >&2
    exit 1
fi
if [ -e "$FINAL" ] && [ ! -f "$FINAL" ]; then
    printf 'error: existing output is not a regular file: %s\n' "$FINAL" >&2
    exit 1
fi

STAGING=""
TMP_OUT=""
cleanup() {
    if [ -n "$STAGING" ] && [ -d "$STAGING" ]; then
        rm -rf -- "$STAGING"
    fi
    # Only remove the temp archive when it is still a temp path (not renamed).
    if [ -n "$TMP_OUT" ] && [ -e "$TMP_OUT" ]; then
        rm -f -- "$TMP_OUT"
    fi
}
trap cleanup EXIT HUP INT TERM

STAGING="$(mktemp -d "${TMPDIR:-/tmp}/codegg-pkg.XXXXXX")"
chmod 700 "$STAGING"

cp -- "$BINARY" "$STAGING/$CODEGG_ARCHIVE_MEMBER"
chmod 755 "$STAGING/$CODEGG_ARCHIVE_MEMBER"

TMP_OUT="$(mktemp "$OUT_DIR/.codegg-$TARGET.tmp.XXXXXX")"
# mktemp creates the temp file; tar output replaces it.
rm -f -- "$TMP_OUT"

# Deterministic gzip header where available (gzip -n drops the embedded
# filename/timestamp). Fall back to plain tar -czf on toolchains without -n.
ARCHIVE_OK=0
if tar -cf - -C "$STAGING" "$CODEGG_ARCHIVE_MEMBER" 2>/dev/null | gzip -n > "$TMP_OUT" 2>/dev/null; then
    if [ -s "$TMP_OUT" ] && tar -tzf "$TMP_OUT" >/dev/null 2>&1; then
        ARCHIVE_OK=1
    else
        rm -f -- "$TMP_OUT"
    fi
fi
if [ "$ARCHIVE_OK" -ne 1 ]; then
    rm -f -- "$TMP_OUT"
    tar -czf "$TMP_OUT" -C "$STAGING" "$CODEGG_ARCHIVE_MEMBER"
fi

# Verify the member list before publishing: exactly one `codegg` entry,
# no absolute paths, no traversal components.
MEMBERS="$(tar -tzf "$TMP_OUT")"
COUNT="$(printf '%s\n' "$MEMBERS" | wc -l | tr -d ' ')"
if [ "$COUNT" != "1" ]; then
    printf 'error: archive must contain exactly one member, found %s\n' "$COUNT" >&2
    exit 1
fi
MEMBER="$(printf '%s\n' "$MEMBERS" | head -n 1)"
case "$MEMBER" in
    /*)
        printf 'error: archive member must not be absolute: %s\n' "$MEMBER" >&2
        exit 1
        ;;
esac
case "$MEMBER" in
    *".."*|*"\\"* )
        printf 'error: archive member must not contain traversal: %s\n' "$MEMBER" >&2
        exit 1
        ;;
esac
NORMALIZED="${MEMBER#./}"
if [ "$NORMALIZED" != "$CODEGG_ARCHIVE_MEMBER" ]; then
    printf 'error: archive member must be exactly `%s`, found `%s`\n' "$CODEGG_ARCHIVE_MEMBER" "$MEMBER" >&2
    exit 1
fi

chmod 644 "$TMP_OUT"
# Atomic publish within the destination filesystem.
mv -- "$TMP_OUT" "$FINAL"
# mv consumed the temp path; prevent the EXIT trap from touching the final.
TMP_OUT=""
trap - EXIT HUP INT TERM
rm -rf -- "$STAGING"
STAGING=""

SUM="$(codegg_release_sha256_file "$FINAL")"
printf 'packaged target=%s archive=%s sha256=%s\n' "$TARGET" "$FINAL" "$SUM"
