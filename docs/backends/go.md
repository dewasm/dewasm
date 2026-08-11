# Go backend

`--target go`.
One self-contained Go source file, compiled with the Go toolchain.

## Output shape

A single `.go` file: a package clause plus a bundled runtime referenced as `Rt.<name>` (methods on a zero-size receiver).
Integers are native `uint32`/`uint64` (wrapping arithmetic makes masking free) and floats are native `float32`/`float64`, so f32 re-rounding and trap-free division need no helper.
Control flow maps onto Go's labeled loops.
Because unused labels and locals are Go compile errors, the backend emits them only when referenced.

The package clause follows the mode:

| Mode | Package | Type | Constructor |
| --- | --- | --- | --- |
| standalone | `main` | `Program` | `NewProgram` |
| library | `--module-name` lowercased | `--module-name` capitalized | `New` + that type |

A standalone artifact is a program, so its internal names are fixed and its bytes never depend on `--module-name`.
A library artifact is a Go *package* someone imports, so the name has to be a Go identifier: `/\A[A-Za-z_][A-Za-z0-9_]*\z/`, ASCII.
A name outside that grammar is rejected at conversion time, with no sanitization: `--module-name rg` gives `package rg` and type `Rg`, `--module-name my-lib` gives an error, not `Mylib`.

The package is also what isolates one artifact from another: each carries its own runtime, so two converted libraries import side by side with nothing shared between them, including their trap types, which are per-package and therefore distinguishable.

## Requirements

`go` on `PATH` (or `$DEWASM_GO`), **1.18 or newer** (the runtime uses generics).
Standalone output is a normal Go program: `go run` or `go build` it.
Library output is a package to import (see below).

## Running it

```console
$ dewasm prog.wasm --target go --mode standalone -o prog.go
$ go build -o prog prog.go && ./prog --dir ./data::/data arg1 arg2
```

Standalone programs follow the shared runtime interface (argv, `--dir` preopens, env, exit/trap): [docs/standalone-interface.md](../standalone-interface.md).

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

Constructor arguments are `(imports, argv, env, preopens)`; exports are typed callables in `Exports`.
`proc_exit` panics with `*rtExit`; recover it if you drive `_start` yourself.

Only exported identifiers cross the package boundary: the instance type, its `Exports`, `Imports`, and the constructor.
Reaching further (the linear memory, an exported global) means writing host code *inside* the package, so add another `.go` file next to the generated one, declaring the same package.
`examples/doom/go` is that shape: the generated `doom/doom_gen.go`, a hand-written `doom/host.go`, and a `main.go` importing the package.
Such a file cannot be *appended* to the generated file itself, because Go requires every `import` to precede all other declarations.

## Capabilities

Full wasm core 1.0 plus the universal baseline, and **full WASI preview 1 including the filesystem**.
Non-function imports, multiple tables, and table bulk ops are supported.
Authoritative matrix: [docs/support.md](../support.md).

## Providers and library usage

Any unprovided WASI import falls back to a bundled WASI, which is built the first time an import actually falls back to it: cover every WASI import and none is ever constructed.
Override imports by passing an `Imports` map (`map[module]source`) to the constructor; preopen directories via the fourth constructor argument.
A source is either a `map[string]any` of name → value, or an object implementing `ImportProvider` (`WasmImport(name string) any`) that resolves names itself, optionally also `ImportAttacher` (`Attach(instance any)`), which the constructor calls once the instance is fully built so the provider can reach its memory.
The e2e override and custom-provider glues in `crates/dewasm-backend-go/tests/e2e.rs` are written from inside the artifact's package, so unqualified; from another package, prefix the constructor with the package name:

```go
inst = NewProg(Imports{"wasi_snapshot_preview1": map[string]any{"fd_write": fdWrite}}, nil, nil, nil)
inst.Exports["_start"].(func())()   // random_get falls back to the bundled WASI

// or one object standing in for the whole module:
inst = NewProg(Imports{"wasi_snapshot_preview1": &myWasi{}}, nil, nil, nil)
```

## Caveats

- **Build cost dominates.**
  Being compiled, the first `go build`/`go run` of a large generated file is the slow step: for a large binary the compile costs more than the run.
  The e2e suite compiles to a content-addressed cache binary to pay this once.
- Native floats mean IEEE semantics come for free, but Go's strict no-FMA contraction is what keeps f32/f64 bit-exact.
