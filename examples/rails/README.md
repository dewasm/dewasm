# Rails on Ruby alone

A Rails 8.1 blog application whose process holds no native extension beyond Ruby's own.
The C extensions it needs are compiled to wasm, converted to Ruby by dewasm, and loaded in place
of the `.so` files:

| Gem | What its C extension carries | Converted Ruby |
| --- | --- | --- |
| `sqlite3` 2.10.1 | the gem's extension and SQLite 3.54.1 | 7.3 MB |
| `nokogiri` 1.19.4 | the gem's extension, `libxml2` 2.13.9, `libxslt` 1.1.43, `gumbo` | 9.1 MB |
| `herb` 0.11.1 | the HTML+ERB parser Rails 8.2 compiles templates with, and Prism | 3.9 MB |
| `bcrypt` 3.1.22 | the hash behind `has_secure_password` | 0.2 MB |
| `pygments.rb` 5.0.0 | no C: the Python process it runs the Pygments it ships in, so CPython 3.14.6 itself, with Pygments 2.20.0 imported | 52 MB |

The gems' own Ruby and C code run unmodified.
What makes that possible is dewasm cext, under `cext/`: a `ruby.h` of this example's own, in
which every Ruby C API function is a wasm import, and `cext/lib/dewasm_cext.rb`, which
implements the API on the host over a handle table.
The application is a `rails new` application; its Gemfile asks for the converted gems in one
line each:

```ruby
require_relative "cext/lib/dewasm_cext/gemfile"

gem "sqlite3", ">= 2.1"

group :dewasm do
  dewasm_gem "sqlite3", "2.10.1"
end
```

`dewasm_gem` fetches the gem at that version, compiles its extension against the `ruby.h`,
converts it with dewasm, and hands Bundler the result as a `path:` gem under `vendor/dewasm/`.
The line replaces the `gem` line that names the same gem, and adds a gem no line names, such as
`nokogiri`, a dependency of `rails`.
It builds under `bundle install` only.
An out-of-date build at boot is an error that names the command.
The gems with a Ruby fallback of their own (`nio4r`, `websocket-driver`, `racc`) use it.
`puma` is replaced by WEBrick, and Bootsnap is left out.

```
Rails 8.1 (unmodified), templates compiled by Herb
  └─ sqlite3, nokogiri, herb, bcrypt: the gems' Ruby (unmodified)
       └─ <gem>_wasm.rb: the gem's C extension converted by dewasm (generated)
            └─ dewasm cext (cext/lib/dewasm_cext.rb): the Ruby C API as imports (rb_str_new, ...)
```

## Run it

```console
$ bundle install
$ bin/rails db:setup
$ bin/rails server
```

`bundle install` fetches the four gems at fixed versions, compiles their extensions with
`wasi-sdk`, and converts them; the first time takes a few minutes, most of it `libxml2`.
`bin/rails db:setup` creates the database and seeds one account and two posts.
`bin/rails server` serves on WEBrick at `http://127.0.0.1:3000`.
Reading needs no account; to write a post, sign in as `author@example.com` with `secret`.
`/stats` reports what the process runs on, and which native extensions it loaded beyond Ruby's
own: none.

`bin/rails test` drives the application through its stack: the index, a sign-in, a post with
HTML in its body, a comment, a profiled request with its flame graph, and the `/stats` report.

Requirements: Ruby 4.0, `wasi-sdk` with `WASI_SDK_PATH` set, `make` and `patch` for libxml2,
Wizer for `pygments.rb`, and network access for the gems.
`mise install` at the repository root provides Ruby, `wasi-sdk`, and Wizer.
`examples/apps/setup.sh` must have run first: the `pygments.rb` build links against the CPython
library it builds (`cpython-sdk`), and `bundle install` says so when it is missing.

Pieces, individually:

- `cext/bin/build-gem <gem> <version>`: the command-line form of `dewasm_gem`, into
  `vendor/dewasm/` under the working directory.
- `cext/test/run.sh`: dewasm cext's own checks against a small extension (`test/demo_ext.c`).
  They cover strings, exceptions through guest frames, `rb_protect`, blocks, typed data, and
  handle collection.
- `cext/test/smoke/<gem>.rb <gem directory>`: each gem's own API on its converted extension,
  including the callbacks the earlier shim could not offer: SQLite functions and collations, SAX
  handlers.
- `script/profile.rb` and `script/flamegraph.rb`: a Vernier profile of request handling, and a
  flame graph from it (below).

## dewasm cext (`cext/`)

`include/ruby.h` declares each `rb_*` function as an import from module `ruby`.
A `VALUE` is a 32-bit tagged word: a fixnum, one of the four constants, or a handle index.
`include/support.c` holds what needs no host: `rb_scan_args`, `rb_sprintf`, `st_table`.

`lib/dewasm_cext/recipes/<gem>.rb` says how a gem's extension is built: its sources, its flags,
the `Init_*` symbol, and the path the gem requires its extension from.
The `nokogiri` recipe also builds `libxml2` and `libxslt` for wasm from the archives the gem
ships.
A gem without a recipe gets the generic one, every `.c` under `ext/<name>/`.
`lib/dewasm_cext/builder.rb` runs a recipe: fetch with a checksum, unpack, compile, convert,
write the loader, and stamp the result so that a changed input rebuilds it.
`lib/dewasm_cext/gemfile.rb` is `dewasm_gem`.

`lib/dewasm_cext.rb` is the host.
It keeps the handle table, defines the Ruby methods a `rb_define_method` call asks for, and calls
the C function through the module's exported function table.
A pointer into a Ruby String is a copy in linear memory, written back when the C code returns or
calls into Ruby.
A Ruby exception raised inside an import unwinds through the generated frames on its own.
The host saves `__stack_pointer` at each entry and restores it, since no epilogue runs.
Handles made during a call are collected once the outermost call returns.
One that a registered address or a wrapped struct's `dmark` marks stays strong; any other turns
weak, and stays valid while its object lives anywhere in Ruby, as a `VALUE` does in CRuby.

It covers what these four gems use, about 200 functions.
An extension that calls another one fails at load with the missing import's name.

## Profiles in the application

The page header names the mode the four gems run in, `dewasm gems` or `native gems`, with a color
of its own, and the time the page took on the server up to the header.
The page header has a `Profiling: off` button.
Turned on, every following page request is recorded by Vernier, and the page served links to its own
flame graph; `?profile=1` records a single request.
Assets and the profile pages themselves are not recorded.
`/profiles` lists the recordings with the mode each was taken in, in pages, with a search over
the path and the mode; `/profiles/<id>` draws one.
A click on a frame narrows the picture to it: the frame becomes the bottom row, the full width,
and a click on that row widens the picture by one level.
A link saves the recording for the Firefox Profiler.
`lib/request_profiler.rb` is the Rack layer that records, `lib/flame_graph.rb` the drawing.

Frames are colored by the kind of code they run: a converted gem, a native gem's C method,
dewasm cext, a C function of Ruby itself, or Ruby (the application, Rails, and the other gems).
The gem a frame belongs to is in the frame's title, and in the summary under the picture.
With `--function-names` (the conversion flag `cext/lib/dewasm_cext/compiler.rb` passes), a
frame reads `Sqlite3Ext#sqlite3VdbeExec`.

Vernier is a C extension, and it is the one exception to "Ruby alone".
It loads on the first profiled request only, and `/stats` reports it apart as instrumentation.
A profiler in Ruby alone was tried and measured (`agents/experiments.md`): a sampling thread gets
the interpreter about 8 times a second, 84 with `RUBY_THREAD_TIMESLICE=1`, which is too few
for one request.

## Pygments on a converted CPython

A post page has a `View HTML` button: the body is then shown as the HTML it is served as,
highlighted, in place of the rendering.
The seeded post's HTML has no `<script>` tag, since `sanitize` removed the one its author wrote.
The button is a ReActionView state (`herb:state` in `app/views/posts/show.html.erb`): the browser
flips it and asks the server for the branches that read it, as a `slots` request.
That request renders the HTML branch, so it is what a profile of the switch shows; with profiling
on, it is recorded like any other request.
The highlighting is `pygments.rb`, used as any Rails application would (`Pygments.highlight`,
`Pygments.css`).
That gem runs the Pygments it ships in a `python3` child process and talks to it over a pipe.
Converted, the process is CPython 3.14.6 compiled to wasm and converted to Ruby, in the Rails
process; the gem's Ruby and Python are unmodified.
It runs at page load; nothing is stored.

The recipe (`cext/lib/dewasm_cext/recipes/pygments_rb.rb`) builds a reactor around CPython,
`mentos_host.c`, with two exports: `pyg_init` starts CPython, imports the gem's `mentos.py` and
runs the common lexers once, and `pyg_mentos` answers one request of the gem's protocol.
A converted CPython takes about 40 seconds to boot and import Pygments.
So Wizer runs `pyg_init` ahead of time and writes the memory it leaves into the module, and the
Ruby code starts from that memory: the first highlight takes about a second (YJIT warms up), the
next ones 0.1 to 0.4 seconds.
The gem is replaced the way the C extension gems are, by a file found first: the recipe writes
`lib_dewasm/pygments/popen.rb`, ahead of the gem's `lib` on its require paths, which loads the
gem's `popen.rb` and points `Pygments::Popen#mentos` at the reactor.
The `cpython-sdk` app of `examples/apps/setup.sh` is the library build the reactor links against.

## The same application on the native extensions

```console
$ bundle config set --local without dewasm
$ bundle install
```

With the `dewasm` group excluded, each `dewasm_gem` line does nothing, and Bundler takes the
gems' own native extensions, as in any Rails application; the lock follows.
`bundle config unset --local without` and `bundle install` switch back.
`bin/rails test` passes in both modes, and in the native one expects the four extensions in
`/stats`.
`script/flamegraph.rb` draws a recorded request from `storage/profiles/<id>.json`.
With the 26 posts the test seed makes on the index, on an M-series laptop:

| | converted | native |
| --- | --- | --- |
| `GET /posts` | 2.2 s (10860 samples) | 92 ms (460 samples) |
| self time in SQLite | 60% (`Sqlite3Ext#sqlite3VdbeExec` and its callees, as Ruby frames) | 1% (`SQLite3::Statement#step`, one C frame) |
| self time in Nokogiri | 15% | 5% |
| self time in dewasm cext | 2% | — |

In the converted picture, a query fans out into the SQLite functions it runs; in the native one,
the same query is a single dark leaf, and most of the time is Rails itself.
A native gem's C method has a color of its own, so the two pictures tell apart at a glance.

## Profile from the command line

```console
$ bin/rails runner script/profile.rb tmp/profile.json
$ ruby script/flamegraph.rb tmp/profile.json tmp/flamegraph.svg
```

`script/profile.rb` drives the application in one process through the integration session and
records it with Vernier; `script/flamegraph.rb` draws the flame graph and prints where the time
goes.
The recording opens in the Firefox Profiler.

Ten rounds of index, post page, comment, and sign-in on an M-series laptop:

| Where the code lives | Self time |
| --- | --- |
| converted `sqlite3` | 45% |
| Ruby: the application, Rails, and the other gems | 25% |
| converted `bcrypt` | 11% |
| converted `nokogiri` | 10% |
| Ruby's own C functions (`IO::Buffer#get_value`, `Integer#^`, ...) | 8% |
| dewasm cext | 1% |

## Limits

- Speed: under YJIT the converted code runs 50 to 130 times slower than native.
  A page with a few queries renders in tens of milliseconds; a template compiles in about a second.
  `bcrypt` at its default cost of 12 takes 19 seconds per sign-in, so
  `config/initializers/bcrypt_cost.rb` sets cost 5.
  Herb's 1 second parse limit is raised in `config/initializers/reactionview.rb`.
- A callback into Ruby holds the instance's lock.
  Ruby code that waits for another thread inside such a callback deadlocks.
- WASI has no `iconv`, threads, `dlopen`, or shared memory.
  libxml2 decodes UTF-8, UTF-16, ISO-8859-1, and ASCII documents itself.
  SQLite runs without loadable extensions, and `config/database.yml` sets a rollback journal
  (`journal_mode: delete`), since a database left in WAL mode by a native SQLite does not open.
- Rails 8.1: the Herb template handler comes from ReActionView, which carries the class Rails 8.2
  ships, and its reactive templates drive the `View HTML` switch.
  Its developer tools are off, since their element outlines keep the slot client from placing a
  fetched branch (ReActionView 0.6.0, with the native Herb too).
  Its development server is off, since the file watcher it uses (`cruise`) is a native extension.
