![dewasm logo](./assets/dewasm_logo_hex_gradient.png)

`dewasm` translates a WebAssembly binary into the **source code** of an ordinary programming language.

Here is [`cowsay`](https://wasmer.io/syrusakbary/cowsay), a WebAssembly binary, converted to a **pure Bash script** and run with nothing but `bash`:

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

As a larger example, [QuickJS-NG](https://quickjs-ng.github.io/quickjs/), a JavaScript engine written in C, can be converted just as well, this time to **pure Ruby**:

```console
$ dewasm examples/apps/cache/qjs.wasm --target ruby --mode standalone -o qjs.rb
$ ruby qjs.rb -e 'console.log("2**16 =", 2**16); console.log(JSON.stringify(["Ruby", "JavaScript"].sort()))'
2**16 = 65536
["JavaScript","Ruby"]
```

A WebAssembly binary can also be used as a **library** instead of a standalone command.
Here, a small example [`add.wat`](examples/wat/add.wat) is converted and its `add` export is called directly from Ruby:

```console
$ dewasm examples/wat/add.wat --target ruby --mode library -o add.rb
```

```ruby
require_relative "add"

inst = Add.new
inst.invoke("add", 2, 3) # => 5
```

This scales to real libraries too: `dewasm` converts SQLite itself to pure Ruby in [examples/rails](examples/rails), which drives a minimal Rails app with it as the only database engine. And it is not limited to command-line software: [examples/doom](examples/doom) converts DOOM to pure Go and pure Java and plays it in a window — and to pure Ruby, pure Python, and even pure Bash, played right in the terminal via ANSI art — with each language supplying its own rendering and input on top of the same wasm binary.

Finally, a quick summary of `dewasm`'s features:

- `dewasm` implements most of the [Wasm 1.0](https://www.w3.org/TR/wasm-core-1/) feature set and [WASI preview 1](https://github.com/WebAssembly/WASI/tree/wasi-0.1) API surface, so it can convert most real-world WebAssembly binaries.
- `dewasm` is polyglot; it translates one WebAssembly binary to several target languages, such as Ruby, Bash, and Go.
- `dewasm` can output either a standalone script or library source code.
- `dewasm` bundles only the runtime that a WebAssembly binary actually needs; the output source code contains only the functions the binary uses.

## Installation

Currently, `dewasm` is not published to [crates.io](https://crates.io).

```console
$ cargo install --git https://github.com/dewasm/dewasm dewasm-cli
```

or:

```console
$ git clone https://github.com/dewasm/dewasm && cd dewasm
$ cargo build --release
```

## Usage

```console
$ dewasm input.<wasm|wat>
    --target <ruby|bash|python|go|java>
    --mode <standalone|library>
    -o output.<rb|sh|py|go|java>
```

- `input` is a WebAssembly binary to be translated.
  * `dewasm` accepts a WebAssembly text (WAT) file too.
- `--target` (or `-t`) selects the target language (default: `ruby`).
- `--mode` (or `-m`) specifies the translation mode (default: `library`).
  * `--mode standalone` wires up WASI and runs the module's `_start`.
  * `--mode library` exposes the module's exports to the target language.
- `--output` (or `-o`) specifies the output file (default: `-`).
  * When `-` is specified, `dewasm` outputs the result to `stdout`.

Note that `dewasm` has additional command-line options such as `--module-name`.
Please see `dewasm --help` for the full list.

## Copyright

The MIT license.
See the [LICENSE](./LICENSE) file.

Copyright (c) 2026 Hiroya Fujinami
