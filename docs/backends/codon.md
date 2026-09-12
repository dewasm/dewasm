# Codon backend

`--target codon`.
One self-contained Codon source file (a statically typed Python dialect), compiled ahead of time by the [Codon](https://github.com/exaloop/codon) toolchain.

## Output shape

A single `.codon` file: a class per artifact plus a bundled runtime class referenced as `<Class>Rt`.
Integers are native `UInt[32]`/`UInt[64]` (wrapping arithmetic, unsigned compares and shifts come from the representation itself); signed views are `Int[32]`/`Int[64]` casts.
Floats are native `float32`/`float`, with `@llvm` units for the bit paths (reinterpretation, raw `align 1` memory access, `fdiv` without the host's zero-divisor exception).
Control flow uses the branch-register model: block bodies are spliced inline, forward branches set a per-function `_br` register, and only real loops become `while True:`.

| Mode | Class | Runtime class |
| --- | --- | --- |
| standalone | `Program` | `ProgramRt` |
| library | `--module-name` verbatim | `--module-name` + `Rt` |

A standalone artifact is a program, so its internal names are fixed and its bytes never depend on `--module-name`.
A library name must be a single identifier (`[A-Za-z_][A-Za-z0-9_]*`); anything else is rejected at conversion time with no sanitization.
The per-artifact runtime class is what isolates one artifact from another: two converted libraries coexist in one namespace with nothing shared, including their trap types.

## Requirements

`codon` on `PATH` (or `$DEWASM_CODON`), **0.20 or newer**.
A binary built by `codon build` links Codon's runtime dylibs (`libcodonrt`, `libomp`); run it with the toolchain's `lib/codon` directory on the loader path (`DYLD_LIBRARY_PATH`/`LD_LIBRARY_PATH`), or use `codon run`, which needs neither.

## Running it

```console
$ dewasm prog.wasm --target codon --mode standalone -o prog.codon
$ codon run -release prog.codon arg1 arg2
$ # or compile once:
$ codon build -release -o prog prog.codon
```

Standalone programs follow the shared runtime interface (argv, `--dir` preopens, env, exit/trap): [docs/standalone-interface.md](../standalone-interface.md).

## Embedding a library artifact

Codon is statically typed, so the dynamic boundary is boxed: exports live in `exports` as `Extern` values whose functions take and return lists of boxed `Val`s.

```console
$ dewasm add.wasm --target codon --mode library --module-name Add -o add.codon
```

```python
_i = Add(Dict[str, Dict[str, AddRt.Extern]](), List[str](), Dict[str, str](), Dict[str, str]())
_r = _i.exports["add"].fn.invoke([AddRt.Val.of_i32(UInt[32](2)), AddRt.Val.of_i32(UInt[32](3))])
print(_r[0].i32())  # 5
```

Constructor arguments are `(imports, argv, env, preopens)`.
Imports are a plain `Dict[str, Dict[str, Extern]]`; another converted instance's `exports` dict slots in directly as an import source.
Import resolution checks the kind, a function's structural signature, and a global's value type at instantiation.
`proc_exit` raises `<Class>Rt.Exit`; catch it if you drive `_start` yourself.

## Capabilities

Full wasm core 1.0 plus the universal baseline (non-function imports, multiple tables, table bulk ops).
The exception-handling and tail-call proposals are accepted input but rejected at conversion time until their lowerings land.
WASI preview 1 currently covers the argv/env, stdout/stderr `fd_write`, `proc_exit`, `random_get` and `sched_yield` surface; the filesystem follows separately.
Authoritative matrix: [docs/support.md](../support.md).

## Caveats

- **Build cost dominates, superlinearly on huge functions.**
  Microbenchmark-size artifacts compile in seconds; a single function of tens of thousands of statements pushes `codon build -release` into minutes.
  The test suites compile to a content-addressed cache binary to pay each build once.
- The output is a Codon dialect, not CPython-compatible Python: `UInt[N]`, `Ptr[byte]` and `@llvm` blocks do not run under `python3`.
- A standalone program runs the guest on the native stack with no depth mitigation yet, so extremely deep guest recursion can overflow it.
