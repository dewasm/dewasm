# ADR-37 — Opt-in Data-Segment Externalization (`--data-file`)

Status: **Accepted, 2026-07-28.** Implemented for the Ruby and Go backends
behind the CLI's `--data-file` flag; Bash, Python and Java reject it loudly at
the CLI. Default (no-flag) output is byte-identical to before.

## Context

A wasm module's data segments are lowered into the generated source as inline
literals: Ruby `["<hex>"].pack("H*")` (`crates/dewasm-backend-ruby/src/lib.rs`,
`hex_bytes`), Go `Rt.unhex("<hex>")` (`crates/dewasm-backend-go/src/lib.rs`,
`hex_bytes`). Hex costs two source bytes per data byte, and the bytes then live
inside the parsed program forever.

For data-heavy modules this bloats the source. wasm2go's `-embed` flag is the
prior art: move the segment bytes into a binary sidecar the program loads at
startup, keeping only offset/length constants in the code. The open question was
whether to do this always, and how to shape the sidecar.

## Decision

Add an **opt-in** `--data-file <path>` CLI flag (ADR-0's "unsupported fails at
conversion time" applies to the rejections below). When set, the backend:

- concatenates every segment's bytes into one blob, returned as a second
  `OutputFile` whose `name` is the sidecar filename; the CLI routes it to
  `--data-file`'s path and the primary source to `-o`;
- bakes per-segment `(blob_offset, len)` prefix sums into codegen; active
  segments init a memory from a blob slice, passive segments keep an addressable
  slice view for `memory.init`/`data.drop`;
- references the sidecar **relative to the program itself**: Ruby
  `File.binread(File.join(__dir__, "<name>"))` read once into a frozen
  `DATA_BLOB` constant, then `DATA_BLOB.byteslice(o, len)` (kept ASCII-8BIT, as
  `binread` returns); Go a package-scope `//go:embed <name>` / `var dataBlob
  []byte` sliced `dataBlob[o:o+len]`.

`OutputFile.contents` widened from `String` to `Vec<u8>` so a backend can return
raw binary alongside UTF-8 source.

**Discriminating rule:** externalization is a size/layout trade-off with no
semantic content, so it is a per-invocation *flag*, never a default and never a
backend capability flip — the generated program's behaviour is identical either
way (the spec harness, which never sets the flag, still binds; ADR-3).

**Scope:** Ruby and Go only. Bash embeds data in its runtime rather than as a
standalone literal, so a sidecar would be a larger change; Python and Java are
deliberately deferred behind the two flagship backends. All three *reject*
`--data-file` at the CLI with an attributed error rather than silently ignoring
it. `--data-file` with `-o -` (stdout) is likewise rejected: the sidecar needs a
real path next to the program.

Go mechanics (ADR-29): `//go:embed` is a directive, not a package selector, so
the import scanner cannot see it — `embed` is added as a blank `import _
"embed"` and the directive is emitted verbatim (the scanner's comment-stripping
only computes the import *set*; it never rewrites the output). The directive
must sit immediately above its `var`.

## Rejected alternatives

- **Always externalize.** Rejected: it forces a two-file deployment on every
  user and breaks the single-file `-o -`/stdout path. The benefit is real only
  for data-heavy modules (see Consequences), so it belongs behind a flag.
- **A single in-file base64/blob literal.** Keeps one file but still parses the
  whole payload as a source token every load, and base64 is still 1.33× the raw
  bytes. A binary sidecar is the actual bytes (mmap-friendly) and leaves the
  parsed source entirely.

## Consequences

Positive: opt-in, correctness-neutral, default output unchanged
(byte-identical, verified by diffing every backend/mode before and after). The
data payload leaves the parsed source; the sidecar is the raw bytes.

Measured (release CLI; `ruby.wasm` = CRuby, 35 MB wasm; `qjs.wasm`, 1.5 MB):

| module / backend | embedded source | with `--data-file` | sidecar |
| --- | --- | --- | --- |
| ruby.wasm / Ruby | 167.4 MB | 159.2 MB | 4.17 MB |
| ruby.wasm / Go | 131.4 MB | 123.2 MB | 4.17 MB |
| qjs.wasm / Ruby | 13.45 MB | 13.19 MB | 0.13 MB |
| qjs.wasm / Go | 10.61 MB | 10.34 MB | 0.13 MB |

Ruby `compile_file` parse of `ruby.wasm`: 6.15 s → 5.97 s. Go `build` of
`qjs.wasm`: 10.8 s → 11.1 s (within noise; `ruby.wasm`/Go build is impractical
at either setting — CRuby exceeds the practicality bar, see docs/apps-audit.md).

Honest finding: these interpreters are **code-dominated**, so the source shrinks
by roughly `2 × data_bytes − blob` (the eliminated hex) — ~8 MB / 5 % for
`ruby.wasm`, not the "dominated by data" the motivating framing assumed. Parse
and build time are governed by the code, so they barely move. The win scales
with a module's data:code ratio and is largest for data-heavy modules; for the
current app corpus it is a modest source-size reduction, not a parse-time one.

Follow-ups (out of scope here): adjacent-segment merging to reduce the
per-segment `memory.init` count; extending the test-helper `BackendUnderTest`
with an aux-files channel so the shared app/e2e suites (not just the CLI
integration test) can exercise sidecar output; and — if warranted by the
numbers — Python/Java support.

Cross-refs: ADR-29 (Go lowering / import scanning), ADR-30 (Java lowering,
deferred here), ADR-31 (standalone runtime interface — the generated main that
loads the sidecar).
