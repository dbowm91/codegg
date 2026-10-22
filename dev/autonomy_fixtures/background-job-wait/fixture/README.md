# slowbuild

A one-step build that is **slow by design**. It waits about two minutes and
then writes a single artifact. It is never hung, it needs no network, no
compiler and no dependency — every step is a POSIX shell script that lives
in this directory.

## How to run the build

From the directory that contains `fixture/`:

```sh
sh fixture/build.sh
```

or, equivalently, from inside this directory:

```sh
sh build.sh
```

The build takes about two minutes (110 seconds) and exits `0` when it is
done. It writes exactly one file — `fixture/build-output.txt` — and touches
nothing outside this directory.

For a quick local sanity check you can shorten the wait:

```sh
BUILD_SLEEP_SECONDS=2 sh fixture/build.sh
```

The default (110 seconds) is the wait the build is supposed to run with.

## What it produces

`fixture/build-output.txt` is a plain-text manifest: three header lines
followed by the contents of `src/`, concatenated in name order.

```
build: ok
result: BUILD_OK
sources: 3
<the contents of src/01-header.txt, src/02-body.txt, src/03-footer.txt>
```

`result: BUILD_OK` is the build's result line; the same line is kept in
`expected/build-output.expected.txt` as a reference. The artifact only
appears once the build has finished, so its presence means the build
really ran to completion rather than being abandoned.

## The sources

`src/` holds the three blocks the build stages, in the order they are
staged:

* `src/01-header.txt` — project name and stage.
* `src/02-body.txt` — the payload lines.
* `src/03-footer.txt` — the closing block and the result line.

## Recommended way to run it

The build takes about two minutes, so do not sit in front of it:

1. Start `sh fixture/build.sh` as a **background job** — the `bash` tool
   with `execution_mode: "background"`. It returns a job handle right away.
2. Check the job's state with the `job` tool (`action: "status"`) and keep
   the job id; you will need it to wait later.
3. While the build runs, read this file and everything under `src/`.
4. Then wait for the job with the `job` tool (`action: "wait"`) and read
   the finished output (`action: "output"`).

A `wait` that times out means the build is **still running**, not that it
failed. A two-minute build is normal here, so wait patiently instead of
treating the slow call as a stall.
