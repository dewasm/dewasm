# Standalone runtime interface

A program converted with `--mode standalone` is a self-contained CLI: the generated `main` supplies the WASI guest its argv, environment, and filesystem preopens, and translates the guest's exit/trap into a process exit code.
That interface is uniform across every backend and modelled on wasmtime's CLI, so a converted program behaves like the `.wasm` it came from.

Because the artifact is self-contained, its **internal** name is not part of any interface and is therefore fixed: the module class is `Program` (Ruby, Python, Perl, Java; Perl as `package Program`, Go as type `Program` in `package main`) and the Bash function prefix is `program_`.
`--module-name` is a library-mode flag and is rejected together with `--mode standalone`.

## Invocation

```
<runner> <program> [--dir HOST::GUEST]... [--] [guest args...]
```

The generated `main` consumes a **leading run of `--dir` flags** and hands everything after them to the guest as `argv[1..]`:

- `--dir HOST::GUEST` (repeatable) preopens host directory `HOST` at guest path `GUEST`, exactly like `wasmtime run --dir`.
  Both `--dir X` and `--dir=X` are accepted.
  A value with no `::` mounts the same path on both sides (`--dir /data` = `--dir /data::/data`).
- `--` ends flag parsing; every following token is a guest argument (use it to pass a guest a literal `--dir`).
- The first token that is not a `--dir` flag also ends parsing; it and the rest are guest arguments.

`<runner>` is the per-backend way to launch the program (below).
`--dir` is a shim parsed *inside the generated program*, not a flag of the interpreter, so it comes after `<program>`, whereas wasmtime consumes its own `--dir` before the `.wasm`.

### Per-backend runner lines

| Backend | Build | Run |
| --- | --- | --- |
| Ruby | — | `ruby prog.rb [--dir H::G]... [args...]` |
| Python | — | `python3 prog.py [--dir H::G]... [args...]` |
| Perl | — | `perl prog.pl [--dir H::G]... [args...]` |
| Bash | — | `bash prog.sh [--dir H::G]... [args...]` |
| Go | `go build -o prog prog.go` | `./prog [--dir H::G]... [args...]` |
| Java | `javac Main.java` | `java Main [--dir H::G]... [args...]` |

## argv, env, exit

| Aspect | Behavior |
| --- | --- |
| `argv[0]` | The program name, the basename of the invoked program file (`prog.rb`, `prog`, `prog.sh`, ...), matching `basename(wasm)` under wasmtime. **Java exception:** the JVM does not pass the launched file name to `main`, so Java uses the module class name, which in standalone mode is the fixed `Program`. |
| `argv[1..]` | The tokens left after `--dir` parsing, in order. |
| env | The whole process environment passes through to the guest. |
| `proc_exit(N)` | Process exits with code `N`. |
| `_start` returns | Process exits `0`. |
| trap | `trap: <message>` on stderr, process exits **134**. |
| `--dir` with no argument | `--dir requires a HOST::GUEST argument` on stderr, process exits **1**. |

## Bash `--dir`

The Bash backend honors `--dir` with real WASI filesystem support.
It reaches the same exit/trap surface as the other backends through its status-cascade protocol (133 = `proc_exit`, 134 = trap).

## Example

```console
$ dewasm examples/wat/wasi_standalone_dir.wat --target ruby --mode standalone -o rt.rb
$ mkdir /tmp/work
$ ruby rt.rb --dir /tmp/work::/
hello, wasi fs!
$ cat /tmp/work/hello.txt
hello, wasi fs!
```

The same command under wasmtime (`wasmtime run --dir /tmp/work::/ wasi_standalone_dir.wat`) produces identical output; the shared `wasi_standalone_dir` e2e case runs it on every filesystem backend and re-runs it under wasmtime as ground truth.
