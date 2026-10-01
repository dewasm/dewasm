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
- `fdiv` without the host's exception for division by zero.

Control flow uses the branch-register model.
Block bodies are spliced inline, and forward branches set a per-function `_br` register.
Only real loops become `while True:`.

| Mode | Class | Runtime class |
| --- | --- | --- |
| standalone | `Program` | `ProgramRt` |
| library | `--module-name` unchanged | `--module-name` + `Rt` |

A standalone artifact is a program, so its internal names are fixed.
So its bytes never depend on `--module-name`.
A library name must be a single identifier (`[A-Za-z_][A-Za-z0-9_]*`).
Anything else is rejected at conversion time, not rewritten.
The per-artifact runtime class is what isolates one artifact from another.
Two converted libraries load into one namespace and share nothing, including their trap types.

## Requirements

`codon` on `PATH` (or `$DEWASM_CODON`), **0.20 or newer**.
A binary built by `codon build` links Codon's runtime dynamic libraries (`libcodonrt`, `libomp`).
Run it with the toolchain's `lib/codon` directory on `DYLD_LIBRARY_PATH`/`LD_LIBRARY_PATH`.
Or use `codon run`, which needs neither.

## Running it

```console
$ dewasm prog.wasm --target codon --mode standalone -o prog.codon
$ codon run -release prog.codon arg1 arg2
$ # or compile once:
$ codon build -release -o prog prog.codon
```

Standalone programs share one runtime interface: `argv`, `--dir` preopens, environment, exit/trap.
It is described in [`docs/standalone-interface.md`](../standalone-interface.md).

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
Another converted instance's `exports` dictionary slots in directly as an import source.
Import resolution runs at instantiation.
It checks the kind, a function's structural signature, and a global's value type.
`proc_exit` raises `<Class>Rt.Exit`; catch it if you drive `_start` yourself.

## Capabilities

Full wasm core 1.0 plus the universal baseline.
The baseline is non-function imports, multiple tables, and table bulk operations.
**Full WASI Preview 1 including the file system**, built on `libc` by calling C from Codon.
The final exception-handling proposal is supported.
A thrown wasm exception is a native exception carrying its tag.
`catch_all` cannot observe traps.
Tail calls are supported through a typed trampoline.
A pending call carries its arguments in per-slot fields.
Its target is a prebuilt entry object.
So a chain of tail calls runs in one frame with nothing boxed.
The official support table: [`docs/support.md`](../support.md).

## Limits

- **Build cost dominates, and grows faster than linearly on huge functions.**
  Microbenchmark-size artifacts compile in seconds.
  A single function of tens of thousands of statements pushes `codon build -release` into minutes.
  `cowsay` end to end takes ~9 minutes as a release build and ~70 seconds as a debug build.
  The test suites compile to a content-addressed cache binary to pay each build once.
  They build debug throughout.
- The output is a Codon dialect, not CPython-compatible Python.
  `UInt[N]`, `Ptr[byte]` and `@llvm` blocks do not run under `python3`.
- A standalone program runs the guest on the native stack (8 MB main-thread default).
  That stack alone carries deep but valid recursion, like the 5000-frame e2e case.
  An unbounded recursion ends the process with a stack overflow, not a catchable trap.
  The specification harness's guarded builds are the exception.
