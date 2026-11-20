# Decision 105: A Standalone Program Takes Its Environment From `--env` Flags

Status: **Accepted, 2026-11-20.**
Every backend's generated `main` parses `--env NAME[=VAL]`, and the guest gets no other variable.
The WASI testsuite harness passes `--env` flags and no longer clears the child environment.
This replaces the environment row of [decision 31](31-standalone-runtime-interface.md).
It also replaces point 4 of [decision 40](40-wasi-p1-completion.md) (issue #376).

## Context

Decision 31 modelled the standalone runtime interface on Wasmtime's CLI.
The environment was the one part that did not follow: the whole process environment passed through.
That record gives no reason for it.
Java had handed the guest an empty environment, and it was moved to what the other backends did.
Wasmtime gives the guest no variable unless `--env` names it.

The difference had four costs:

- The WASI testsuite states the exact `environ` of each trial.
  Decision 36 rejected clearing the child environment, since that risks the interpreter launch.
  Decision 40 cleared it anyway, and resolved the interpreter against the parent `PATH` by hand.
  An interpreter that is a version manager's shim needs `HOME` and `PATH` to start.
  On a host where `perl` is such a shim, 67 of the 72 Perl trials failed.
- A variable the host adds reached the guest.
  macOS adds `__CF_USER_TEXT_ENCODING`, CPython adds `LC_CTYPE`, and Bash exports `PWD`, `SHLVL`, and `_`.
  So three count-exact `environ_*` trials were expected failures on five backends.
- The prebuilt Ruby adds `PKG_CONFIG_PATH`.
  That was one of the two reasons [decision 101](101-tool-versions-in-mise.md) builds Ruby from source.
- A converted program read every variable of the shell that ran it, secrets included.

## Decision

**The command line states the guest's environment, as it states the guest's directories.**
The rule behind it: a part of the interface that Wasmtime's CLI has takes Wasmtime's form.
A difference needs a recorded reason.

```
<runner> <program> [--dir HOST::GUEST | --env NAME[=VAL]]... [--] [guest args...]
```

- `--env NAME=VAL` gives the guest `NAME` with the value `VAL`; the name ends at the first `=`.
- `--env NAME` gives the guest the value `NAME` has in the process, and nothing when it is unset.
- With no `--env`, the guest's environment is empty.
- `--env X` and `--env=X` are both accepted, and the flag mixes with `--dir` in any order.
- `--env` with no argument prints `--env requires a NAME[=VAL] argument` and exits 1.

Each of these is what `wasmtime run` does, as measured on Wasmtime 50.0.2.
The guest of the measurement is [`wasi_standalone_env.wat`](../../examples/wat/wasi_standalone_env.wat).

Two points differ from Wasmtime, each for a reason:

- **A name given twice takes its last value.**
  Wasmtime gives the guest both entries.
  In library mode the environment is a map from name to value in four backends.
  Those are Ruby, Python, Perl, and Codon, and a map holds one value for a name.
- **The order of the variables is not part of the interface.**
  Wasmtime keeps the order of the flags.
  The Perl runtime sorts the names, since its map has no order of its own.

`--mode library` does not change: its caller already passes the environment.

The parser is in each backend's `src/lib.rs`, beside its `--dir` parser.
The reference for users is [`docs/standalone-interface.md`](../../docs/standalone-interface.md).

## Rejected alternatives

- **Keep the whole environment, and make the harness find the real interpreter behind a shim.**
  That ends the launch failure and nothing else.
  The variables a host adds still reach the guest, so the expected failures stay.
- **Keep the whole environment by default, and add a flag that clears it.**
  The default then still differs from Wasmtime, with no reason to record for it.
  A program also keeps reading secrets unless its user knows to ask otherwise.
- **Add a flag that passes the whole environment, like Wasmtime's `-S inherit-env`.**
  No program at hand needs it.
  It can be added later, and adding it breaks nothing.
- **Give the guest both entries of a name given twice.**
  That needs a list in place of the map in the library interface of four backends.
  A name with two values is also of no use to a guest: `getenv` returns one of them.
- **Refuse a name given twice.**
  A script that states a value and then replaces it with a later flag would stop working.

## Consequences

- A standalone program that reads a variable now needs `--env` for it.
  That is a change of behavior for every user of `--mode standalone`.
- The harness passes the trial's environment as flags, in `run_standalone_wasi`.
  It no longer clears the child environment or resolves the interpreter by hand.
  The code is in [`backend.rs`](../../crates/dewasm-test-helper/src/backend.rs).
- The 15 expected failures a host variable caused are removed.
  No backend has a host-scoped entry now.
  The mechanism of [decision 46](46-host-scoped-wasi-expected-failures.md) stays for the next one.
- The shared `standalone_env` case runs each form of the flag on all 7 backends.
  Its expected outputs are constants, taken from the Wasmtime measurement.
  The embedded Wasmtime runner does not rerun the case, since it parses flags of its own.
- Building Ruby from source has one reason left: ZJIT.
