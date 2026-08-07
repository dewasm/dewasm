# Rails on dewasm SQLite

A full Rails 8 application whose database is **SQLite compiled to
wasm32-wasi and converted to pure Ruby by dewasm** — no native sqlite3
library anywhere in the process.

```
Rails 8 (unmodified)
  └─ ActiveRecord SQLite3Adapter (unmodified)
       └─ sqlite3/           — shim gem: the sqlite3-ruby API the adapter uses
            └─ sqlite3_wasm.rb  — libsqlite3.wasm converted by dewasm (~17 MB, generated)
                 └─ Rt::WASI     — dewasm's WASI, preopening "/" so the .sqlite3
                                   file lands on the real filesystem
```

## Run it

```console
$ ./run.sh
```

That builds `libsqlite3.wasm` (needs `zig`), converts it with dewasm-cli,
runs the shim + ActiveRecord smokes, generates the Rails app on first use
(needs the `rails` gem, >= 8.1), migrates, boots the server, and drives it
over HTTP. It ends with `RAILS-ON-DEWASM-OK` after a POST/GET round-trip and
a `/stats` request whose `sqlite_version` is answered by the wasm-converted
SQLite (3.53.3).

Pieces, individually:

- `build.sh` — wasm build + conversion only.
- `sqlite3/test_shim.rb` — the shim's own smoke (gem API surface: binds,
  column typing, error mapping, transactions, pragmas).
- `ar_smoke/` — ActiveRecord without Rails: migration, CRUD, type
  round-trips (UTF-8, blob, i64 boundaries, datetime), constraint→exception
  mapping, joins, `insert_all`.
- `setup-app.sh` — regenerates `app/` (`rails new --minimal` plus the files
  in `app-template/`).

## The shim gem (`sqlite3/`)

A drop-in `sqlite3` gem (Bundler `path:` dependency) implementing the
surface Rails 8.1 actually uses — verified against the real gem 2.9.5 and
the adapter source:

- `Database`: `prepare`/`execute`/`execute_batch2`, `changes`/`total_changes`,
  `closed?`/`close`, `encoding`, `busy_handler_timeout=`, transactions.
- `Statement`: `bind_params`, `step` (array rows typed INTEGER→Integer,
  FLOAT→Float, TEXT→UTF-8 String, BLOB→binary String, NULL→nil), `columns`,
  `types` (declared types feed AR's type map), `reset!`, `column_count`.
- `Pragmas` as a real module with real setter methods (Rails introspects
  `method_defined?`), the full exception hierarchy keyed by result code
  (sqlite's own error strings pass through, which is what Rails'
  constraint-violation regexes match), `Constants::Open`, `ForkSafety`.

Each `Database` gets its own wasm instance (own linear memory and guest
heap), so a Rails connection-pool entry is a fully isolated SQLite; a mutex
serializes calls into each instance. Values cross the host/guest boundary
through the generated module's `invoke` + `Rt::Memory`, with `sqlite3_malloc`
for guest-side buffers and the masked-unsigned convention for i64.

The C surface this needs is exported from `libsqlite3.wasm` — the
`SQLITE_EXPORTS` list in `../apps/scripts/sqlite3.sh`.

## Deliberate gaps

- **No guest→host callbacks**: `busy_handler_timeout=` maps to sqlite's
  built-in `sqlite3_busy_timeout` (same observable behavior for Rails), and
  `execute_batch2` is a prepare/step loop instead of `sqlite3_exec`.
  `create_function`, collations, tracing are out.
- **WAL silently degrades**: WASI has no shared memory, so Rails' default
  `journal_mode = wal` pragma leaves the database in rollback-journal mode
  (sqlite reports the old mode; nothing errors).
- **No real file locking**: WASI p1 has no `fcntl` locks, so concurrent
  writers from *separate processes* are unprotected. Inside one server
  process the per-connection mutexes plus sqlite's busy timeout apply.
- `strict:` (needs varargs `sqlite3_db_config`) and `extensions:` are not
  supported; the shim ignores the former and raises on the latter.

## Numbers (M-series laptop, development mode)

- `ruby test_shim.rb` (open + 30 statements): ~0.9 s total.
- Rails boot (`rails runner`): ~1.6 s.
- Simple JSON request with 1-4 queries: ~20 ms end-to-end;
  individual ActiveRecord queries log at 0.5-2 ms.
