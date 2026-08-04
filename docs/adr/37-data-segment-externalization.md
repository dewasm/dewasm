# ADR-37 — Opt-in Data-Segment Externalization (`--data-file`)

Status: **Accepted, 2026-07-28.** Implemented for the Ruby, Go, Python and Java backends behind the CLI's `--data-file` flag; Bash rejects it loudly at the CLI (its data lives in the runtime, not a standalone literal). Default (no-flag) output is byte-identical to before. Extended 2026-07-28 with the Python and Java backends and the Java code-source load strategy.

## Context

A wasm module's data segments are lowered into the generated source as inline literals: Ruby `["<hex>"].pack("H*")` (`crates/dewasm-backend-ruby/src/lib.rs`, `hex_bytes`), Go `Rt.unhex("<hex>")` (`crates/dewasm-backend-go/src/lib.rs`, `hex_bytes`). Hex costs two source bytes per data byte, and the bytes then live inside the parsed program forever.

For data-heavy modules this bloats the source. wasm2go's `-embed` flag is the prior art: move the segment bytes into a binary sidecar the program loads at startup, keeping only offset/length constants in the code. The open question was whether to do this always, and how to shape the sidecar.

## Decision

Add an **opt-in** `--data-file <path>` CLI flag (ADR-0's "unsupported fails at conversion time" applies to the rejections below). When set, the backend:

- concatenates every segment's bytes into one blob, returned as a second `OutputFile` whose `name` is the sidecar filename; the CLI routes it to `--data-file`'s path and the primary source to `-o`;
- bakes per-segment `(blob_offset, len)` prefix sums into codegen; active segments init a memory from a blob slice, passive segments keep an addressable slice view for `memory.init`/`data.drop`;
- references the sidecar **relative to the program itself**: Ruby `File.binread(File.join(__dir__, "<name>"))` read once into a frozen `DATA_BLOB` constant, then `DATA_BLOB.byteslice(o, len)` (kept ASCII-8BIT, as `binread` returns); Go a package-scope `//go:embed <name>` / `var dataBlob []byte` sliced `dataBlob[o:o+len]`; Python a module-level `DATA_BLOB = open(os.path.join(os.path.dirname(__file__), "<name>"), "rb").read()` sliced `DATA_BLOB[o:o+len]`; Java a `static final byte[] DATA_BLOB` loaded by a static method (see the code-source rule below) and sliced `java.util.Arrays.copyOfRange(DATA_BLOB, o, o+len)` — a fresh array per segment, so `data.drop` can still empty the field.

**Java load strategy — code-source-relative, not a working-directory guess.** Java has no `__file__`/`__dir__`, and a compiled program may run from a class directory or a packaged jar. The generated loader resolves the sidecar against the class's own `getProtectionDomain().getCodeSource().getLocation()`: a regular file (the jar) contributes its *parent* directory, a directory (the class dir) is used directly, and the sidecar name is resolved there with `Files.readAllBytes`, rethrowing any failure as an unchecked `RuntimeException`. The discriminating criterion is locating the sidecar by *where the program's own code lives*, which both deployment shapes answer, rather than by the process CWD, which neither reliably does.

`OutputFile.contents` widened from `String` to `Vec<u8>` so a backend can return raw binary alongside UTF-8 source.

**Discriminating rule:** externalization is a size/layout trade-off with no semantic content, so it is a per-invocation *flag*, never a default and never a backend capability flip — the generated program's behaviour is identical either way (the spec harness, which never sets the flag, still binds; ADR-3).

**Scope:** Ruby, Go, Python and Java. Bash embeds data in its runtime rather than as a standalone literal, so a sidecar would be a larger change; it alone *rejects* `--data-file` at the CLI with an attributed error rather than silently ignoring it. `--data-file` with `-o -` (stdout) is likewise rejected: the sidecar needs a real path next to the program.

Go mechanics (ADR-29): `//go:embed` is a directive, not a package selector, so the import scanner cannot see it — `embed` is added as a blank `import _ "embed"` and the directive is emitted verbatim (the scanner's comment-stripping only computes the import *set*; it never rewrites the output). The directive must sit immediately above its `var`.

## Rejected alternatives

- **Always externalize.** Rejected: it forces a two-file deployment on every user and breaks the single-file `-o -`/stdout path. The benefit is real only for data-heavy modules (see Consequences), so it belongs behind a flag.
- **A single in-file base64/blob literal.** Keeps one file but still parses the whole payload as a source token every load, and base64 is still 1.33× the raw bytes. A binary sidecar is the actual bytes (mmap-friendly) and leaves the parsed source entirely.

## Consequences

Positive: opt-in, correctness-neutral, default output unchanged (byte-identical, verified by diffing every backend/mode before and after). The data payload leaves the parsed source; the sidecar is the raw bytes.

Measured (release CLI; `ruby.wasm` = CRuby, 35 MB wasm; `qjs.wasm`, 1.5 MB):

| module / backend | embedded source | with `--data-file` | sidecar |
| --- | --- | --- | --- |
| ruby.wasm / Ruby | 167.4 MB | 159.2 MB | 4.17 MB |
| ruby.wasm / Go | 131.4 MB | 123.2 MB | 4.17 MB |
| qjs.wasm / Ruby | 13.45 MB | 13.19 MB | 0.13 MB |
| qjs.wasm / Go | 10.61 MB | 10.34 MB | 0.13 MB |
| qjs.wasm / Python | 11.46 MB | 11.19 MB | 0.13 MB |
| qjs.wasm / Java | 15.17 MB | 14.99 MB | 0.13 MB |

The Python and Java `qjs.wasm` deltas (measured after this revision, release CLI) match the Ruby/Go pattern exactly: the source shrinks by roughly the eliminated inline encoding (`bytes.fromhex` / chunked Base64) minus the shared 0.13 MB sidecar, a ~2–3 % code-dominated reduction, confirming the win is a source-size one, not a parse-time one.

Ruby `compile_file` parse of `ruby.wasm`: 6.15 s → 5.97 s. Go `build` of `qjs.wasm`: 10.8 s → 11.1 s (within noise; `ruby.wasm`/Go build is impractical at either setting — CRuby exceeds the practicality bar, see docs/apps-audit.md).

Honest finding: these interpreters are **code-dominated**, so the source shrinks by roughly `2 × data_bytes − blob` (the eliminated hex) — ~8 MB / 5 % for `ruby.wasm`, not the "dominated by data" the motivating framing assumed. Parse and build time are governed by the code, so they barely move. The win scales with a module's data:code ratio and is largest for data-heavy modules; for the current app corpus it is a modest source-size reduction, not a parse-time one.

Follow-ups: adjacent-segment merging to reduce the per-segment `memory.init` count landed as its own core pass (ADR-41). Python and Java support landed in this revision. Extending the test-helper `BackendUnderTest` with an aux-files channel — so the shared app/e2e suites (not just the CLI integration test) can exercise sidecar output — is again deliberately deferred: the CLI integration test (`crates/dewasm-cli/tests/data_file.rs`) covers all four backends end-to-end, so the harness extension buys coverage breadth, not a gap, and is not worth the test-helper churn yet.

Cross-refs: ADR-29 (Go lowering / import scanning), ADR-30 (Java lowering), ADR-31 (standalone runtime interface — the generated main that loads the sidecar), ADR-41 (adjacent data-segment merging, the segment-count follow-up).
