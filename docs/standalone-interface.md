# Standalone runtime interface

A program converted with `--mode standalone` is a self-contained CLI.
The generated `main` supplies the WASI guest its `argv`, environment, and file system preopens.
It also translates the guest's exit/trap into a process exit code.
That interface is uniform across every backend.
It is modelled on Wasmtime's CLI, so a converted program behaves like the `.wasm` it came from.

The guest runs as soon as the file is loaded.
No `__main__`-style guard stands between loading the artifact and running it.
A standalone artifact is therefore for executing, never for loading from other code.
An artifact to require, import, or source is what `--mode library` produces.

The artifact is self-contained, so its **internal** name is not part of any interface.
That name is therefore fixed:

- the module class is `Program` in Ruby, Python, and Java;
- Perl names it `package Program`;
- Go names it type `Program` in `package main`;
- the Bash function prefix is `program_`.

`--module-name` is a library-mode flag and is rejected together with `--mode standalone`.

## Command line

```
<runner> <program> [--dir HOST::GUEST | --env NAME[=VAL]]... [--] [guest args...]
```

The generated `main` consumes a **leading run of `--dir` and `--env` flags**, in any order.
It hands everything after them to the guest as `argv[1..]`:

- `--dir HOST::GUEST` (repeatable) preopens host directory `HOST` at guest path `GUEST`.
  It behaves like `wasmtime run --dir`.
  Both `--dir X` and `--dir=X` are accepted.
  A value with no `::` mounts the same path on both sides (`--dir /data` = `--dir /data::/data`).
- `--env NAME=VAL` (repeatable) gives the guest the variable `NAME` with the value `VAL`.
  `--env NAME` gives it the value `NAME` has in the process, and nothing when it is unset.
  They behave like `wasmtime run --env`, and `--env=X` is accepted too.
  The guest's environment holds nothing else.
- `--` ends flag parsing; every following token is a guest argument.
  Use it to pass a guest a literal `--dir` or `--env`.
- The first token that is neither flag also ends parsing; it and the rest are guest arguments.

`<runner>` is the per-backend way to launch the program (below).
The flags are a shim parsed *inside the generated program*, not flags of the interpreter.
They therefore come after `<program>`, whereas `wasmtime` consumes its own before the `.wasm`.

### Per-backend runner lines

| Backend | Build | Run |
| --- | --- | --- |
| Ruby | — | `ruby prog.rb [flags...] [args...]` |
| Python | — | `python3 prog.py [flags...] [args...]` |
| Perl | — | `perl prog.pl [flags...] [args...]` |
| Bash | — | `bash prog.sh [flags...] [args...]` |
| Go | `go build -o prog prog.go` | `./prog [flags...] [args...]` |
| Java | `javac Main.java` | `java Main [flags...] [args...]` |

## `argv`, environment, exit

| Aspect | Behavior |
| --- | --- |
| `argv[0]` | The program name: the name of the program file as run, without its directory (`prog.rb`, `prog`, `prog.sh`, ...), matching `basename(wasm)` under Wasmtime. **Java exception:** the JVM does not pass the launched file name to `main`, so Java uses the module class name, which in standalone mode is the fixed `Program`. |
| `argv[1..]` | The tokens left after flag parsing, in order. |
| Environment | Only the variables the `--env` flags name. A name given twice takes its last value, where Wasmtime gives the guest both entries. The order of the variables is not part of the interface. |
| `proc_exit(N)` | Process exits with code `N`. |
| `_start` returns | Process exits `0`. |
| trap | `trap: <message>` on `stderr`, process exits **134**. |
| `--dir` with no argument | `--dir requires a HOST::GUEST argument` on `stderr`, process exits **1**. |
| `--env` with no argument | `--env requires a NAME[=VAL] argument` on `stderr`, process exits **1**. |

## Bash `--dir`

The Bash backend honors `--dir` with real WASI file system support.
It reaches the same exit/trap surface as the other backends through its status chain protocol.
That protocol uses status 133 for `proc_exit` and 134 for a trap.

## Example

```console
$ dewasm examples/wat/wasi_standalone_dir.wat --target ruby --mode standalone -o rt.rb
$ mkdir /tmp/work
$ ruby rt.rb --dir /tmp/work::/
hello, wasi fs!
$ cat /tmp/work/hello.txt
hello, wasi fs!
```

Under Wasmtime, `wasmtime run --dir /tmp/work::/ wasi_standalone_dir.wat` produces identical output.
The shared `wasi_standalone_dir` e2e case runs the program on every file system backend.
It also re-runs the program under Wasmtime as the reference.

`wasi_standalone_env.wat` prints its environment, one variable per line:

```console
$ dewasm examples/wat/wasi_standalone_env.wat --target ruby --mode standalone -o env.rb
$ ruby env.rb
$ ruby env.rb --env GREETING=hello --env HOME
GREETING=hello
HOME=/home/user
```

`wasmtime run --env GREETING=hello --env HOME wasi_standalone_env.wat` prints the same two lines.
