# Recording the sizes

How to run the size record and how to read its numbers.
The results are in [results.md](results.md) with its figures under `figs/`; the record itself is the dated `*-size.json` under [`benchmarks/results/`](../../benchmarks/results/), beside the timing records.

## Running

```console
$ examples/apps/setup.sh             # the cached apps (cowsay, sqlite3-shell, qjs, ruby)
$ cargo xtask size                   # the whole corpus, a few minutes
```

A run writes a dated record to `benchmarks/results/<timestamp>Z-size.json` and regenerates `docs/sizes/results.md` with its figures.
Useful options:

| Command | Effect |
| --- | --- |
| `cargo xtask size --render <record-size.json>` | Regenerate the document and its figures from a stored record without measuring. |

The apps cache is required: an app that is not built is reported as not measured, with the script that would fix it.
A wasm runtime is not — none has to be installed, and every one that is missing is reported with its reason while the run continues.

## How to read the numbers

The question the record answers is what it costs to distribute a wasm program.
Shipping the wasm means shipping the binary *and* a runtime that can execute it; shipping dewasm's output means shipping source to users who already have the interpreter.
Which of the two is smaller is a fact about a given app and a given backend, and the tables are where to look it up: within one app, compare a backend's row against the wasm binary's row plus whichever runtime that delivery would carry.

- **Sizes are raw bytes on disk, never compressed.** What a release artifact weighs is what the person distributing it pays. Compression is also not neutral between the two sides being compared — source compresses far better than a binary — so a compressed column would flatten exactly the differences this record exists to track, and every later size improvement would show up smaller than it is.
- **A converted program's size is every file the backend emits**, converted in standalone mode with the embedded runtime and the default WASI implementation, which is what a standalone artifact ships with. Java emits more than one file; the delivery is all of them.
- **A runtime's size is its resolved executable plus the shared libraries that executable actually names.** Both halves matter. Homebrew's `wasmedge` command-line tool is a thin front end over `libwasmedge`, so the executable alone would understate it by an order of magnitude; the `lib/` directory beside `wasmtime` holds an embedding SDK its statically linked CLI never opens, so counting that directory would overstate it about twofold. What counts as a dependency is what the executable's own bytes mention, which is where the dynamic linker's list lives. The record stores every counted file with its path and size, so the accounting can be checked against the host rather than trusted.

Two things the numbers do not include, stated because they cut in opposite directions:

- Converted source presumes the target language's interpreter is already installed. That is the premise of shipping to that language's users — a Ruby program reaching Ruby users — but it is a presumption, and the interpreter is not free on a host that lacks it.
- A runtime binary is built for one platform; source is portable. A runtime figure is therefore the cost of *one* platform's delivery, and a source figure the cost of all of them.

The record is host-specific and dated: the runtime sizes are whatever that host happens to have installed, at whatever versions.
Two records taken on different hosts compare on their source figures, not on their runtime ones.
