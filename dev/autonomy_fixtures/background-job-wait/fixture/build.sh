#!/bin/sh
# The slowbuild build step for the background-job-wait case.
#
# It is a real, self-contained build: no network, no dependencies, no
# compiler. It waits (long enough to be worth backgrounding) and then
# stages src/ into build-output.txt next to itself. It reads and writes
# nothing outside this directory.
#
# The wait is the point of the case. The default is 110 seconds, which is
# "roughly two minutes"; BUILD_SLEEP_SECONDS shortens it for local
# experimentation only.
set -eu

script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
out="$script_dir/build-output.txt"
src_dir="$script_dir/src"

sleep_seconds=${BUILD_SLEEP_SECONDS:-110}

printf 'slowbuild: waiting %ss before staging\n' "$sleep_seconds"
sleep "$sleep_seconds"

# The staging step: the three header lines, then every source file in name
# order. Written through a temporary file so a reader never sees a
# half-written artifact.
tmp="$out.tmp.$$"
trap 'rm -f "$tmp"' EXIT INT TERM
{
    printf 'build: ok\n'
    printf 'result: BUILD_OK\n'
    printf 'sources: %s\n' "$(find "$src_dir" -maxdepth 1 -type f -name '*.txt' | wc -l | tr -d ' ')"
    find "$src_dir" -maxdepth 1 -type f -name '*.txt' | LC_ALL=C sort | while IFS= read -r source; do
        cat "$source"
    done
} > "$tmp"
mv "$tmp" "$out"

printf 'slowbuild: wrote %s\n' "$out"
exit 0
