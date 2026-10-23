# Decision 104: Pygments Runs on a CPython Reactor That Wizer Pre-Initializes

Status: **Accepted, 2026-10-23.**
The Rails example (decision 102) shows a post's body as the HTML it is served as, on request
(`?code=1`), highlighted by Pygments, a Python library.
Pygments runs on CPython 3.14.6 compiled to wasm and converted to Ruby.
The interpreter is a reactor of the example's own, `examples/rails/pygments/ext/pyg.c`, linked
against the `cpython-sdk` library build of `examples/apps/setup.sh`.
Wizer runs its `pyg_init` ahead of time and writes the initialized memory into the module.
The gem in `examples/rails/pygments/` builds all of that under `bundle install`.

## Context

A converted CPython takes 11 to 22 seconds to boot on Ruby, and `import pygments` another 27.
One highlight after that takes 0.4 seconds.
So the interpreter has to stay in the process, and its start-up has to happen before the process.
The prebuilt `cpython` app is a command: `_start` runs `main` and exits.
A snapshot of a command taken while it waits for input cannot be restored, since the wasm stack
it waits on is the Ruby call stack of the converted functions.

## Decision

**A reactor with an initialization export, pre-initialized by Wizer before the conversion.**

- A reactor, because a snapshot needs an empty wasm stack: an export that returns.
  `pyg_init` starts CPython, imports Pygments, and runs the common lexers once, so their regular
  expressions are compiled in the snapshot too.
- Wizer rather than a snapshot of the Ruby runtime, because it needs no change to dewasm.
  The pre-initialized module is ordinary wasm; the memory becomes data segments.
- The library build of CPython is an app of `examples/apps/setup.sh` (`cpython-sdk`).
  Its source version, checksum, and toolchain are fixed there like every other app's.
  The Rails side builds only what is specific to Pygments: the wheel, the link, Wizer, the
  conversion.
- The gem builds under `bundle install`, by the rule `dewasm_gem` follows.
  A missing SDK is an error there that names `setup.sh`; an out-of-date build is an error at
  boot that names `bundle install`.

Two conditions hold the snapshot together, and both are checked by use:

- The preopened directories are given in the same order at restore as at initialization.
  `wasi-libc` records each by its file descriptor number.
- No file is open at the snapshot point; the import system closes everything.

## Rejected alternatives

- **A CPython process per request.** 40 seconds each.
- **The command build kept resident, driven over pipes from a thread.**
  Measured to work (0.4 seconds per highlight after start-up), but the 40 seconds of start-up
  stay in every process, and the thread and the `$stdin` swap make it the odd piece of the
  example.
- **A snapshot taken by the Ruby runtime (`ruby-merman`'s approach).**
  It works for a reactor as well, and it is the fallback should a module ever not fit Wizer.
  It needs a snapshot format and a restore path in the runtime, which Wizer makes unnecessary.
- **Highlighting at save time, stored in the database.**
  The point of the page is that the work happens on request, where a profile shows it.
- **A before-and-after view of `sanitize` on the post page.**
  A view of the served HTML is the natural feature; that it lacks the `<script>` tag the author
  wrote is then something a reader notices, not something the page announces.

## Consequences

- Positive: the first highlight of a process takes about a second, the next ones 0.1 to 0.4.
  A flame graph of the HTML view shows `PygmentsExt#` frames under Rails.
- Negative: the converted file carries the memory image, 52 MB of Ruby; a `require` takes 2
  seconds.
  `examples/apps/setup.sh` gains a build of several minutes, and the tool list gains Wizer.
- Watch: a CPython or Pygments update changes the snapshot's inputs, which the stamp catches.
