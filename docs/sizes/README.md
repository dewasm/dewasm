# Recording the sizes

How to run the size record and how to read its numbers.
The results are in [results.md](results.md) with its figures under `figs/`.
The record itself is the dated `*-size.json` under [`records/`](../../records/README.md), beside the timing records.

## Running

```console
$ examples/apps/setup.sh             # the cached apps (cowsay, sqlite3-shell, qjs, ruby)
$ cargo xtask record-size            # the whole corpus, a few minutes
$ cargo xtask render-size            # results.md and its figures, from that record
```

Measuring and rendering are two commands, as on the speed side.
A run writes a dated `<timestamp>Z-size.json` to [`records/`](../../records/README.md) and nothing else.
Rendering turns a record into `docs/sizes/results.md` with its figures.
`render-size` takes the newest size record by default.
Useful options:

| Command | Effect |
| --- | --- |
| `cargo xtask render-size <record>` | Render an older record instead of the newest one; a `-speed.json` path is refused. |

The apps cache is required.
An app that is not built is reported as not measured, with the script that would fix it.
A wasm runtime is not: none has to be installed.
Every missing runtime is reported with its reason, and the run continues.

## How to read the numbers

The question the record answers is what it costs to distribute a wasm program.
Shipping the wasm means shipping the binary *and* a runtime that can execute it.
Shipping dewasm's output means shipping source to users who already have the interpreter.
Which of the two is smaller is a fact about a given app and a given backend.
The tables are where to look it up.
Within one app, compare a backend's row against the sum of two rows.
Those are the wasm binary's row and the row of whichever runtime that delivery would carry.

Three rules fix what a number covers.

- **Sizes are raw bytes on disk, never compressed.**
  Compression is not neutral between the two sides being compared, so it is left out.
- **A converted program's size is every file the backend emits.**
  The program is converted in standalone mode with the embedded runtime.
  It uses the default WASI implementation.
- **A runtime's size is its resolved executable plus the shared libraries that executable itself names.**
  The record stores every counted file with its path and size.
  So the accounting can be checked against the host rather than trusted.

Two things the record leaves out favor opposite sides:

- converted source presumes the target language's interpreter is already installed;
- a runtime binary is the cost of one platform's delivery, where source covers all of them.

The record is host-specific and dated.
The runtime sizes are whatever that host happens to have installed, at whatever versions.
Two records taken on different hosts compare on their source figures, not on their runtime ones.
