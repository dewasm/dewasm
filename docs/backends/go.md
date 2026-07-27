# Go backend

`--target go`. One self-contained Go source file, compiled with the Go
toolchain.

## Output shape

A single `.go` file: `package main` plus a bundled runtime referenced as
`Rt.<name>` (methods on a zero-size receiver). Integers are native
`uint32`/`uint64` (wrapping arithmetic makes masking free) and floats are
native `float32`/`float64`, so f32 re-rounding and trap-free division need no
helper. Control flow maps onto Go's labeled loops. Because unused labels and
locals are Go compile errors, the backend emits them only when referenced. See
[ADR-29](../adr/29-go-backend-lowering.md).

## Requirements

`go` on `PATH` (or `$DEWASM_GO`), **1.18 or newer** (the runtime uses
generics). The output is a normal Go program — `go run` or `go build` it.

## Running it

```console
$ dewasm prog.wasm --target go --mode standalone -o prog.go
$ go build -o prog prog.go && ./prog --dir ./data::/data arg1 arg2
```

Standalone programs follow the shared runtime interface (argv, `--dir` preopens,
env, exit/trap): [docs/standalone-interface.md](../standalone-interface.md).

Library mode: the generated file is `package main`, so add your own `func main`
in the same package (the file already imports `fmt`). Constructor arguments are
`(imports, argv, env, preopens)`; exports are typed callables in `Exports`:

```go
func main() {
	inst := NewAdd(nil, nil, nil, nil)
	fmt.Println(inst.Exports["add"].(func(uint32, uint32) uint32)(2, 3)) // 5
}
```

`proc_exit` panics with `*rtExit`; recover it if you drive `_start` yourself.

## Capabilities

Full wasm core 1.0 plus the universal baseline, and **full WASI preview 1
including the filesystem**. Non-function imports, multiple tables, and table
bulk ops are supported. Authoritative matrix: [docs/support.md](../support.md).

## Providers and library usage

Any unprovided WASI import falls back to a bundled WASI. Override imports by
passing an `Imports` map to the constructor (`map[module]map[name]func`);
preopen directories via the fourth constructor argument. The e2e override glue
in `crates/dewasm-backend-go/tests/e2e.rs`:

```go
inst = NewProg(Imports{"wasi_snapshot_preview1": {"fd_write": fdWrite}}, nil, nil, nil)
inst.Exports["_start"].(func())()   // random_get falls back to the bundled WASI
```

## Caveats

- **Build cost dominates.** Being compiled, the first `go build`/`go run` of a
  large generated file is the slow step (ripgrep's ~22 MB wasm is dominated by
  the compile, not the run). The e2e suite compiles to a content-addressed
  cache binary to pay this once.
- Native floats mean IEEE semantics come for free, but Go's strict no-FMA
  contraction is what keeps f32/f64 bit-exact ([ADR-29](../adr/29-go-backend-lowering.md)).
