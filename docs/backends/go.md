# Go backend

`--target go`.
One self-contained Go source file, compiled with the Go toolchain.

## Output shape

A single `.go` file: a package clause plus a bundled runtime referenced as `Rt.<name>`.
The runtime functions are methods on a zero-size receiver.
Integers are native `uint32`/`uint64`, whose wrapping arithmetic makes masking free.
Floats are native `float32`/`float64`, so f32 re-rounding and trap-free division need no helper.
Control flow maps onto Go's labeled loops.
Because unused labels and locals are Go compile errors, the backend emits them only when referenced.

The package clause follows the mode:

| Mode | Package | Type | Constructor |
| --- | --- | --- | --- |
| standalone | `main` | `Program` | `NewProgram` |
| library | `--module-name` in lower case | `--module-name` with a capital first letter | `New` + that type |

A standalone artifact is a program, so its internal names are fixed.
Its bytes never depend on `--module-name`.
A library artifact is a Go *package* someone imports, so the name has to be a Go identifier.
The grammar is `/\A[A-Za-z_][A-Za-z0-9_]*\z/`, ASCII.
A name outside that grammar is rejected at conversion time, not rewritten.
`--module-name rg` gives `package rg` and type `Rg`.
`--module-name my-lib` gives an error, not `Mylib`.

The package is also what isolates one artifact from another.
Each carries its own runtime, so two converted libraries import side by side with nothing shared.
That includes their trap types, which are per-package and therefore distinguishable.

## Requirements

`go` on `PATH`, **1.20 or newer** (the runtime uses generics and `unsafe.SliceData`).
A little-endian target, which is every `GOARCH` except `mips`, `mips64`, `ppc64` and `s390x`.
The linear-memory accessors read memory in host byte order.
A big-endian build fails at compile time rather than computing wrong values.
Standalone output is a normal Go program: `go run` or `go build` it.
Library output is a package to import (see below).

## Running it

```console
$ dewasm prog.wasm --target go --mode standalone -o prog.go
$ go build -o prog prog.go && ./prog --dir ./data::/data arg1 arg2
```

Standalone programs share one runtime interface: `argv`, `--dir` preopens, environment, exit/trap.
It is described in [`docs/standalone-interface.md`](../standalone-interface.md).

## Embedding a library artifact

The artifact is a package.
Put it in a directory named after it and import it like any other:

```console
$ mkdir add
$ dewasm add.wasm --target go --mode library --module-name add -o add/add.go
```

```go
package main

import (
	"fmt"

	"example.com/myapp/add"
)

func main() {
	inst := add.NewAdd(nil, nil, nil, nil)
	fmt.Println(inst.Exports["add"].(func(uint32, uint32) uint32)(2, 3)) // 5
}
```

Constructor arguments are `(imports, argv, env, preopens)`.
Exports are typed callables in `Exports`.
`proc_exit` panics with `*rtExit`; recover it if you drive `_start` yourself.

Only exported identifiers cross the package boundary.
Those are the instance type, its `Exports`, `Imports`, and the constructor.
Reaching anything else means writing host code *inside* the package.
Examples are the linear memory and an exported global.
So add another `.go` file next to the generated one, declaring the same package.
`examples/doom/go` is that shape:

- the generated `doom/doom_gen.go`;
- a hand-written `doom/host.go`;
- a `main.go` importing the package.

Go requires every `import` to precede all other declarations.
So such a file cannot be *added to the end* of the generated file itself.

## Capabilities

Full wasm core 1.0 plus the universal baseline, and **full WASI Preview 1 including the file system**.
Non-function imports, multiple tables, and table bulk operations are supported.
The final exception-handling proposal is supported.
A thrown wasm exception is a native exception carrying its tag.
`catch_all` cannot observe traps.
C programs based on `setjmp`/`longjmp` convert and run; `mruby` is the covered app case.
The official support table: [`docs/support.md`](../support.md).

## Providers and library usage

Any unprovided WASI import falls back to a bundled WASI.
The bundled WASI is built the first time an import falls back to it.
Cover every WASI import and none is ever constructed.
Override imports by passing an `Imports` map (`map[module]source`) to the constructor.
Preopen directories via the fourth constructor argument.
A source is one of two things:

- a `map[string]any` of name → value;
- an object implementing `ImportProvider` (`WasmImport(name string) any`) that resolves names itself.

Such an object may also implement `ImportAttacher` (`Attach(instance any)`).
The constructor calls `Attach` once the instance is built.
The provider can then reach its memory.
The e2e override and custom-provider glues are in `crates/dewasm-backend-go/tests/e2e.rs`.
They are written from inside the artifact's package, so they are unqualified.
From another package, prefix the constructor with the package name:

```go
inst = NewProg(Imports{"wasi_snapshot_preview1": map[string]any{"fd_write": fdWrite}}, nil, nil, nil)
inst.Exports["_start"].(func())()   // random_get falls back to the bundled WASI

// or one object standing in for the whole module:
inst = NewProg(Imports{"wasi_snapshot_preview1": &myWasi{}}, nil, nil, nil)
```

## Limits

- **Build cost dominates.**
  Being compiled, the first `go build`/`go run` of a large generated file is the slow step.
  For a large binary the compile costs more than the run.
  The e2e suite compiles to a content-addressed cache binary to pay this once.
- Native floats mean IEEE semantics come for free.
  But Go's strict rule against FMA contraction is what keeps f32/f64 bit-exact.
