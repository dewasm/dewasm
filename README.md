# dewasmify

dewasmify translates WebAssembly binaries into **source code** of various
programming languages. The generated code embeds a lightweight runtime and
needs no wasm runtime at all — a program compiled from C/C++/Rust to wasm
can run anywhere the target language runs.

Unlike existing single-target translators (wasm2c, w2c2, wasm2go, unwasm,
wasm2lua, ...), dewasmify is built around one shared IR with pluggable
language backends.

**Status: early development.** The Ruby backend is functional and passes
the WebAssembly core spec testsuite (19,400+ assertions) for the supported
feature set; a useful subset of WASI preview 1 works end-to-end.

## Supported input

- Wasm core spec 1.0 (MVP), plus the extensions that C/Rust toolchains
  enable by default: mutable globals, sign-extension operators, saturating
  float-to-int, multi-value, bulk memory (`memory.copy/fill/init`, passive
  data segments)
- WASI preview 1 (stdio, args, environ, clocks, random, proc_exit;
  filesystem APIs are not implemented yet)
- Not yet: reference types, SIMD, threads, GC, multiple memories/tables,
  cross-module linking

## Target languages

| Language | Status |
|---|---|
| Ruby | ✅ works, spec testsuite green |
| Bash | planned (pure-bash softfloat for f32/f64) |
| Java | planned |
| Go | planned |
| Python | planned |
| PHP | planned |

## Usage

```console
$ cargo run -p dewasmify-cli -- input.wasm --target ruby --mode standalone -o out.rb
$ ruby out.rb            # runs _start with WASI wired up
```

Library mode exposes the module as a class instead:

```console
$ cargo run -p dewasmify-cli -- add.wat -o add.rb   # .wat input also accepted
```

```ruby
require_relative "add"
inst = Add.new                  # pass an imports table if the module needs one
inst.invoke("add", 2, 3)        # => 5
inst.memory                     # linear memory, if any
```

WASI imports work out of the box in library mode: functions the embedder
does not provide fall back to the bundled WASI (constructed only if
actually needed; disable with `--no-default-wasi`). An imports-table
value is either a Hash of callables or a *provider* object — implement
`import(name)` and optionally `attach(instance)` to replace a whole
namespace, e.g. a custom WASI runtime:

```ruby
class MyWasi
  def import(name) = respond_to?(name) ? method(name) : nil
  def attach(instance) = @memory = instance.memory  # bound before wasm runs
  def fd_write(fd, iovs, iovs_len, out_ptr) = ...   # @memory.bytes is a binary String
end

inst = Prog.new({ "wasi_snapshot_preview1" => MyWasi.new })
inst.invoke("_start")           # proc_exit raises Prog::Rt::Exit — rescue it
```

A real example — a Rust program compiled for `wasm32-wasip1` (including
`std`) converts to a single ~1.5 MB Ruby file and produces output identical
to wasmtime:

```console
$ rustc --target wasm32-wasip1 -O main.rs -o demo.wasm
$ cargo run -p dewasmify-cli -- demo.wasm --mode standalone -o demo.rb
$ ruby demo.rb foo bar
Hello from Rust compiled to wasm!
args: ["foo", "bar"]
```

## How it works

- `crates/dewasmify-core` decodes and validates the binary (wasmparser) and
  builds a structured IR: wasm's control flow (block/loop/if/br) is kept
  as-is — no relooper needed — and the value stack is flattened into
  variables per (depth, type) slot, wasm2c-style.
- `crates/dewasmify-backend-*` lower the IR per language. For Ruby:
  - i32/i64 are masked unsigned Integers; signed views only where needed
  - f32 results are re-rounded to single precision; NaN bit patterns are
    preserved with software bit conversions where MRI's `pack` would
    canonicalize them
  - multi-level `br` becomes catch/throw; loops become `while true` +
    catch whose value distinguishes continue from fallthrough
- `runtime/<lang>/units/` holds the runtime as per-method units with
  declared dependencies (linear memory, table + `call_indirect` type
  checks, numeric helpers, traps, WASI); only the units a module actually
  needs are bundled into the output, nested inside the generated class so
  multiple generated files can coexist in one process.

## Design decisions

Significant decisions are recorded as Architecture Decision Records in
[`docs/adr/`](docs/adr/README.md) — start with
[ADR-0](docs/adr/0-foundation.md) for the goal and architecture. The IR
shape, the numeric semantics strategy, the testing approach, and the
per-backend lowering conventions each have their own ADR.

## Testing

The spec harness (`crates/dewasmify-cli/tests/spec.rs`) parses the official
testsuite's `.wast` files, converts every module with the Ruby backend, and
generates a Ruby script that runs all assertions on the real interpreter:

```console
$ git submodule update --init   # fetches tests/spec (WebAssembly/testsuite)
$ cargo test -p dewasmify-cli --test spec -- --nocapture
...
TOTAL: pass=19446 fail=5 skip=894 (rust: invalid-ok=1563 invalid-bad=0)
```

Directives that exercise unsupported features are skipped; the 5 expected
failures come from cross-module table sharing (`register`), which is not
supported yet. Adding a backend = making this harness pass for it — the
full policy is [ADR-3](docs/adr/3-testing-strategy.md).

## Roadmap

1. ~~Core pipeline + Ruby backend + spec harness~~ (done)
2. WASI filesystem support (path_open + preopens), more real-world programs
3. Bash backend (integers/memory/WASI first, then pure-bash IEEE754
   softfloat — no external commands)
4. Java / Go backends
5. Python / PHP backends
6. Readability pass (expression folding), self-hosting demo (dewasmify →
   wasm → Ruby → run dewasmify on Ruby)
