# Decision 102: Rails Example Runs Gem C Extensions Through a dewasm `ruby.h`

Status: **Accepted, 2026-10-23.**
`examples/rails` runs a Rails 8.1 blog application on Ruby alone.
The native extensions of `sqlite3`, `nokogiri`, `herb`, and `bcrypt` are compiled to wasm against
a dewasm-specific `ruby.h`, converted to Ruby, and loaded in place of the `.so` files.
Three parts landed:

- `examples/rails/cext/`: the header (`include/ruby.h`), its guest-side C (`support.c`), and the
  host runtime (`lib/dewasm_cext.rb`) that implements the Ruby C API as imports;
- `cext/lib/dewasm_cext/`: a recipe per gem (sources, flags, the `Init_*` symbol), the builder
  (fetch with a checksum, compile, convert, loader, stamp), and `dewasm_gem` for a Gemfile;
- the application, a Rails 8.1 blog with its tests, and its profile pages.

It replaces [decision 45](45-rails-sqlite3-shim-example.md).

## Context

Decision 45 bridged ActiveRecord to converted SQLite at the `sqlite3` gem API, with a shim gem.
The shim tracked the surface Rails calls, and it ruled out every guest-to-host callback.
So `create_function`, collations, and `busy_handler` were out, and each new gem would need a shim.
Nokogiri's Ruby-facing surface is too wide for one.

The question that reopened the design was whether a full Rails application can run on Ruby alone.
Rails itself needs `nokogiri` (through `rails-html-sanitizer`) and, with `load_defaults 8.2`, `herb`.
A blog needs a database and a sign-in hash: `sqlite3` and `bcrypt`.
`nio4r`, `websocket-driver`, and `racc` ship a Ruby side of their own.

A C extension does not call SQLite or libxml2 alone: it calls the Ruby C API.
In a wasm module that API can only be a set of imports.
dewasm's Ruby output already takes any import from the embedder (decision 7).
It exports the function table and `__stack_pointer` when the link asks for them.
And a Ruby exception raised inside an import unwinds through the generated frames by itself.
So the missing piece was a `ruby.h` whose functions are imports, and a host that serves them.

## Decision

**Bridge at the Ruby C API, not at a gem's API: compile the gem's own extension against a
dewasm `ruby.h`, and implement that API on the host over a handle table.**
The gem's Ruby code and C code both run unmodified.
The rules that make it work:

- A `VALUE` is a 32-bit tagged word: fixnum, the four constants, or a handle index shifted by 3.
  Classes, modules, symbols, and everything made during `Init_*` get permanent handles.
- A handle made during a call stays strong until a collection at depth 0.
  What keeps it strong is a registered address or a wrapped struct's `dmark`, as in CRuby.
  Any other handle turns weak: valid while its object lives anywhere in Ruby, as a `VALUE` is.
  C code keeps handles in memory the host cannot see, such as a libxml2 node's `_private`, and
  `dmark` on the array holding those nodes says nothing about their handles.
  The index is reused once the object is gone.
- A pointer into a Ruby String is a copy in linear memory.
  The host writes it back at every point the C code could observe a change.
  Those are a call into Ruby, `rb_str_set_len`, and the return from the outermost frame.
- `rb_raise` is a Ruby `raise`; `rb_protect`, `rb_ensure`, and `rb_rescue` are Ruby blocks.
  The host saves `__stack_pointer` at every entry and restores it on the way out.
- The data pointer of a wrapped object lives in a 4-byte guest cell, so `DATA_PTR(v) = p` assigns.
- One instance, one re-entrant lock: a wasm instance has one shadow stack and one heap.
- The variadic functions (`rb_raise`, `rb_sprintf`, `rb_scan_args`) are C in `support.c`.
  Imports are not variadic, and `PRIsVALUE` needs a formatter that knows about `VALUE`.

A gem whose extension has a Ruby fallback keeps the fallback (`agents/experiments.md`).
The converted C is 3 to 6 times slower than the fallback for `websocket-driver`.

## Rejected alternatives

- **A shim per gem (decision 45).**
  No callbacks, and a surface to track per gem and per Rails release.
  `nokogiri` would need its JRuby-sized Ruby layer written again.
- **Running the extensions in a wasm engine instead of converting them.**
  An engine gives no Ruby exception through wasm frames and no Ruby frame in a profile.
  The point of the example is that the process holds Ruby alone.
- **The `ruby.wasm` route: the whole interpreter and its extensions in one wasm binary.**
  That is a different product.
  It also makes every extension a static part of the interpreter build.
- **A 64-bit `VALUE` matching CRuby's.**
  wasm32 has 32-bit pointers; a wider `VALUE` would make every struct layout differ from C's.

## Consequences

- Positive: the four gems convert without a source change, and callbacks work.
  `create_function`, aggregates, collations, SAX handlers, and `rb_protect` all run.
  The application's `/stats` lists the native extensions loaded beyond Ruby's own: none.
  A profile shows Rails calling `Sqlite3Ext#sqlite3VdbeExec` as a Ruby method (decision 103).
- Negative: speed.
  Under YJIT the converted code runs 50 to 130 times slower than native.
  `bcrypt` at the default cost of 12 takes 19 seconds per sign-in, so the example uses cost 5.
  Herb's 1 second parse limit is raised for the converted parser.
- Negative: a callback into Ruby holds the instance's lock.
  Ruby code that waits for another thread in that callback deadlocks.
- Negative: WASI has no `iconv`, threads, `dlopen`, or shared memory.
  libxml2 decodes UTF-8, UTF-16, ISO-8859-1, and ASCII itself.
  SQLite runs without loadable extensions, and WAL mode degrades to a rollback journal.
- Carry-over: dewasm cext covers what four gems use, about 200 functions.
  A gem outside that set adds imports to `ruby.h` and `lib/dewasm_cext.rb`, and a recipe when the
  generic one (every `.c` under `ext/<name>/`) does not fit.
  `dewasm_gem` is a Gemfile method over a `path:` gem; a Bundler source plugin was weighed and
  left for a published gem, since it binds to Bundler's plugin API for the same result.
  The generated `Rt::Table` carries only the `callN` arities the module uses.
  So the host reads a slot directly; an entry point for any arity belongs in the product.
  Publishing the layer as a gem is possible later; it is not planned now.
