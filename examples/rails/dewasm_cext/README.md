# dewasm cext

A gem's C extension, compiled to wasm against a `ruby.h` of this directory's own.
dewasm converts it to Ruby, and the result is loaded in place of the `.so` file.
The gem's own Ruby and C code run unmodified.
The gems with a recipe are `sqlite3`, `nokogiri`, `herb`, `bcrypt`, and `pygments.rb`.

## In a Gemfile

```ruby
require_relative "dewasm_cext/lib/dewasm_cext/gemfile"

gem "sqlite3", ">= 2.1"

group :dewasm do
  dewasm_gem "sqlite3", "2.10.1"
end
```

`dewasm_gem` fetches the gem at that version and compiles its extension against the `ruby.h`.
It converts the result and hands it to Bundler as a `path:` gem under `vendor/dewasm/`.
The line replaces the `gem` line that names the same gem.
It also adds a gem no line names, such as `nokogiri`, a dependency of `rails`.
It builds under `bundle install` only, or with `DEWASM_CEXT_BUILD=1`.
An out-of-date build at boot is an error that names the command.
With the group excluded (`BUNDLE_WITHOUT=dewasm`), the line does nothing.
Bundler then takes the gem's own native extension.

The build needs `wasi-sdk` with `WASI_SDK_PATH` set, and `make` and `patch` for `libxml2`.
It needs Wizer for `pygments.rb`, and network access for the gems.
`mise install` at the repository root provides `wasi-sdk` and Wizer.
The `pygments.rb` build links against the CPython library of `examples/apps/scripts/cpython-sdk.sh`.
That script must have run first.
The first `bundle install` takes a few minutes, most of it `libxml2`.

## Pieces

- `bin/build-gem <gem> <version>`: the command-line form of `dewasm_gem`.
  It builds into `vendor/dewasm/` under the working directory.
- `bin/compile-ext <name> <Module> <Init_symbol> <out dir> <c sources...>`: an extension that is no gem.
  It is compiled and converted the same way.
- `test/run.sh`: the checks against a small extension (`test/demo_ext.c`).
  They cover strings, exceptions through guest frames, `rb_protect`, and blocks.
  They also cover typed data and handle collection.
- `test/smoke/<gem>.rb <gem directory>`: each gem's own API on its converted extension.
  Callbacks are included: SQLite functions and collations, SAX handlers.

## How it works

`include/ruby.h` declares each `rb_*` function as an import from module `ruby`.
A `VALUE` is a 32-bit tagged word: a fixnum, one of the four constants, or a handle index.
`include/support.c` holds what needs no host: `rb_scan_args`, `rb_sprintf`, `st_table`.

`lib/dewasm_cext/recipes/<gem>.rb` says how a gem's extension is built.
That is its sources, its flags, the `Init_*` symbol, and the path the gem requires it from.
The `nokogiri` recipe also builds `libxml2` and `libxslt` for wasm from the archives the gem ships.
A gem without a recipe gets the generic one, every `.c` under `ext/<name>/`.
`lib/dewasm_cext/builder.rb` runs a recipe: fetch with a checksum, unpack, compile, convert.
It then writes the loader and stamps the result, so that a changed input rebuilds it.
`lib/dewasm_cext/gemfile.rb` is `dewasm_gem`.

`lib/dewasm_cext.rb` is the host.
It keeps the handle table and defines the Ruby methods a `rb_define_method` call asks for.
It calls the C function through the module's exported function table.
A pointer into a Ruby String is a copy in linear memory.
The copy is written back when the C code returns or calls into Ruby.
A Ruby exception raised inside an import unwinds through the generated frames on its own.
The host saves `__stack_pointer` at each entry and restores it, since no epilogue runs.
Handles made during a call are collected once the outermost call returns.
One that a registered address or a wrapped struct's `dmark` marks stays strong.
Any other turns weak and stays valid while its object lives anywhere in Ruby, as in CRuby.
It covers what these gems use, about 200 functions.
An extension that calls another one fails at load with the missing import's name.

`pygments.rb` has no C extension; its boundary is the `python3` process it talks to over a pipe.
The recipe builds a reactor around CPython, `recipes/pygments_rb/mentos_host.c`, with two exports.
`pyg_init` starts CPython, imports the gem's `mentos.py`, and runs the common lexers once.
`pyg_mentos` answers one request of the gem's protocol.
A converted CPython, however, takes more than 30 seconds to boot and import Pygments.
That is too long to pay at every start of the Rails process.
So the initialization happens once, at build time.
Wizer runs `pyg_init` and writes the memory it leaves into the wasm module.
Now, the converted Ruby starts from an interpreter that is already initialized.
The first highlighting takes about a second, the next ones 0.1 to 0.4 seconds.
The gem is replaced by a file found first: the recipe writes `lib_dewasm/pygments/popen.rb`.
That file sits ahead of the gem's `lib` on its require paths.
It loads the gem's `popen.rb` and points `Pygments::Popen#mentos` at the reactor.

## Limits

- Under YJIT the converted code runs 50 to 130 times slower than native.
  `bcrypt` at its default cost of 12 takes 19 seconds per hash.
- A callback into Ruby holds the instance's lock.
  Ruby code that waits for another thread inside such a callback deadlocks.
- WASI has no `iconv`, threads, `dlopen`, or shared memory.
  `libxml2` decodes UTF-8, UTF-16, ISO-8859-1, and ASCII documents itself.
  SQLite runs without loadable extensions.
  A database left in WAL mode by a native SQLite does not open.
