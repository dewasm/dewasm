# Running the Ruby suites on another Ruby

dewasm emits Ruby source, and which implementation runs that source is the user's choice.
Pointing the existing suites at a different Ruby measures two things at once.
They are what that implementation supports, and what dewasm's output depends on.
This file is how to do that run and how to read it.

The worked example is an ahead-of-time Ruby compiler, the case that produced decisions 96 and 97.

## The hook

[`dewasm_backend_ruby::find_ruby`](../crates/dewasm-backend-ruby/src/lib.rs) honors `$DEWASM_RUBY`.
It probes the candidate with `-e "print RUBY_VERSION"`, requiring 3.4 or newer.
Every suite that runs Ruby goes through it.
A wrapper standing in for `ruby` is then the whole mechanism, and the product needs no change.

```bash
#!/bin/bash
# $DEWASM_RUBY stand-in: compile the script, then exec the binary.
set -uo pipefail
if [ "${1-}" = "-e" ]; then exec ruby "$@"; fi   # the find_ruby version probe

script=$1; shift
out=$CACHE/$(basename "$script" .rb)
cp "$script" "$LOGS/"                            # keep it: a failure is investigated by recompiling this one file
if ! <compiler> "$script" -o "$out" > "$LOGS/compile.log" 2>&1; then
  echo "COMPILE-FAIL"; head -20 "$LOGS/compile.log"; exit 91
fi
exec "$out" "$@"
```

The wrapper must not break four things, because the suites assert on all of them.
They are argv, stdin, the environment, and the exit status.
With no `RESULT` line, the spec harness prints the first lines of a script's stdout and stderr.
A compile failure therefore belongs on stdout.
The harness writes scripts to temporary files that are gone by the time the run ends.
Keeping each script and each compile log is then what makes a failure investigable afterwards.

## The two suites

| Suite | Command | What one case is | What it compares |
| --- | --- | --- | --- |
| spec | `cargo test -p dewasm-backend-ruby --test spec` | one `.wast` file, converted and phrased as `check` calls | `RESULT pass=N fail=M` against the expected-failure list |
| WASI p1 | `cargo test -p dewasm-backend-ruby --test wasi_testsuite` | one conformance trial, run through the standalone interface | process exit code, and stdout where the manifest pins it |

Of the spec suite's 257 files, 97 produce a script.
The rest convert nothing (unsupported proposals).
CRuby passes every case of both suites, so under another runtime each failure is that runtime's.
The CRuby run of the same script is then the reference to diff against.

For repeated runs, collect the generated scripts once, then drive the compiler over them directly.
To collect them, set an env var per test and have the wrapper save the script under that name.
Run one `cargo test --exact <name>` per file.
That makes a run restartable and lets it be ordered smallest-first.
It also gives each script a stable name across runs.

## Reading a run

Classify each script.
Compare the classification against the previous run rather than against nothing:

| Class | Meaning |
| --- | --- |
| `match` | identical `pass`/`fail` counts to CRuby on the same script |
| `value-mismatch` | ran, but some assertions answer differently |
| `compile-refusal` | the compiler refused the file |
| `segfault` / `timeout` | ran and died, or never finished |

The delta between two runs (fixed / regressed / changed) is the useful artifact.
The absolute table mostly repeats what the previous one said.
Record the compiler's version string with each run.
The same path can hold a different build an hour later.
A delta between two runs of the same build is silent about that.

## Narrowing a failure

- **Which assertion died.**
  Rebuild the script with `$stdout.sync = true`.
  Print a `TRY <desc>` line at the top of each `check` helper.
  A crash then names the assertion it died on.
  That separates an arithmetic bug from a stack-exhaustion one.
- **Delta debugging needs a two-sided oracle.**
  A candidate is accepted only if CRuby still runs it cleanly *and* the target still fails the same way.
  Requiring only the failure lets the reducer delete the path under test.
  It then keeps a symptom that arrives by another route.
  One reduction here dropped an assignment.
  That sent the reduced file down a branch the real output never takes.
  The conclusion drawn from it stood until the other project's own investigation corrected it.
  Requiring CRuby's stdout and the target's error text to stay byte-identical prevents it.
  The requirement costs nothing.
- **A hand-written distillation is not a reduction.**
  Where the compiler analyses the whole file, removing the neighbours removes the bug.
  Three reductions here only reproduced with dozens of untouched sibling methods around them.
  Their hand-written versions passed for reasons that had nothing to do with the bug.
  Keep the machine's output, and record which hand-written shapes did *not* reproduce.
  That boundary is itself evidence.

## What the first campaign found

Measured 2026-09-20 to 2026-09-22 against twelve builds of one ahead-of-time Ruby compiler.
The dewasm output was the same throughout:

| Suite | Start | End |
| --- | --- | --- |
| spec, files matching CRuby | 68 of 97 | 97 of 97 |
| spec, assertions passing | — | 29396 of 29396 |
| WASI p1 trials passing | 34 of 72 | 71 of 72 |

Two changes landed on the dewasm side, both shapes that were no better under an interpreter:

- Decision 96 resolves at conversion time what conversion time knows.
  It prefers the spelling that compiles.
- Decision 97 makes optional a host library the runtime may lack.
  Its absence is refused rather than faked.

Everything else was the other project's, reported as a minimal pure-Ruby reproduction per class.

The one trial still failing is **WASI `rust/path_link`**.
It is a deliberate dewasm answer rather than a defect.
Hardlinking a symlink itself needs `linkat(2)`, which only Fiddle reaches.
On a macOS host without Fiddle, that one request then answers `ENOTSUP` (decision 97).
On a Linux host `File.link` is already nofollow, so the same build is expected to pass all 72.
