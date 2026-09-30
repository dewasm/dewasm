# Getting started

A hands-on tour of dewasm: convert a wasm module to a real language and run it standalone.
Then call it as a library and override one of its imports.
Every command here is verified against the repository.
The `dewasm` binary is built with `cargo build --release` (`target/release/dewasm`).
You can also run it in place with `cargo run -p dewasm --`.

## 1. A binary to convert

You can point dewasm at any `wasm32-wasip1` binary.
But the repository ships small `.wat` examples, so you need no toolchain to follow along.
We will use [`examples/wat/hello.wat`](../examples/wat/hello.wat), which writes a line via WASI's `fd_write` and exits:

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

dewasm accepts `.wat` text directly, so it needs no separate step to convert the text to binary.

## 2. Standalone mode: run the program

`--mode standalone` connects the module to WASI and runs its `_start`.
Pick a target with `--target`:

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

The compiled backends emit source you build first.
Go is one `package main`:

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

Codon compiles a statically typed Python dialect ahead of time.
`codon run` builds and runs in one step:

```console
$ dewasm examples/wat/hello.wat --target codon --mode standalone -o hello.codon
$ codon run -release hello.codon
Hello, WASI!
```

A real binary works the same way.
If you have run `examples/apps/setup.sh`, try the `cowsay` app:

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

Standalone programs share one runtime interface across every backend, modelled on Wasmtime's CLI.
Pass the guest arguments after the program.
Mount host directories with repeatable `--dir HOST::GUEST` flags.
A `proc_exit(N)` becomes exit code `N`, and a trap prints to standard error and exits 134.
The full reference is [`docs/standalone-interface.md`](standalone-interface.md).
It covers `argv`, environment, exit/trap, and per-backend runner lines.

```console
$ dewasm examples/wat/wasi_standalone_dir.wat --target ruby --mode standalone -o rt.rb
$ mkdir /tmp/work
$ ruby rt.rb --dir /tmp/work::/
hello, wasi fs!
```

## 3. Library mode: call the exports

`--mode library` (the default) exposes the module's exports to the host language.
It does not run `_start`.
We will use [`examples/wat/add.wat`](../examples/wat/add.wat), which exports `add` and a recursive `fib`.

`--module-name` names the generated class/package and is required in library mode.
It is used exactly as written.
A name that does not fit the target language's grammar is a conversion-time error.
It is never silently reshaped.

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

Library output is a Go **package** named after `--module-name`.
So put it in a directory of that name and import it.
Exports are typed callables in `Exports`:

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

The generated module class is package-private.
It carries the runtime as `static` nested classes (hence `Add.Rt.Fn`).
So put your `public class Main` in the *same* `.java` file.
Generate it with `--module-name Add`, then add the class below at the end of the file:

```java
public class Main {
    public static void main(String[] args) {
        Add inst = new Add(null, null, null, null);
        System.out.println((int)(Integer)((Add.Rt.Fn) inst.Exports.get("add")).invoke(new Object[]{2, 3})); // 5
        System.out.println((int)(Integer)((Add.Rt.Fn) inst.Exports.get("fib")).invoke(new Object[]{10}));    // 55
    }
}
```

```console
$ dewasm examples/wat/add.wat --target java --mode library --module-name Add -o Main.java
$ javac Main.java && java Main   # after appending the class above
```

The compiled backends take the constructor arguments `(imports, argv, env, preopens)` positionally.
Pass `nil`/`null` for none.
Ruby, Python, and Perl take the imports table as the first positional argument and the rest by name.
So preopens are `preopens:` in Ruby, `preopens=` in Python, and `preopens =>` in Perl.
See [`docs/backends/`](backends/) for the exact per-language shape.

## 4. Overriding an import (provider)

In library mode, any WASI import the embedder does not provide falls back to the bundled WASI.
You can replace individual imports to capture output or sandbox the module.
You can also supply host functions it imports.

Convert `hello.wat` as a library and provide our own `fd_write`.
Let `proc_exit` fall back to the bundled WASI, which raises `Rt::Exit`:

```console
$ dewasm examples/wat/hello.wat --target ruby --mode library --module-name Hello -o hello_lib.rb
```

```ruby
require_relative "hello_lib"

captured = +"".b
holder = {}
fd_write = lambda do |_fd, iovs, _iovs_len, out_ptr|
  mem = holder[:inst].memory
  ptr = mem.iwl(iovs)       # iwl = i32 load, iws = i32 store
  len = mem.iwl(iovs + 4)
  captured << mem.read_string(ptr, len)
  mem.iws(out_ptr, len)
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

An imports-table value can also be a whole *provider object* that replaces an entire namespace.
A custom WASI is one example.
The object implements `import(name)` and optionally `attach(instance)`.
Any WASI import the table leaves unresolved falls back to the bundled WASI.
Every backend's provider example is in [`docs/backends/`](backends/).

## Where to go next

- [`docs/backends/`](backends/): output shape, requirements, and conventions per target language.
- [`docs/standalone-interface.md`](standalone-interface.md): the standalone runtime interface shared by every backend.
  It covers `argv`, `--dir`, environment, and exit/trap.
- [`docs/support.md`](support.md): which features and WASI calls each backend supports.
- [README](../README.md): what dewasm is, plus the real-world examples it converts.
  Those are Rails on converted SQLite, DOOM, and NES.
