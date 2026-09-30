# Decision 37: Data Segments Moved to a Separate File on Request (`--data-file`)

Status: **Accepted, 2026-07-28.**
Implemented for the Ruby, Go, Python and Java backends behind the CLI's `--data-file` flag.
Bash rejects it loudly at the CLI: its data lives in the runtime, not a standalone literal.
Default (no-flag) output is byte-identical to before.
Extended 2026-07-28 with the Python and Java backends and the Java code-source load strategy.

## Context

A wasm module's data segments are lowered into the generated source as inline literals:

| Backend | Literal | Source |
| --- | --- | --- |
| Ruby | `["<hex>"].pack("H*")` | `crates/dewasm-backend-ruby/src/lib.rs`, `hex_bytes` |
| Go | `Rt.unhex("<hex>")` | `crates/dewasm-backend-go/src/lib.rs`, `hex_bytes` |

Hexadecimal text costs two source bytes per data byte.
The bytes then live inside the parsed program forever.

For data-heavy modules this grows the source.
wasm2go's `-embed` flag is the prior art.
It moves the segment bytes into a binary data file the program loads at start.
Only offset/length constants stay in the code.
The open question was whether to do this always, and how to shape the data file.

## Decision

Add an **optional** `--data-file <path>` CLI flag.
Decision 0's "unsupported fails at conversion time" applies to the rejections below.
When set, the backend:

- joins every segment's bytes into one block, returned as a second `OutputFile`.
  Its `name` is the data-file name.
  The CLI routes it to `--data-file`'s path and the primary source to `-o`.
- writes the per-segment `(blob_offset, len)` prefix sums into the generated code.
  Active segments fill a memory from a slice of the block.
  Passive segments keep an addressable slice view for `memory.init`/`data.drop`.
- references the data file **relative to the program itself**, per backend:

| Backend | Load | Slice |
| --- | --- | --- |
| Ruby | `File.binread(File.join(__dir__, "<name>"))` read once into a frozen `DATA_BLOB` constant | `DATA_BLOB.byteslice(o, len)` (kept ASCII-8BIT, as `binread` returns) |
| Go | a package-scope `//go:embed <name>` / `var dataBlob []byte` | `dataBlob[o:o+len]` |
| Python | a module-level `DATA_BLOB = open(os.path.join(os.path.dirname(__file__), "<name>"), "rb").read()` | `DATA_BLOB[o:o+len]` |
| Java | a `static final byte[] DATA_BLOB` loaded by a static method (see the code-source rule below) | `java.util.Arrays.copyOfRange(DATA_BLOB, o, o+len)`, a fresh array per segment, so `data.drop` can still empty the field |

**Java load strategy: code-source-relative, not a working-directory guess.**
Java has no `__file__`/`__dir__`.
A compiled program may run from a class directory or a packaged JAR file.
The generated loader resolves the data file against the class's own code source.
That is `getProtectionDomain().getCodeSource().getLocation()`:

- a regular file (the JAR file) contributes its *parent* directory;
- a directory (the class directory) is used directly.

The data-file name is resolved there with `Files.readAllBytes`.
Any failure is rethrown as an unchecked `RuntimeException`.
The deciding criterion is locating the data file by *where the program's own code lives*.
Both ways of shipping the program answer that.
The process CWD is not used, since neither shape reliably answers it.

`OutputFile.contents` widened from `String` to `Vec<u8>`.
So a backend can return raw binary alongside UTF-8 source.

**Deciding rule:** a separate data file is a size/layout trade-off with no semantic content.
So it is a *flag* set on each conversion, never a default and never a backend capability flip.
The generated program's behaviour is identical either way.
The specification harness, which never sets the flag, still binds (decision 3).

**Scope:** Ruby, Go, Python and Java.
Bash embeds data in its runtime rather than as a standalone literal.
So a data file would be a larger change for Bash.
It alone *rejects* `--data-file` at the CLI with an attributed error rather than silently ignoring it.
`--data-file` with `-o -` (`stdout`) is likewise rejected.
The data file needs a real path next to the program.

Go mechanics (decision 29): `//go:embed` is a directive, not a package selector.
So the import scanner cannot see it.
`embed` is added as a blank `import _ "embed"`, and the directive is emitted unchanged.
The scanner's comment-stripping only computes the import *set*; it never rewrites the output.
The directive must sit immediately above its `var`.

## Rejected alternatives

- **Always write a separate data file.**
  Rejected: it forces every user to ship two files.
  It also breaks the single-file `-o -` (`stdout`) path.
  The benefit is real only for data-heavy modules (see Consequences), so it belongs behind a flag.
- **A single in-file Base64 or byte-string literal.**
  Keeps one file but still parses the whole payload as a source token every load.
  Base64 is still 1.33× the raw bytes.
  A binary data file is the actual bytes, which `mmap` can map, and it leaves the parsed source.

## Consequences

Positive: optional, correctness-neutral, default output unchanged.
The output is byte-identical, verified by diffing every backend/mode before and after.
The data payload leaves the parsed source; the data file is the raw bytes.

Measured (release CLI; `ruby.wasm` = CRuby, 35 MB wasm; `qjs.wasm`, 1.5 MB):

| module / backend | embedded source | with `--data-file` | data file |
| --- | --- | --- | --- |
| ruby.wasm / Ruby | 167.4 MB | 159.2 MB | 4.17 MB |
| ruby.wasm / Go | 131.4 MB | 123.2 MB | 4.17 MB |
| qjs.wasm / Ruby | 13.45 MB | 13.19 MB | 0.13 MB |
| qjs.wasm / Go | 10.61 MB | 10.34 MB | 0.13 MB |
| qjs.wasm / Python | 11.46 MB | 11.19 MB | 0.13 MB |
| qjs.wasm / Java | 15.17 MB | 14.99 MB | 0.13 MB |

The Python and Java `qjs.wasm` deltas were measured after this revision, with the release CLI.
They match the Ruby/Go pattern exactly.
The source gets smaller by roughly the eliminated inline encoding (`bytes.fromhex` / chunked Base64).
The shared 0.13 MB data file is subtracted from that.
The result is a ~2-3 % code-dominated reduction.
This confirms the win is a source-size one, not a parse-time one.

Ruby `compile_file` parse of `ruby.wasm`: 6.15 s → 5.97 s.
Go `build` of `qjs.wasm`: 10.8 s → 11.1 s (within noise).
`ruby.wasm`/Go build is not practical at either setting.
CRuby exceeds the practicality bar, see agents/apps-audit.md.

Honest finding: these interpreters are **code-dominated**.
So the source gets smaller by roughly `2 × data_bytes − blob` (the eliminated hexadecimal text).
That is ~8 MB / 5 % for `ruby.wasm`.
It is not the "dominated by data" the motivating framing assumed.
Parse and build time are governed by the code, so they barely move.
The win scales with a module's data:code ratio and is largest for data-heavy modules.
For the current app corpus it is a small source-size reduction, not a parse-time one.

Follow-ups: adjacent-segment merging landed as its own core pass (decision 41).
It reduces the per-segment `memory.init` count.
Python and Java support landed in this revision.
An extra-files channel on the test-helper `BackendUnderTest` is again deliberately deferred.
With the channel, the shared app/e2e suites could exercise data-file output.
Today only the CLI integration test does.
The CLI integration test (`crates/dewasm-cli/tests/data_file.rs`) covers all four backends end-to-end.
So the harness extension would widen coverage, not fill a gap.
It is not worth the changes to the test helper yet.

Related decisions:

- decision 29 (Go lowering / import scanning);
- decision 30 (Java lowering);
- decision 31 (standalone runtime interface: the generated main that loads the data file);
- decision 41 (adjacent data-segment merging, the segment-count follow-up).
