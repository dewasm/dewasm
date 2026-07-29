# dewasm

dewasm translates a WebAssembly binary into the **source code** of an
ordinary programming language. The generated file embeds a small runtime and
needs no wasm engine at all: a program compiled from C, C++, Rust, or Zig to
`wasm32-wasip1` becomes a single `.rb`, `.sh`, `.py`, `.go`, or `.java` file
that runs anywhere that language runs. One shared IR feeds every backend, so
each new target is a lowering table plus runtime units rather than a fresh
translator ([docs/related-work.md](docs/related-work.md) compares the prior
art).

Here is `cowsay`, a C program, converted to a **pure Bash script** and run
with nothing but a shell:

```console
$ dewasm examples/apps/cache/cowsay.wasm --target bash --mode standalone -o cowsay.sh
$ echo "Hello from Bash" | bash cowsay.sh
 _________________
< Hello from Bash >
 -----------------
        \   ^__^
         \  (oo)\_______
            (__)\       )\/\
               ||----w |
                ||     ||
```

The same binary — and much larger ones, up to SQLite and CPython — convert to
Ruby, Python, Go, and Java just as well.

## Status

**Early development, heading toward 0.1.** All five backends pass the
WebAssembly core spec testsuite for the supported feature set (~29,000
assertions each) and implement WASI preview 1. This is real, tested output,
not a demo: real-world binaries are proven byte-identical to `wasmtime` (see
[what it can convert](#what-it-can-convert)).

## Target languages

`--target <name>`; every backend shares the same input dialect and the same
spec gate.

| Target | Character | Spec status |
| --- | --- | --- |
| `ruby` | The most complete backend; full WASI p1 incl. the filesystem; the SQLite/CRuby north-star host | spec-green |
| `bash` | Runs where the only dependency is a shell; integer + a pure-Bash IEEE-754 softfloat, no external commands; WASI core, no filesystem | spec-green |
| `python` | A plain importable module; full WASI p1 incl. the filesystem | spec-green |
| `go` | One self-contained `package main` compiled with `go build`; full WASI p1 | spec-green |
| `java` | One `.java` compiled with `javac`; splits huge modules across classes; full WASI p1 | spec-green |

The authoritative, generated feature/WASI matrix per backend is
[`docs/support.md`](docs/support.md). Bash is the deliberate outlier: no WASI
filesystem, and non-function imports / multiple tables / table bulk ops are
rejected at conversion time.

Later target languages, deliberately out of 0.1 scope: Haskell, OCaml, C#,
and PHP ([ADR-24](docs/adr/24-01-scope-reset.md),
[ADR-10](docs/adr/10-csharp-target.md)).

## Install

Build from source with the pinned Rust toolchain (picked up automatically
from `rust-toolchain.toml`):

```console
$ cargo install --git https://github.com/makenowjust-sandbox/dewasm dewasm-cli
```

or clone and build locally:

```console
$ git clone https://github.com/makenowjust-sandbox/dewasm
$ cd dewasm
$ cargo build --release        # binary at target/release/dewasm
```

dewasm itself has no runtime dependency beyond Rust. *Running* the generated
code needs only that language's own interpreter or toolchain — `ruby`,
`bash` 5+, `python3` 3.9+, `go` 1.18+, or a JDK 11+. Per-target details are in
[docs/backends/](docs/backends/).

## Usage

```console
$ dewasm input.wasm --target <ruby|bash|python|go|java> --mode <standalone|library> -o out
```

- `--target` selects the language (default `ruby`).
- `--mode standalone` wires up WASI and runs the module's `_start`;
  `--mode library` (default) exposes the module's exports to the host
  language.
- `.wat` text input is accepted directly (no separate assembler step).
- `-o -` writes to stdout.

Standalone is the "run this program" shape:

```console
$ dewasm hello.wat --target python --mode standalone -o hello.py
$ python3 hello.py
Hello, WASI!
```

Standalone programs share one runtime interface across every backend, modelled
on wasmtime's CLI — guest arguments after the program, host directories mounted
with repeatable `--dir HOST::GUEST` flags — documented in
[docs/standalone-interface.md](docs/standalone-interface.md).

Library mode hands you the module as a class/type to call from your own code:

```console
$ dewasm add.wat --target ruby --mode library -o add.rb
```

```ruby
require_relative "add"
inst = Add.new                 # pass an imports table if the module needs one
inst.invoke("add", 2, 3)       # => 5
inst.memory                    # linear memory, if any
```

WASI imports work out of the box in library mode: any import the embedder does
not supply falls back to a bundled WASI implementation (built only if actually
used; disable with `--no-default-wasi`). You can also override individual
imports or replace a whole namespace with a *provider* object — see the
[getting-started guide](docs/getting-started.md) and
[ADR-7](docs/adr/7-import-providers.md).

A full walkthrough — standalone and library, with a provider override —
lives in [docs/getting-started.md](docs/getting-started.md).

## What it can convert

Every app below is a pinned, real-world binary; the byte-for-byte comparison
against `wasmtime` is a checked-in test gate ([docs/apps-audit.md](docs/apps-audit.md)).
`examples/apps/fetch.sh` fetches or builds them into a local cache
([ADR-9](docs/adr/9-example-apps-from-registry.md)).

- **cowsay** — the C classic, byte-identical to `wasmtime` on **all five
  backends**.
- **minigzip** (zlib) — a binary-stdio gzip CLI, byte-exact compression on
  **all five backends** (including Bash — it has no floats).
- **QuickJS** — a complete JavaScript engine as one generated file: one-shot
  `-e` eval on all backends, plus file I/O and a scripted REPL against a
  preopened directory on the filesystem-capable backends.
- **SQLite** — the shell (in-memory and on a real DB file) plus a C-API
  library drive and a guest→host callback binding.
- **ripgrep** — a recursive directory search, byte-identical to
  `wasmtime --dir`.
- **CPython** and **CRuby** — both language runtimes convert *and execute* on
  the Ruby backend, each reading its stdlib from a preopened directory.
  Converting a runtime and running it back on Ruby is the **"Ruby on Ruby"**
  north-star, achieved.

Honest caveats: the slow apps (QuickJS, SQLite, CPython, CRuby) are `#[ignore]`d
by default in the test suite — gated behind each backend crate's `slow_test`
cargo feature ([ADR-48](docs/adr/48-slow-test-tiers.md)) — because they are slow and large — CRuby is a ~335 MB Ruby
source file that runs in ~60 s — and Bash skips them by default because its
softfloat makes float-heavy programs slow. These are deliberate performance
opt-outs, not correctness gaps ([ADR-15](docs/adr/15-tests-fail-not-skip.md)).

The north-star demos we steer toward: a library-mode SQLite as a pure-Ruby
`sqlite3` driver (Ruby on Rails on a dewasmified SQLite), and C/Rust tools
running under plain Bash.

## Scope

0.1 targets exactly **wasm core 1.0 + WASI preview 1**, plus the extensions
every C/Rust/Zig toolchain enables by default: mutable globals,
sign-extension operators, saturating float-to-int, multi-value, and bulk
memory (`memory.copy`/`fill`/`init`, passive data segments). Wasm 2.0+
proposals (SIMD, reference types as *constructs*, tail calls, exception
handling) and the component model are deliberately **out of scope** and
rejected at conversion time with a clear error, never a runtime surprise
([ADR-0](docs/adr/0-foundation.md), [ADR-24](docs/adr/24-01-scope-reset.md)).

## How it works

- `crates/dewasm-core` decodes and validates the binary (wasmparser) and
  builds a structured IR: wasm's control flow (block/loop/if/br) is kept
  as-is — no relooper — and the value stack is flattened into per-slot
  variables, wasm2c-style ([ADR-1](docs/adr/1-ir-design.md)).
- `crates/dewasm-backend-*` lower that IR per language. The numeric strategy
  (masked-unsigned integers, f32 re-rounding, NaN bit exactness) is decided
  once and shared ([ADR-2](docs/adr/2-numeric-semantics.md)); per-backend
  lowering shapes have their own ADRs, linked from
  [docs/backends/](docs/backends/).
- `runtime/<lang>/units/` holds the runtime as per-method units with declared
  dependencies; only the units a module actually needs are bundled into the
  output ([ADR-6](docs/adr/6-runtime-units.md)).

## Documentation

- [docs/getting-started.md](docs/getting-started.md) — a hands-on walkthrough.
- [docs/backends/](docs/backends/) — per-target reference (output shape, how
  to run it, requirements, caveats).
- [docs/support.md](docs/support.md) — the generated feature/WASI matrix.
- [docs/apps-audit.md](docs/apps-audit.md) — the real-world app gate record.
- [docs/testing.md](docs/testing.md) — for contributors: what `cargo test`
  needs.
- [docs/adr/](docs/adr/README.md) — the design decisions; start at
  [ADR-0](docs/adr/0-foundation.md).
- [docs/docs-policy.md](docs/docs-policy.md) — what goes where.

## Toward 0.1

The backends and the app gate are in place. Remaining before tagging 0.1:

1. Rename the GitHub repository to `dewasm` ([ADR-26](docs/adr/26-rename-dewasm.md)).
2. Cut and tag the 0.1 release.
