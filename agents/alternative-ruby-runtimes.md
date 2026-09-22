# Running the Ruby suites on another Ruby

dewasm emits Ruby source, and which implementation runs that source is the user's choice.
Pointing the existing suites at a different Ruby measures two things at once: what that implementation supports, and what dewasm's output depends on.
This file is how to do that run and how to read it.

The worked example throughout is an ahead-of-time Ruby compiler, the case that produced decisions 96 and 97.

## The hook

[`dewasm_backend_ruby::find_ruby`](../crates/dewasm-backend-ruby/src/lib.rs) honors `$DEWASM_RUBY` and probes the candidate with `-e "print RUBY_VERSION"`, requiring 3.4 or newer.
Every suite that runs Ruby goes through it, so a wrapper standing in for `ruby` is the whole mechanism and the product needs no change.

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

Four things the wrapper must not break, because the suites assert on all of them: argv, stdin, the environment, and the exit status.
A compile failure belongs on stdout, since the spec harness prints the first lines of a script's stdout and stderr when it sees no `RESULT` line.
Keeping each script and each compile log is what makes a failure investigable afterwards: the harness writes scripts to temporary files that are gone by the time the run ends.

## The two suites

| Suite | Command | What one case is | What it compares |
| --- | --- | --- | --- |
| spec | `cargo test -p dewasm-backend-ruby --test spec` | one `.wast` file, converted and phrased as `check` calls | `RESULT pass=N fail=M` against the expected-failure list |
| WASI p1 | `cargo test -p dewasm-backend-ruby --test wasi_testsuite` | one conformance trial, run through the standalone interface | process exit code, and stdout where the manifest pins it |

Of the spec suite's 257 files, 97 produce a script; the rest convert nothing (unsupported proposals).
CRuby passes every one of both suites, so under another runtime each failure is that runtime's, and the CRuby run of the same script is the reference to diff against.

For repeated runs it is worth collecting the generated scripts once (set an env var per test and have the wrapper save the script under that name, one `cargo test --exact <name>` per file) and then driving the compiler over the collection directly.
That makes a run restartable, lets it be ordered smallest-first, and gives each script a stable name across runs.

## Reading a run

Classify each script, and compare the classification against the previous run rather than against nothing:

| Class | Meaning |
| --- | --- |
| `match` | identical `pass`/`fail` counts to CRuby on the same script |
| `value-mismatch` | ran, but some assertions answer differently |
| `compile-refusal` | the compiler refused the file |
| `segfault` / `timeout` | ran and died, or never finished |

The delta between two runs (fixed / regressed / changed) is the useful artifact; the absolute table mostly repeats what the previous one said.
Record the compiler's version string with each run: the same path can hold a different build an hour later, and a delta between two runs of the same build is silent about it.

## Narrowing a failure

- **Which assertion died.**
  Rebuild the script with `$stdout.sync = true` and a `TRY <desc>` line printed at the top of each `check` helper.
  A crash then names the assertion it died on, which separates an arithmetic bug from a stack-exhaustion one.
- **Delta debugging needs a two-sided oracle.**
  A candidate is accepted only if CRuby still runs it cleanly *and* the target still fails the same way.
  Requiring only the failure lets the reducer delete the path under test and keep a symptom that arrives by another route.
  One reduction here dropped an assignment, which sent the reduced file down a branch the real output never takes, and the conclusion drawn from it stood until the other project's own investigation corrected it.
  Requiring CRuby's stdout and the target's error text to stay byte-identical costs nothing and prevents it.
- **A hand-written distillation is not a reduction.**
  Where the compiler analyses the whole file, removing the neighbours removes the bug.
  Three reductions here only reproduced with dozens of untouched sibling methods around them, and the hand-written versions passed for reasons that had nothing to do with the bug.
  Keep the machine's output, and record which hand-written shapes did *not* reproduce: that boundary is itself evidence.

## What the first campaign found

Measured 2026-09-20 to 2026-09-22 against one ahead-of-time Ruby compiler, on the same dewasm output throughout, over twelve measured builds:

| Suite | Start | End |
| --- | --- | --- |
| spec, files matching CRuby | 68 of 97 | 97 of 97 |
| spec, assertions passing | — | 29396 of 29396 |
| WASI p1 trials passing | 34 of 72 | 71 of 72 |

Two changes landed on the dewasm side, both of them shapes that were no better under an interpreter: decision 96 (resolve at conversion time what conversion time knows, and prefer the spelling that compiles) and decision 97 (a host library the runtime may lack is optional, and its absence is refused rather than faked).
Everything else was the other project's, reported as a minimal pure-Ruby reproduction per class.

The one trial still failing is **WASI `rust/path_link`**, and it is a deliberate dewasm answer rather than a defect. Hardlinking a symlink itself needs `linkat(2)`, which only Fiddle reaches, so on a macOS host without Fiddle that one request answers `ENOTSUP` (decision 97). On a Linux host `File.link` is already nofollow, so the same build is expected to pass all 72.
