# Decision 104: `pygments.rb` Runs on a CPython Reactor That Wizer Pre-Initializes

Status: **Accepted, 2026-10-23.**
The Rails example (decision 102) shows a post's body as the HTML it is served as, highlighted.
The switch is a ReActionView state, and the highlighting goes through the `pygments.rb` gem.
The browser asks the server for the branch as a `slots` request, the request a profile shows.
That gem spawns a `python3` child process, which runs the Pygments the gem comes with.
Converted, the process is a CPython 3.14.6 turned into Ruby, running in the Rails process.
It is a reactor of the recipe's own, `dewasm_cext/lib/dewasm_cext/recipes/pygments_rb/mentos_host.c`.
It links against the `cpython-sdk` library build of `examples/apps/setup.sh`.
Wizer runs its `pyg_init` ahead of time and writes the initialized memory into the module.
The gem's Ruby and Python are unmodified; `dewasm_gem "pygments.rb"` builds it like the others.

## Context

A converted CPython takes 11 to 22 seconds to boot on Ruby, and `import pygments` another 27.
One highlight after that takes 0.4 seconds.
So the interpreter has to stay in the process, and its start-up has to happen before the process.
The prebuilt `cpython` app is a command: `_start` runs `main` and exits.
A snapshot of a command taken while it waits for input cannot be restored.
The wasm stack it waits on is the Ruby call stack of the converted functions.

`pygments.rb` has no C extension.
So the point the other recipes replace, the `require` of the extension, does not exist.
Its boundary is the process: every API call goes through `Pygments::Popen#mentos`.
That method writes a JSON header and the code to the child and reads the answer back.

## Decision

**A reactor with an initialization export, pre-initialized by Wizer, behind the gem's `mentos`.**

- A reactor, because a snapshot needs an empty wasm stack: an export that returns.
  `pyg_init` starts CPython, imports the gem's own `mentos.py`, and runs the common lexers once.
  So their regular expressions are compiled in the snapshot too.
  `pyg_mentos` answers one request of the gem's protocol through the same `Mentos.get_data`.
- Wizer rather than a snapshot of the Ruby runtime, because it needs no change to dewasm.
  The pre-initialized module is ordinary wasm; the memory becomes data segments.
- The gem is replaced the way the C extension gems are: by a file found first.
  The recipe writes `lib_dewasm/pygments/popen.rb` and puts `lib_dewasm` first in the gem's `require_paths`.
  That file loads the gem's `popen.rb` and redefines `mentos`, `start`, `stop`, and `alive?`.
  Nothing in the gem's `lib` or `vendor` changes.
  The converted module reads them from the gem directory, preopened as `/gem`.
- The library build of CPython is an app of `examples/apps/setup.sh` (`cpython-sdk`).
  Its source version, checksum, and toolchain are fixed there like every other app's.
  The recipe builds only what is specific to the gem: the host C file, the link, Wizer, conversion.
- With the `dewasm` group excluded, the gem is the released `pygments.rb` and needs a `python3`.
  That is how the other four gems need their native extensions.

Two conditions hold the snapshot together, and both are checked by use:

- The preopened directory is the same path (`/gem`) at restore as at initialization.
  `wasi-libc` records a preopen by its file descriptor number and CPython by its path.
- No file is open at the snapshot point; the import system closes everything.

## Rejected alternatives

- **A CPython process per request.** 40 seconds each.
- **The command build kept resident, driven over pipes from a thread.**
  Measured to work: 0.4 seconds per highlight after start-up.
  But the 40 seconds of start-up stay in every process.
  The thread and the `$stdin` swap make it the odd piece of the example.
- **A snapshot taken by the Ruby runtime (`ruby-merman`'s approach).**
  It works for a reactor as well, and it is the fallback should a module ever not fit Wizer.
  It needs a snapshot format and a restore path in the runtime, which Wizer makes unnecessary.
- **A gem of the example's own over the Pygments wheel.**
  Built and measured first; it worked the same.
  But the example then had two mechanisms and an API of its own.
  `pygments.rb` is the gem a Rails application would use, and it is replaced by the same rule.
- **Highlighting at save time, stored in the database.**
  The point of the page is that the work happens on request, where a profile shows it.
- **A before-and-after view of `sanitize` on the post page.**
  A view of the served HTML is the natural feature.
  That it lacks the `<script>` tag the author wrote is then something a reader notices.
  The page does not announce it.

## Consequences

- Positive: the first highlight of a process takes about a second, the next ones 0.1 to 0.4.
  A flame graph of the HTML view shows `PygmentsRbExt#` frames under Rails.
  The application uses the gem's API and `Pygments.css` as is.
- Negative: the converted file carries the memory image, 52 MB of Ruby; a `require` takes 2 seconds.
  `examples/apps/setup.sh` gains a build of several minutes, and the tool list gains Wizer.
- Watch: a CPython update or a `pygments.rb` update changes the snapshot's inputs.
  The stamp catches that.
  `Pygments.engine` is one `Popen` per thread in the gem; the loader shares one interpreter under a lock.
- Watch: the two branches of the switch must differ in shape.
  Herb merges an `if`/`else` whose branches are both one output alone into one slot (Herb 0.11.1).
  That slot no longer reads the state, so the page would not switch back.
