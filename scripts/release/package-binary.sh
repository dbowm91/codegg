#!/usr/bin/env bash
#
# package-binary.sh — package already-built CodeGG runfiles into the
# canonical managed release archive.
#
# Usage:
#   scripts/release/package-binary.sh \
#     --target <triple> --binary <codegg> \
#     --sandbox-helper <path> --eggsearch <path> \
#     --out-dir <dir> [--notice <path>] [--eggsearch-version <ver>] [--force]
#
# The script never builds. It validates an explicit target against the
# supported allowlist, validates every required already-built input as a
# regular executable file (never a symlink/device), validates the pinned
# upstream eggsearch identity/version by executing `<eggsearch> --version`,
# stages only the allowlisted per-target manifest with normalized modes,
# creates the tarball at a temporary path, verifies the member list, then
# atomically renames it to:
#   <out-dir>/codegg-<target>.tar.gz
#
# Unix archives contain exactly:
#   codegg, codegg-sandbox-helper, codegg-eggsearch
# Windows archives contain exactly:
#   codegg.exe, codegg-sandbox-helper.exe, codegg-eggsearch.exe
# Either may additionally contain the single fixed notice member
# THIRD-PARTY-NOTICES.txt when --notice is given. No other member is
# permitted.
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
Usage: package-binary.sh --target <triple> --binary <path> --sandbox-helper <path> --eggsearch <path> --out-dir <dir> [--notice <path>] [--eggsearch-version <ver>] [--force]

Supported targets:
  $CODEGG_SUPPORTED_TARGETS
  (required for release completeness: $CODEGG_REQUIRED_TARGETS)

Pinned eggsearch sidecar: $CODEGG_EGGSEARCH_PINNED_VERSION ($CODEGG_EGGSEARCH_SOURCE tag $CODEGG_EGGSEARCH_UPSTREAM_TAG)

Options:
  --target             Rust target triple (must be in the supported allowlist)
  --binary             Path to an already-built regular codegg executable (symlinks rejected)
  --sandbox-helper     Path to an already-built regular codegg-sandbox-helper executable
  --eggsearch          Path to an already-built upstream eggsearch executable (staged as codegg-eggsearch)
  --out-dir            Release directory receiving codegg-<target>.tar.gz
  --notice             Optional regular file staged as THIRD-PARTY-NOTICES.txt (max 64 KiB)
  --eggsearch-version  Expected eggsearch version (default: pinned $CODEGG_EGGSEARCH_PINNED_VERSION; must equal the pin)
  --force              Replace an existing final archive (default: refuse to overwrite)
  --help               Print this message
EOF
}

TARGET=""
BINARY=""
SANDBOX_HELPER=""
EGGSEARCH=""
OUT_DIR=""
NOTICE=""
EGGSEARCH_VERSION="$CODEGG_EGGSEARCH_PINNED_VERSION"
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
        --sandbox-helper)
            [ $# -ge 2 ] || { printf 'error: --sandbox-helper needs a value\n' >&2; exit 1; }
            SANDBOX_HELPER="$2"; shift 2
            ;;
        --eggsearch)
            [ $# -ge 2 ] || { printf 'error: --eggsearch needs a value\n' >&2; exit 1; }
            EGGSEARCH="$2"; shift 2
            ;;
        --out-dir)
            [ $# -ge 2 ] || { printf 'error: --out-dir needs a value\n' >&2; exit 1; }
            OUT_DIR="$2"; shift 2
            ;;
        --notice)
            [ $# -ge 2 ] || { printf 'error: --notice needs a value\n' >&2; exit 1; }
            NOTICE="$2"; shift 2
            ;;
        --eggsearch-version)
            [ $# -ge 2 ] || { printf 'error: --eggsearch-version needs a value\n' >&2; exit 1; }
            EGGSEARCH_VERSION="$2"; shift 2
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
[ -n "$SANDBOX_HELPER" ] || { printf 'error: --sandbox-helper is required\n' >&2; usage >&2; exit 1; }
[ -n "$EGGSEARCH" ] || { printf 'error: --eggsearch is required\n' >&2; usage >&2; exit 1; }
[ -n "$OUT_DIR" ] || { printf 'error: --out-dir is required\n' >&2; usage >&2; exit 1; }

codegg_release_reject_option_like "--target value" "$TARGET" || exit 1
codegg_release_reject_option_like "--binary value" "$BINARY" || exit 1
codegg_release_reject_option_like "--sandbox-helper value" "$SANDBOX_HELPER" || exit 1
codegg_release_reject_option_like "--eggsearch value" "$EGGSEARCH" || exit 1
codegg_release_reject_option_like "--out-dir value" "$OUT_DIR" || exit 1
if [ -n "$NOTICE" ]; then
    codegg_release_reject_option_like "--notice value" "$NOTICE" || exit 1
fi
codegg_release_reject_option_like "--eggsearch-version value" "$EGGSEARCH_VERSION" || exit 1

if ! codegg_release_is_supported_target "$TARGET"; then
    printf 'error: unsupported target: %s\n' "$TARGET" >&2
    printf 'supported targets: %s\n' "$CODEGG_SUPPORTED_TARGETS" >&2
    exit 1
fi

# The pin in lib-release.sh is authoritative: an explicit version that
# differs from the pin fails closed so version drift needs a deliberate
# pin bump plus RELEASING.md/notice updates, not a silent flag.
if [ "$EGGSEARCH_VERSION" != "$CODEGG_EGGSEARCH_PINNED_VERSION" ]; then
    printf 'error: --eggsearch-version %s does not match pinned %s (bump the pin to change versions)\n' "$EGGSEARCH_VERSION" "$CODEGG_EGGSEARCH_PINNED_VERSION" >&2
    exit 1
fi

check_executable_input() {
    local label="$1" path="$2"
    if [ ! -e "$path" ]; then
        printf 'error: %s does not exist: %s\n' "$label" "$path" >&2
        return 1
    fi
    if [ -L "$path" ]; then
        printf 'error: %s must be a regular file, not a symlink: %s\n' "$label" "$path" >&2
        return 1
    fi
    if [ ! -f "$path" ]; then
        printf 'error: %s must be a regular file: %s\n' "$label" "$path" >&2
        return 1
    fi
    if [ ! -s "$path" ]; then
        printf 'error: %s is empty: %s\n' "$label" "$path" >&2
        return 1
    fi
    if [ ! -x "$path" ]; then
        printf 'error: %s is not executable (chmod +x first): %s\n' "$label" "$path" >&2
        return 1
    fi
    return 0
}

check_executable_input "codegg binary" "$BINARY" || exit 1
check_executable_input "sandbox-helper binary" "$SANDBOX_HELPER" || exit 1
check_executable_input "eggsearch binary" "$EGGSEARCH" || exit 1

if [ -n "$NOTICE" ]; then
    if [ ! -e "$NOTICE" ]; then
        printf 'error: notice does not exist: %s\n' "$NOTICE" >&2
        exit 1
    fi
    if [ -L "$NOTICE" ]; then
        printf 'error: notice must be a regular file, not a symlink: %s\n' "$NOTICE" >&2
        exit 1
    fi
    if [ ! -f "$NOTICE" ]; then
        printf 'error: notice must be a regular file: %s\n' "$NOTICE" >&2
        exit 1
    fi
    if [ ! -s "$NOTICE" ]; then
        printf 'error: notice is empty: %s\n' "$NOTICE" >&2
        exit 1
    fi
    notice_bytes="$(wc -c <"$NOTICE" | tr -d ' ')"
    if [ "$notice_bytes" -gt 65536 ]; then
        printf 'error: notice exceeds 64 KiB (%s bytes): %s\n' "$notice_bytes" "$NOTICE" >&2
        exit 1
    fi
fi

# Upstream identity/version validation: the staged sidecar must report the
# pinned eggsearch version. This runs the already-built input binary with
# the safe `--version` probe only (no network, no config access).
EGGSEARCH_OUT=""
if ! EGGSEARCH_OUT="$("$EGGSEARCH" --version 2>&1)"; then
    printf 'error: eggsearch --version probe failed for: %s\n' "$EGGSEARCH" >&2
    printf '%s\n' "$EGGSEARCH_OUT" >&2
    exit 1
fi
if ! codegg_release_check_eggsearch_version_output "$EGGSEARCH_OUT" "$EGGSEARCH_VERSION"; then
    exit 1
fi

# Best-effort target architecture/OS cross-check where inspectable.
# Fixture shell scripts and text inputs carry no ELF/Mach-O/PE magic and
# are skipped; real binaries with contradictory magic fail closed.
if command -v file >/dev/null 2>&1; then
    for probe_pair in "$BINARY:codegg" "$SANDBOX_HELPER:sandbox-helper" "$EGGSEARCH:eggsearch"; do
        probe_path="${probe_pair%%:*}"
        probe_label="${probe_pair##*:}"
        probe_desc="$(file -b -- "$probe_path" 2>/dev/null || true)"
        if [ -z "$probe_desc" ]; then
            continue
        fi
        case "$probe_desc" in
            *Mach-O*)
                if codegg_release_is_windows_target "$TARGET"; then
                    printf 'error: %s looks like macOS Mach-O but target is %s: %s\n' "$probe_label" "$TARGET" "$probe_desc" >&2
                    exit 1
                fi
                case "$TARGET" in
                    *-apple-darwin) ;;
                    *)
                        printf 'error: %s looks like macOS Mach-O but target is %s: %s\n' "$probe_label" "$TARGET" "$probe_desc" >&2
                        exit 1
                        ;;
                esac
                ;;
            *PE32*|*MS-DOS*executable*)
                if ! codegg_release_is_windows_target "$TARGET"; then
                    printf 'error: %s looks like Windows PE but target is %s: %s\n' "$probe_label" "$TARGET" "$probe_desc" >&2
                    exit 1
                fi
                ;;
            *ELF*)
                case "$TARGET" in
                    *-unknown-linux-gnu) ;;
                    *)
                        printf 'error: %s looks like Linux ELF but target is %s: %s\n' "$probe_label" "$TARGET" "$probe_desc" >&2
                        exit 1
                        ;;
                esac
                ;;
        esac
        case "$probe_desc" in
            *x86-64*|*x86_64*)
                case "$TARGET" in
                    x86_64-*) ;;
                    *) printf 'error: %s looks like x86_64 but target is %s\n' "$probe_label" "$TARGET" >&2; exit 1 ;;
                esac
                ;;
            *aarch64*|*ARM64*|*arm64*)
                case "$TARGET" in
                    aarch64-*) ;;
                    *) printf 'error: %s looks like aarch64 but target is %s\n' "$probe_label" "$TARGET" >&2; exit 1 ;;
                esac
                ;;
        esac
    done
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

# Resolve the per-target member names explicitly (no Unix assumption for
# Windows) and stage only the allowlisted manifest.
if codegg_release_is_windows_target "$TARGET"; then
    MEMBER_MAIN="codegg.exe"
    MEMBER_HELPER="codegg-sandbox-helper.exe"
    MEMBER_EGGSEARCH="codegg-eggsearch.exe"
else
    MEMBER_MAIN="codegg"
    MEMBER_HELPER="codegg-sandbox-helper"
    MEMBER_EGGSEARCH="codegg-eggsearch"
fi

cp -- "$BINARY" "$STAGING/$MEMBER_MAIN"
cp -- "$SANDBOX_HELPER" "$STAGING/$MEMBER_HELPER"
cp -- "$EGGSEARCH" "$STAGING/$MEMBER_EGGSEARCH"
chmod 755 "$STAGING/$MEMBER_MAIN" "$STAGING/$MEMBER_HELPER" "$STAGING/$MEMBER_EGGSEARCH"

if [ -n "$NOTICE" ]; then
    cp -- "$NOTICE" "$STAGING/$CODEGG_NOTICE_MEMBER"
    chmod 644 "$STAGING/$CODEGG_NOTICE_MEMBER"
fi

TMP_OUT="$(mktemp "$OUT_DIR/.codegg-$TARGET.tmp.XXXXXX")"
# mktemp creates the temp file; tar output replaces it.
rm -f -- "$TMP_OUT"

# Fixed member order keeps archives deterministic for identical inputs.
if [ -n "$NOTICE" ]; then
    # shellcheck disable=SC2086
    TAR_MEMBERS="$MEMBER_MAIN $MEMBER_EGGSEARCH $MEMBER_HELPER $CODEGG_NOTICE_MEMBER"
else
    # shellcheck disable=SC2086
    TAR_MEMBERS="$MEMBER_MAIN $MEMBER_EGGSEARCH $MEMBER_HELPER"
fi

# Deterministic gzip header where available (gzip -n drops the embedded
# filename/timestamp). Fall back to plain tar -czf on toolchains without -n.
ARCHIVE_OK=0
# shellcheck disable=SC2086
if tar -cf - -C "$STAGING" $TAR_MEMBERS 2>/dev/null | gzip -n > "$TMP_OUT" 2>/dev/null; then
    if [ -s "$TMP_OUT" ] && tar -tzf "$TMP_OUT" >/dev/null 2>&1; then
        ARCHIVE_OK=1
    else
        rm -f -- "$TMP_OUT"
    fi
fi
if [ "$ARCHIVE_OK" -ne 1 ]; then
    rm -f -- "$TMP_OUT"
    # shellcheck disable=SC2086
    tar -czf "$TMP_OUT" -C "$STAGING" $TAR_MEMBERS
fi

# Verify the member list before publishing: exactly the allowlisted
# runfiles (plus the fixed notice when staged); no absolute paths, no
# traversal components, no duplicates, no unexpected payloads.
MEMBERS="$(tar -tzf "$TMP_OUT")"
EXPECTED_COUNT=3
if [ -n "$NOTICE" ]; then
    EXPECTED_COUNT=4
fi
COUNT="$(printf '%s\n' "$MEMBERS" | wc -l | tr -d ' ')"
if [ "$COUNT" != "$EXPECTED_COUNT" ]; then
    printf 'error: archive must contain exactly %s member(s), found %s\n' "$EXPECTED_COUNT" "$COUNT" >&2
    printf '%s\n' "$MEMBERS" >&2
    exit 1
fi
# shellcheck disable=SC2016
seen=""
for m in $MEMBERS; do
    case "$m" in
        /*)
            printf 'error: archive member must not be absolute: %s\n' "$m" >&2
            exit 1
            ;;
    esac
    case "$m" in
        *".."*|*"\\"* )
            printf 'error: archive member must not contain traversal: %s\n' "$m" >&2
            exit 1
            ;;
    esac
    norm="${m#./}"
    case " $seen " in
        *" $norm "*)
            printf 'error: duplicate archive member: %s\n' "$m" >&2
            exit 1
            ;;
    esac
    seen="$seen $norm"
    if [ "$norm" != "$MEMBER_MAIN" ] && [ "$norm" != "$MEMBER_HELPER" ] && [ "$norm" != "$MEMBER_EGGSEARCH" ] && [ "$norm" != "$CODEGG_NOTICE_MEMBER" ]; then
        printf 'error: unexpected archive member: %s (expected runfiles + optional %s)\n' "$m" "$CODEGG_NOTICE_MEMBER" >&2
        exit 1
    fi
done
for required in "$MEMBER_MAIN" "$MEMBER_HELPER" "$MEMBER_EGGSEARCH"; do
    case " $seen " in
        *" $required "*) ;;
        *)
            printf 'error: archive missing required runfile: %s (found:%s)\n' "$required" "$seen" >&2
            exit 1
            ;;
    esac
done
if [ -z "$NOTICE" ]; then
    case " $seen " in
        *" $CODEGG_NOTICE_MEMBER "*)
            printf 'error: unexpected notice member without --notice\n' >&2
            exit 1
            ;;
    esac
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
BIN_SUM="$(codegg_release_sha256_file "$BINARY")"
HELPER_SUM="$(codegg_release_sha256_file "$SANDBOX_HELPER")"
EGG_SUM="$(codegg_release_sha256_file "$EGGSEARCH")"
printf 'packaged target=%s archive=%s sha256=%s\n' "$TARGET" "$FINAL" "$SUM"
printf 'inputs: codegg=%s sandbox-helper=%s eggsearch(%s)=%s\n' "$BIN_SUM" "$HELPER_SUM" "$EGGSEARCH_VERSION" "$EGG_SUM"
