# Getting started

A hands-on tour of dewasm: convert a wasm module to a real language, run it standalone, then call it as a library and override one of its imports. Every command here is verified against the repository; the `dewasm` binary is built with `cargo build --release` (`target/release/dewasm`) or run in place with `cargo run -p dewasm-cli --`.

## 1. A binary to convert

You can point dewasm at any `wasm32-wasip1` binary, but the repository ships small `.wat` examples so you need no toolchain to follow along. We will use [`examples/wat/hello.wat`](../examples/wat/hello.wat), which writes a line via WASI's `fd_write` and exits:

```wat
(module
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 8) "Hello, WASI!\n")
  (func (export "_start")
    (i32.store (i32.const 0) (i32.const 8))
    (i32.store (i32.const 4) (i32.const 13))
    (drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 20)))
    (call $proc_exit (i32.const 0))))
```

dewasm accepts `.wat` text directly, so no separate assembler step is needed.

## 2. Standalone mode: run the program

`--mode standalone` wires up WASI and runs the module's `_start`. Pick a target with `--target`:

```console
$ dewasm examples/wat/hello.wat --target python --mode standalone -o hello.py
$ python3 hello.py
Hello, WASI!
```

```console
$ dewasm examples/wat/hello.wat --target ruby --mode standalone -o hello.rb
$ ruby hello.rb
Hello, WASI!
```

The Bash backend produces a script you run with `bash` 5+:

```console
$ dewasm examples/wat/hello.wat --target bash --mode standalone -o hello.sh
$ bash hello.sh
Hello, WASI!
```

The compiled backends emit source you build first. Go is one `package main`:

```console
$ dewasm examples/wat/hello.wat --target go --mode standalone -o hello.go
$ go run hello.go
Hello, WASI!
```

Java's standalone entry point is always a `public class Main`, so name the output `Main.java`:

```console
$ dewasm examples/wat/hello.wat --target java --mode standalone -o Main.java
$ javac Main.java && java Main
Hello, WASI!
```

A real binary works the same way. If you have run `examples/apps/setup.sh`, try the cowsay showpiece:

```console
$ dewasm examples/apps/cache/cowsay.wasm --target bash --mode standalone -o cowsay.sh
$ echo "moo" | bash cowsay.sh
 _____
< moo >
 -----
        \   ^__^
         \  (oo)\_______
            (__)\       )\/\
               ||----w |
                ||     ||
```

Standalone programs share one runtime interface across every backend, modelled on wasmtime's CLI: pass the guest arguments after the program, and mount host directories with repeatable `--dir HOST::GUEST` flags (on the filesystem-capable backends). A `proc_exit(N)` becomes exit code `N`, and a trap prints to stderr and exits 134. The full reference — argv, env, exit/trap, and per-backend runner lines — is [docs/standalone-interface.md](standalone-interface.md).

```console
$ dewasm examples/wat/wasi_standalone_dir.wat --target ruby --mode standalone -o rt.rb
$ mkdir /tmp/work
$ ruby rt.rb --dir /tmp/work::/
hello, wasi fs!
```

## 3. Library mode: call the exports

`--mode library` (the default) exposes the module's exports to the host language instead of running `_start`. We will use [`examples/wat/add.wat`](../examples/wat/add.wat), which exports `add` and a recursive `fib`.

`--module-name` names the generated class/package and is required in library mode. It is used exactly as written — a name that does not fit the target language's grammar is a conversion-time error, never a silently reshaped name ([ADR-63](adr/63-module-name-policy.md)).

### Ruby

```console
$ dewasm examples/wat/add.wat --target ruby --mode library --module-name Add -o add.rb
```

```ruby
require_relative "add"

inst = Add.new
puts inst.invoke("add", 2, 3)   # => 5
puts inst.invoke("fib", 10)     # => 55
```

### Python

```console
$ dewasm examples/wat/add.wat --target python --mode library --module-name Add -o add.py
```

```python
from add import Add

inst = Add()
print(inst.invoke("add", 2, 3))   # 5
print(inst.invoke("fib", 10))     # 55
```

### Go

Library output is a Go **package** named after `--module-name`, so put it in a directory of that name and import it. Exports are typed callables in `Exports`:

```console
$ mkdir add
$ dewasm examples/wat/add.wat --target go --mode library --module-name add -o add/add.go
```

```go
// main.go, next to the add/ directory
package main

import (
	"fmt"

	"example.com/myapp/add"
)

func main() {
	inst := add.NewAdd(nil, nil, nil, nil)
	fmt.Println(inst.Exports["add"].(func(uint32, uint32) uint32)(2, 3)) // 5
	fmt.Println(inst.Exports["fib"].(func(uint32) uint32)(10))          // 55
}
```

```console
$ go run .
```

### Java

The generated module class is package-private, so put your `public class Main` in the *same* `.java` file (generate with `--module-name Add`, then append):

```java
public class Main {
    public static void main(String[] args) {
        Add inst = new Add(null, null, null, null);
        System.out.println((int)(Integer)((Rt.Fn) inst.Exports.get("add")).invoke(new Object[]{2, 3})); // 5
        System.out.println((int)(Integer)((Rt.Fn) inst.Exports.get("fib")).invoke(new Object[]{10}));    // 55
    }
}
```

```console
$ dewasm examples/wat/add.wat --target java --mode library --module-name Add -o Main.java
$ javac Main.java && java Main   # after appending the class above
```

The constructor arguments are `(imports, argv, env, preopens)` for the compiled backends (`nil`/`null` for none); Ruby and Python take the imports table as the first positional argument and `preopens:` as a keyword. See [docs/backends/](backends/) for the exact per-language shape.

## 4. Overriding an import (provider)

In library mode any WASI import the embedder does not provide falls back to a bundled WASI implementation. You can intercept individual imports — useful for capturing output, sandboxing, or supplying host functions the module imports.

Convert `hello.wat` as a library and provide our own `fd_write`, letting `proc_exit` fall back to the bundled WASI (which raises `Rt::Exit`):

```console
$ dewasm examples/wat/hello.wat --target ruby --mode library --module-name Hello -o hello_lib.rb
```

```ruby
require_relative "hello_lib"

captured = +""
holder = {}
fd_write = lambda do |_fd, iovs, _iovs_len, out_ptr|
  mem = holder[:inst].memory
  ptr = mem.bytes.unpack1("L<", offset: iovs)
  len = mem.bytes.unpack1("L<", offset: iovs + 4)
  captured << mem.bytes.byteslice(ptr, len)
  mem.bytes[out_ptr, 4] = [len].pack("L<")
  0
end

inst = Hello.new({ "wasi_snapshot_preview1" => { "fd_write" => fd_write } })
holder[:inst] = inst
begin
  inst.invoke("_start")
rescue Hello::Rt::Exit => e   # proc_exit fell back to the bundled WASI
  warn "exited with #{e.code}"
end
print "captured: ", captured   # captured: Hello, WASI!
```

An imports-table value can also be a whole *provider object* (implement `import(name)` and optionally `attach(instance)`) to replace an entire namespace — for example a custom WASI. The mechanism and its fallback rules are [ADR-7](adr/7-import-providers.md); every backend's provider snippet is in [docs/backends/](backends/).

## Where to go next

- [docs/backends/](backends/) — output shape, requirements, and idioms per target language.
- [docs/standalone-interface.md](standalone-interface.md) — the standalone runtime interface (argv, `--dir`, env, exit/trap) shared by every backend.
- [docs/support.md](support.md) — which features and WASI calls each backend supports.
- [README](../README.md#what-it-can-convert) — the real-world apps dewasm converts and how to fetch them.
