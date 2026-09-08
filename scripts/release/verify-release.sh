#!/usr/bin/env bash
#
# verify-release.sh — validate a complete manual-release directory offline.
#
# Usage:
#   scripts/release/verify-release.sh --dir <release-dir>
#     [--allow-incomplete-target-set]
#     [--expect-version <X.Y.Z>]
#     [--skip-version-smoke]
#
# Default mode enforces the full four-target Linux/macOS release set.
# --allow-incomplete-target-set is an explicit per-host testing relaxation:
# at least one supported archive must be present and valid, but the full
# required set is not demanded.
#
# Checks (read-only except private temp extraction):
#   1. checksums.txt exists and every line is "<64 hex><space><space|*><basename>"
#      with a supported basename, no absolute/traversal paths, no duplicates.
#   2. Every manifest entry exists and its recomputed SHA-256 matches.
#   3. Every supported archive present on disk is listed in the manifest.
#   4. Required-set completeness (unless relaxed).
#   5. No unexpected files in the release directory.
#   6. Every archive contains exactly one `codegg` regular-file member;
#      absolute/traversal/symlink/device/unexpected payloads are rejected.
#   7. Where the archive target matches the current host, extract to a temp
#      dir and run `codegg --version` (compared to --expect-version when
#      given). Non-native targets skip execution with a note.
#
# Exit 0 only when every check passes. Safe to re-run (idempotent).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/lib-release.sh"

usage() {
    cat <<EOF
Usage: verify-release.sh --dir <release-dir> [--allow-incomplete-target-set] [--expect-version <ver>] [--skip-version-smoke]

Options:
  --dir                        Release directory containing archives + checksums.txt
  --allow-incomplete-target-set
                               Relax completeness: require >=1 valid supported
                               archive instead of all four required targets.
                               Explicit testing mode; final releases must not use it.
  --expect-version <ver>       Require native smoke output to equal this version
                               (leading 'v' allowed, e.g. v0.1.0 or 0.1.0)
  --skip-version-smoke         Skip executing any packaged binary
  --help                       Print this message
EOF
}

DIR=""
ALLOW_INCOMPLETE=0
EXPECT_VERSION=""
SKIP_SMOKE=0

while [ $# -gt 0 ]; do
    case "$1" in
        --dir)
            [ $# -ge 2 ] || { printf 'error: --dir needs a value\n' >&2; exit 1; }
            DIR="$2"; shift 2
            ;;
        --allow-incomplete-target-set)
            ALLOW_INCOMPLETE=1; shift
            ;;
        --expect-version)
            [ $# -ge 2 ] || { printf 'error: --expect-version needs a value\n' >&2; exit 1; }
            EXPECT_VERSION="$2"; shift 2
            ;;
        --skip-version-smoke)
            SKIP_SMOKE=1; shift
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

MANIFEST="$DIR/$CODEGG_CHECKSUM_FILE"
if [ ! -e "$MANIFEST" ]; then
    printf 'error: missing manifest: %s\n' "$MANIFEST" >&2
    exit 1
fi
if [ -L "$MANIFEST" ]; then
    printf 'error: manifest must not be a symlink: %s\n' "$MANIFEST" >&2
    exit 1
fi
if [ ! -f "$MANIFEST" ]; then
    printf 'error: manifest is not a regular file: %s\n' "$MANIFEST" >&2
    exit 1
fi

# Normalize --expect-version: allow one leading 'v', then strict semver-ish.
if [ -n "$EXPECT_VERSION" ]; then
    codegg_release_reject_option_like "--expect-version value" "$EXPECT_VERSION" || exit 1
    EXPECT_VERSION="${EXPECT_VERSION#v}"
    if [ -z "$EXPECT_VERSION" ]; then
        printf 'error: --expect-version must not be empty\n' >&2
        exit 1
    fi
    if ! printf '%s' "$EXPECT_VERSION" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9.+_-]*$'; then
        printf 'error: --expect-version has illegal characters: %s\n' "$EXPECT_VERSION" >&2
        exit 1
    fi
    case "$EXPECT_VERSION" in
        *.*.*) ;;
        *)
            printf 'error: --expect-version must look like X.Y.Z (got: %s)\n' "$EXPECT_VERSION" >&2
            exit 1
            ;;
    esac
fi

# --- 1. Parse and validate the manifest -------------------------------------
MANIFEST_NAMES=""
MANIFEST_COUNT=0
line_no=0
while IFS= read -r line || [ -n "$line" ]; do
    line_no=$((line_no + 1))
    # Reject empty/whitespace-only lines and comments.
    if [ -z "$line" ]; then
        printf 'error: %s:%s: malformed line (empty/comment/leading space)\n' "$CODEGG_CHECKSUM_FILE" "$line_no" >&2
        exit 1
    fi
    case "$line" in
        "#"*) printf 'error: %s:%s: malformed line (empty/comment/leading space)\n' "$CODEGG_CHECKSUM_FILE" "$line_no" >&2; exit 1 ;;
        " "*) printf 'error: %s:%s: malformed line (empty/comment/leading space)\n' "$CODEGG_CHECKSUM_FILE" "$line_no" >&2; exit 1 ;;
    esac
    if printf '%s' "$line" | grep -q '^[[:space:]]'; then
        printf 'error: %s:%s: malformed line (empty/comment/leading space)\n' "$CODEGG_CHECKSUM_FILE" "$line_no" >&2
        exit 1
    fi
    # Split into hash + remainder. Conventional form is "<hash>  <name>"
    # (two spaces); binary-mode "<hash> *<name>" is also accepted.
    hash="${line%% *}"
    rest="${line#"$hash"}"
    if [ "$hash" = "$line" ]; then
        printf 'error: %s:%s: malformed line (expected "<sha256>  <basename>")\n' "$CODEGG_CHECKSUM_FILE" "$line_no" >&2
        exit 1
    fi
    case "$hash" in
        *[!0-9a-f]*)
            printf 'error: %s:%s: hash must be lowercase hex\n' "$CODEGG_CHECKSUM_FILE" "$line_no" >&2
            exit 1
            ;;
    esac
    if [ "${#hash}" -ne 64 ]; then
        printf 'error: %s:%s: hash must be 64 hex chars\n' "$CODEGG_CHECKSUM_FILE" "$line_no" >&2
        exit 1
    fi
    # Strip exactly one leading space, then optional '*' or second space.
    case "$rest" in
        " "* ) rest="${rest# }" ;;
        *)
            printf 'error: %s:%s: malformed separator (expected spaces)\n' "$CODEGG_CHECKSUM_FILE" "$line_no" >&2
            exit 1
            ;;
    esac
    case "$rest" in
        " "* ) rest="${rest# }" ;;
        "*"* ) rest="${rest#\*}" ;;
        *)
            printf 'error: %s:%s: malformed separator (expected two-column form)\n' "$CODEGG_CHECKSUM_FILE" "$line_no" >&2
            exit 1
            ;;
    esac
    name="$rest"
    case "$name" in
        ""|*" "*|*"$'\t'"*|*"/"*|*"\\"*|*..*)
            printf 'error: %s:%s: manifest entry must be a bare basename without paths/traversal: %s\n' "$CODEGG_CHECKSUM_FILE" "$line_no" "$name" >&2
            exit 1
            ;;
    esac
    case "$name" in
        -*) printf 'error: %s:%s: manifest entry must not be option-like: %s\n' "$CODEGG_CHECKSUM_FILE" "$line_no" "$name" >&2; exit 1 ;;
    esac
    if ! codegg_release_is_supported_archive "$name"; then
        printf 'error: %s:%s: unknown artifact (not a supported release asset): %s\n' "$CODEGG_CHECKSUM_FILE" "$line_no" "$name" >&2
        exit 1
    fi
    case " $MANIFEST_NAMES " in
        *" $name "*)
            printf 'error: %s:%s: duplicate manifest entry: %s\n' "$CODEGG_CHECKSUM_FILE" "$line_no" "$name" >&2
            exit 1
            ;;
    esac
    MANIFEST_NAMES="$MANIFEST_NAMES $name"
    MANIFEST_COUNT=$((MANIFEST_COUNT + 1))
done < "$MANIFEST"

if [ "$MANIFEST_COUNT" -eq 0 ]; then
    printf 'error: manifest is empty: %s\n' "$MANIFEST" >&2
    exit 1
fi

# --- 2. Recompute every hash --------------------------------------------------
name=""
for name in $MANIFEST_NAMES; do
    path="$DIR/$name"
    if [ ! -e "$path" ]; then
        printf 'error: manifest lists missing file: %s\n' "$name" >&2
        exit 1
    fi
    if [ -L "$path" ]; then
        printf 'error: archive must not be a symlink: %s\n' "$name" >&2
        exit 1
    fi
    if [ ! -f "$path" ]; then
        printf 'error: archive is not a regular file: %s\n' "$name" >&2
        exit 1
    fi
    # Extract the expected hash for this exact basename (exact match only,
    # never substring/regex).
    expected=""
    while IFS= read -r mline || [ -n "$mline" ]; do
        mhash="${mline%% *}"
        mrest="${mline#"$mhash"}"
        mrest="${mrest# }"
        case "$mrest" in
            " "* ) mrest="${mrest# }" ;;
            "*"* ) mrest="${mrest#\*}" ;;
        esac
        if [ "$mrest" = "$name" ]; then
            if [ -n "$expected" ]; then
                printf 'error: duplicate manifest entry for %s\n' "$name" >&2
                exit 1
            fi
            expected="$mhash"
        fi
    done < "$MANIFEST"
    if [ -z "$expected" ]; then
        printf 'error: internal error resolving manifest entry: %s\n' "$name" >&2
        exit 1
    fi
    actual="$(codegg_release_sha256_file "$path")" || exit 1
    if [ "$actual" != "$expected" ]; then
        printf 'error: checksum mismatch for %s\n' "$name" >&2
        printf '  expected: %s\n' "$expected" >&2
        printf '  actual:   %s\n' "$actual" >&2
        exit 1
    fi
    printf 'checksum ok: %s\n' "$name"
done

# --- 3. Archives on disk must all be listed -----------------------------------
t=""
for t in $CODEGG_SUPPORTED_TARGETS; do
    aname="$(codegg_release_archive_name "$t")"
    if [ -e "$DIR/$aname" ]; then
        case " $MANIFEST_NAMES " in
            *" $aname "*) ;;
            *)
                printf 'error: archive present but missing from manifest: %s\n' "$aname" >&2
                exit 1
                ;;
        esac
    fi
done

# --- 4. Completeness ------------------------------------------------------------
if [ "$ALLOW_INCOMPLETE" -eq 1 ]; then
    if [ "$MANIFEST_COUNT" -lt 1 ]; then
        printf 'error: no supported archives listed (even relaxed mode needs >=1)\n' >&2
        exit 1
    fi
    printf 'note: relaxed completeness (--allow-incomplete-target-set): %s listed archive(s)\n' "$MANIFEST_COUNT"
else
    missing=""
    for t in $CODEGG_REQUIRED_TARGETS; do
        aname="$(codegg_release_archive_name "$t")"
        case " $MANIFEST_NAMES " in
            *" $aname "*) ;;
            *) missing="$missing $aname" ;;
        esac
    done
    if [ -n "$missing" ]; then
        printf 'error: incomplete release set, missing required asset(s):%s\n' "$missing" >&2
        printf 'hint: package every required target or re-run with --allow-incomplete-target-set for per-host testing\n' >&2
        exit 1
    fi
    printf 'completeness ok: all four required targets present\n'
fi

# --- 5. No unexpected files ------------------------------------------------------
# The release directory must contain only supported archives + checksums.txt.
# Anything else (stray builds, configs, credentials, temp files, subdirs)
# fails closed so maintainers inspect before upload.
unexpected=""
for entry in "$DIR"/* "$DIR"/.*; do
    base="$(basename -- "$entry")"
    case "$base" in
        "."|".."|"*"|".*" )
            # "$DIR"/.* glob: skip . and ..; any other dotfile is unexpected.
            case "$base" in
                "."|".." ) continue ;;
                "*" ) continue ;;  # nullglob off: literal when no dotfiles
            esac
            ;;
    esac
    [ -e "$entry" ] || continue
    case "$base" in
        "$CODEGG_CHECKSUM_FILE") continue ;;
    esac
    if codegg_release_is_supported_archive "$base" >/dev/null 2>&1; then
        continue
    fi
    unexpected="$unexpected $base"
done
if [ -n "$unexpected" ]; then
    # shellcheck disable=SC2086
    printf 'error: unexpected file(s) in release directory (only supported archives + %s allowed):%s\n' "$CODEGG_CHECKSUM_FILE" "$unexpected" >&2
    exit 1
fi
printf 'directory ok: no unexpected files\n'

# --- 6+7. Archive payload + version smoke -----------------------------------------
TMP_EXTRACT_BASE=""
cleanup_extract() {
    if [ -n "$TMP_EXTRACT_BASE" ] && [ -d "$TMP_EXTRACT_BASE" ]; then
        rm -rf -- "$TMP_EXTRACT_BASE"
    fi
}
trap cleanup_extract EXIT HUP INT TERM
TMP_EXTRACT_BASE="$(mktemp -d "${TMPDIR:-/tmp}/codegg-verify.XXXXXX")"
chmod 700 "$TMP_EXTRACT_BASE"

# Detect the native target for smoke purposes (best effort, offline).
NATIVE_TARGET=""
if command -v rustc >/dev/null 2>&1; then
    host_line="$(rustc -vV 2>/dev/null | grep '^host:' | awk '{print $2}' || true)"
    if codegg_release_is_supported_target "${host_line:-}" >/dev/null 2>&1; then
        NATIVE_TARGET="$host_line"
    fi
fi
if [ -z "$NATIVE_TARGET" ]; then
    os="$(uname -s 2>/dev/null || printf 'unknown')"
    mach="$(uname -m 2>/dev/null || printf 'unknown')"
    case "$os/$mach" in
        Linux/x86_64|Linux/amd64) NATIVE_TARGET="x86_64-unknown-linux-gnu" ;;
        Linux/aarch64|Linux/arm64) NATIVE_TARGET="aarch64-unknown-linux-gnu" ;;
        Darwin/x86_64) NATIVE_TARGET="x86_64-apple-darwin" ;;
        Darwin/arm64|Darwin/aarch64) NATIVE_TARGET="aarch64-apple-darwin" ;;
        *) NATIVE_TARGET="" ;;
    esac
fi

SMOKE_RAN=0
for name in $MANIFEST_NAMES; do
    path="$DIR/$name"
    target="$(codegg_release_target_for_archive "$name")"

    members="$(tar -tzf "$path" 2>/dev/null)" || {
        printf 'error: cannot list archive members: %s\n' "$name" >&2
        exit 1
    }
    count="$(printf '%s\n' "$members" | wc -l | tr -d ' ')"
    if [ "$count" != "1" ]; then
        printf 'error: archive %s must contain exactly one member, found %s:\n%s\n' "$name" "$count" "$members" >&2
        exit 1
    fi
    member="$(printf '%s\n' "$members" | head -n 1)"
    case "$member" in
        /*)
            printf 'error: archive %s has absolute member: %s\n' "$name" "$member" >&2
            exit 1
            ;;
    esac
    case "$member" in
        *".."*|*"\\"*)
            printf 'error: archive %s has traversal member: %s\n' "$name" "$member" >&2
            exit 1
            ;;
    esac
    normalized="${member#./}"
    if [ "$normalized" != "$CODEGG_ARCHIVE_MEMBER" ]; then
        printf 'error: archive %s must contain exactly `%s`, found `%s`\n' "$name" "$CODEGG_ARCHIVE_MEMBER" "$member" >&2
        exit 1
    fi
    # Reject symlinks/devices: verbose entry must start with '-' (regular).
    verbose="$(tar -tvzf "$path" 2>/dev/null | head -n 1 || true)"
    case "$verbose" in
        "-"*) ;;
        *)
            printf 'error: archive %s member is not a regular file: %s\n' "$name" "$verbose" >&2
            exit 1
            ;;
    esac
    case "$verbose" in
        *" -> "*) printf 'error: archive %s member is a symlink: %s\n' "$name" "$verbose" >&2; exit 1 ;;
    esac
    printf 'payload ok: %s contains exactly `%s`\n' "$name" "$CODEGG_ARCHIVE_MEMBER"

    # Smoke: only the native target is executed.
    if [ "$SKIP_SMOKE" -eq 1 ]; then
        continue
    fi
    if [ -z "$NATIVE_TARGET" ]; then
        printf 'note: skipping version smoke for %s (host target unknown)\n' "$name"
        continue
    fi
    if [ "$target" != "$NATIVE_TARGET" ]; then
        printf 'note: skipping version smoke for %s (build host smoke covers non-native targets)\n' "$name"
        continue
    fi
    work="$TMP_EXTRACT_BASE/$target"
    mkdir -p -- "$work"
    tar -xzf "$path" -C "$work" || {
        printf 'error: extraction failed for %s\n' "$name" >&2
        exit 1
    }
    bin="$work/$CODEGG_ARCHIVE_MEMBER"
    if [ -L "$bin" ]; then
        printf 'error: extracted %s is a symlink\n' "$name" >&2
        exit 1
    fi
    if [ ! -f "$bin" ]; then
        printf 'error: extracted %s missing `%s`\n' "$name" "$CODEGG_ARCHIVE_MEMBER" >&2
        exit 1
    fi
    chmod 755 "$bin" 2>/dev/null || true
    if [ ! -x "$bin" ]; then
        printf 'error: extracted %s is not executable\n' "$name" >&2
        exit 1
    fi
    version_out="$("$bin" --version 2>&1)" || {
        printf 'error: `%s --version` failed: %s\n' "$name" "$version_out" >&2
        exit 1
    }
    printf 'version smoke: %s -> %s\n' "$name" "$version_out"
    case "$version_out" in
        *"codegg"*"0."*|*"codegg"*"1."*|*"codegg"*"2."*) ;;
        *)
            printf 'error: unexpected version output for %s: %s\n' "$name" "$version_out" >&2
            exit 1
            ;;
    esac
    if [ -n "$EXPECT_VERSION" ]; then
        case "$version_out" in
            *"$EXPECT_VERSION"*) ;;
            *)
                printf 'error: version mismatch for %s: expected %s, got: %s\n' "$name" "$EXPECT_VERSION" "$version_out" >&2
                exit 1
                ;;
        esac
    fi
    SMOKE_RAN=1
done

if [ "$SKIP_SMOKE" -eq 1 ]; then
    printf 'note: version smoke skipped (--skip-version-smoke)\n'
elif [ "$SMOKE_RAN" -eq 0 ] && [ -n "$NATIVE_TARGET" ]; then
    # Native archive absent (relaxed mode) or nothing executed.
    printf 'note: no native (%s) archive executed; native build-host smoke applies before packaging\n' "$NATIVE_TARGET"
elif [ "$SMOKE_RAN" -eq 0 ]; then
    printf 'note: version smoke not executed (no runnable target on this host)\n'
fi

trap - EXIT HUP INT TERM
rm -rf -- "$TMP_EXTRACT_BASE"
TMP_EXTRACT_BASE=""

printf 'release verification passed: dir=%s archives=%s\n' "$DIR" "$MANIFEST_COUNT"
