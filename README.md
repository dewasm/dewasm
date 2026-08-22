![dewasm logo](./assets/dewasm_logo_hex_gradient.png)

`dewasm` converts WebAssembly binaries into **pure source code** for languages like Ruby, Bash, and Go.

No WebAssembly runtime is *required*.
The output runs anywhere plain `ruby`, `bash`, or `go` does.

Here is [`cowsay`](https://wasmer.io/syrusakbary/cowsay), a WebAssembly binary, converted to a **pure Bash script** and run with *nothing but* `bash`:

```console
$ dewasm examples/apps/cache/cowsay.wasm --target bash --mode standalone -o cowsay.sh
$ echo "Hello from Bash" | bash cowsay.sh
 _________________
< Hello from Bash >
 -----------------
        \   ^__^
         \  (oo)\_______
            (__)\       )\/\
               ||----w |
                ||     ||
```

Even, [QuickJS-NG](https://quickjs-ng.github.io/quickjs/), a JavaScript engine written in C, can be converted just as well, this time to **pure Ruby**:

```console
$ dewasm examples/apps/cache/qjs.wasm --target ruby --mode standalone -o qjs.rb
$ ruby qjs.rb -e 'console.log("2**16 =", 2**16); console.log(JSON.stringify(["Ruby", "JavaScript"].sort()))'
2**16 = 65536
["JavaScript","Ruby"]
```

A WebAssembly binary can also be used as a library instead of a standalone application.
Here, a small example [`add.wat`](examples/wat/add.wat) is converted and its `add` export is called directly from Ruby:

```console
$ dewasm examples/wat/add.wat --target ruby --mode library --module-name Add -o add.rb
```

```ruby
require_relative "add"

inst = Add.new
inst.invoke("add", 2, 3) # => 5
```

Beyond simple examples, `dewasm` scales to *real libraries and applications* too:

- [examples/rails](examples/rails) demonstrates that **[SQLite](https://sqlite.org)**, converted to pure Ruby by `dewasm`, can be used as the database engine for a Rails app.
- [examples/doom](examples/doom) shows how `dewasm` can port the WebAssembly version of **[DOOM](https://github.com/jacobenget/doom.wasm)** to multiple programming languages, *including Bash*.
- [examples/nes](examples/nes) converts a **NES emulator** ([agnes](https://github.com/kgabis/agnes)) once and turns every backend language into a native NES player.

Here is a quick summary of what `dewasm` can do:

- **Support real-world binaries**: Implements most of the [Wasm 1.0](https://www.w3.org/TR/wasm-core-1/) and [WASI preview 1](https://github.com/WebAssembly/WASI/tree/wasi-0.1) specs, plus the [exception-handling proposal](https://github.com/WebAssembly/exception-handling) on most backends.
- **Target multiple languages**: Translates one WebAssembly binary to several target languages, such as Ruby, Bash, and Go.
- **Adapt to your needs**: Generates either standalone scripts or importable library source code.
- **Keep it minimal**: Bundles only the specific runtime code that the WebAssembly binary actually requires.

## Installation

```console
$ cargo install dewasm
```

or:

```console
$ git clone https://github.com/dewasm/dewasm && cd dewasm
$ cargo build --release
```

## Usage

```console
$ dewasm input.<wasm|wat>
    --target <ruby|bash|python|perl|go|java>
    --mode <standalone|library>
    -o output.<rb|sh|py|pl|go|java>
```

- `input` is a WebAssembly binary to be translated.
  * `dewasm` accepts a WebAssembly text (WAT) file too.
- `--target` (or `-t`) selects the target language (required).
- `--mode` (or `-m`) specifies the translation mode (default: `library`).
  * `--mode standalone` wires up WASI and runs the module's `_start`.
  * `--mode library` exposes the module's exports to the target language.
- `--output` (or `-o`) sets the output file (default: `-`).
  * When `-` is specified, `dewasm` outputs the result to `stdout`.
- `--module-name` names the generated class/package and is required in library mode (standalone output has a fixed internal name and rejects it).

Please see `dewasm --help` for the full list of options.

Next: [getting started](docs/getting-started.md), the [per-target reference](docs/backends/), [what each backend supports](docs/support.md), and [projects built on dewasm](docs/users.md).

## Copyright

The MIT license.
See the [LICENSE](./LICENSE) file.

Copyright (c) 2026 Hiroya Fujinami
