# Codon backend

`--target codon`.
One self-contained Codon source file, compiled ahead of time by the [Codon](https://github.com/exaloop/codon) toolchain.
Codon is a statically typed Python dialect.

## Output shape

A single `.codon` file: a class per artifact plus a bundled runtime class referenced as `<Class>Rt`.
Integers are native `UInt[32]`/`UInt[64]`.
Wrapping arithmetic, unsigned compares and shifts come from the representation itself.
Signed views are `Int[32]`/`Int[64]` casts.
Floats are native `float32`/`float`, with `@llvm` units for the bit paths:

- reinterpretation;
- raw `align 1` memory access;
- `fdiv` without the host's zero-divisor exception.

Control flow uses the branch-register model.
Block bodies are spliced inline, and forward branches set a per-function `_br` register.
Only real loops become `while True:`.

| Mode | Class | Runtime class |
| --- | --- | --- |
| standalone | `Program` | `ProgramRt` |
| library | `--module-name` verbatim | `--module-name` + `Rt` |

A standalone artifact is a program, so its internal names are fixed.
Its bytes never depend on `--module-name`.
A library name must be a single identifier (`[A-Za-z_][A-Za-z0-9_]*`).
Anything else is rejected at conversion time with no sanitization.
The per-artifact runtime class is what isolates one artifact from another.
Two converted libraries coexist in one namespace with nothing shared, including their trap types.

## Requirements

`codon` on `PATH` (or `$DEWASM_CODON`), **0.20 or newer**.
A binary built by `codon build` links Codon's runtime dylibs (`libcodonrt`, `libomp`).
Run it with the toolchain's `lib/codon` directory on `DYLD_LIBRARY_PATH`/`LD_LIBRARY_PATH`.
Or use `codon run`, which needs neither.

## Running it

```console
$ dewasm prog.wasm --target codon --mode standalone -o prog.codon
$ codon run -release prog.codon arg1 arg2
$ # or compile once:
$ codon build -release -o prog prog.codon
```

Standalone programs follow the shared runtime interface: argv, `--dir` preopens, env, exit/trap.
It is described in [docs/standalone-interface.md](../standalone-interface.md).

## Embedding a library artifact

Codon is statically typed, so the dynamic boundary is boxed.
Exports live in `exports` as `Extern` values whose functions take and return lists of boxed `Val`s.

```console
$ dewasm add.wasm --target codon --mode library --module-name Add -o add.codon
```

```python
_i = Add(Dict[str, Dict[str, AddRt.Extern]](), List[str](), Dict[str, str](), Dict[str, str]())
_r = _i.exports["add"].fn.invoke([AddRt.Val.of_i32(UInt[32](2)), AddRt.Val.of_i32(UInt[32](3))])
print(_r[0].i32())  # 5
```

Constructor arguments are `(imports, argv, env, preopens)`.
Imports are a plain `Dict[str, Dict[str, Extern]]`.
Another converted instance's `exports` dict slots in directly as an import source.
Import resolution runs at instantiation.
It checks the kind, a function's structural signature, and a global's value type.
`proc_exit` raises `<Class>Rt.Exit`; catch it if you drive `_start` yourself.

## Capabilities

Full wasm core 1.0 plus the universal baseline.
The baseline is non-function imports, multiple tables, and table bulk ops.
**Full WASI preview 1 including the filesystem**, built on libc via C interop.
The final exception-handling proposal is supported.
A thrown wasm exception is a native exception carrying its tag.
catch_all cannot observe traps.
Tail calls are supported through a typed trampoline.
A parked call carries its arguments in per-slot fields and its target as a prebuilt entry object.
So a chain bounces in one frame with nothing boxed.
Authoritative matrix: [docs/support.md](../support.md).

## Caveats

- **Build cost dominates, superlinearly on huge functions.**
  Microbenchmark-size artifacts compile in seconds.
  A single function of tens of thousands of statements pushes `codon build -release` into minutes.
  Cowsay end to end takes ~9 minutes as a release build and ~70 seconds as a debug build.
  The test suites compile to a content-addressed cache binary to pay each build once.
  They build debug throughout.
- The output is a Codon dialect, not CPython-compatible Python.
  `UInt[N]`, `Ptr[byte]` and `@llvm` blocks do not run under `python3`.
- A standalone program runs the guest on the native stack (8 MB main-thread default).
  That stack carries deep-but-valid recursion like the 5000-frame e2e case unmitigated.
  A runaway recursion is a fatal overflow rather than a catchable trap.
  The spec harness's guarded builds are the exception.
