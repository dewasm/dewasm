# Rails on dewasm SQLite

A full Rails 8 application.
Its database is **SQLite compiled to wasm32-wasi and converted to pure Ruby by dewasm**.
No native sqlite3 library is anywhere in the process.

```
Rails 8 (unmodified)
  └─ ActiveRecord SQLite3Adapter (unmodified)
       └─ sqlite3/: shim gem, the sqlite3-ruby API the adapter uses
            └─ sqlite3_wasm.rb: libsqlite3.wasm converted by dewasm (~17 MB, generated)
                 └─ Rt::WASI: dewasm's WASI, preopening "/" so the .sqlite3
                              file lands on the real filesystem
```

## Run it

```console
$ ./run.sh
```

That builds `libsqlite3.wasm` (needs wasi-sdk, `WASI_SDK_PATH`) and converts it with dewasm.
It runs the shim + ActiveRecord smokes.
On first use, it generates the Rails app (needs the `rails` gem, >= 8.1).
It then migrates, boots the server, and drives it over HTTP.
It ends with `RAILS-ON-DEWASM-OK` after a POST/GET round-trip and a `/stats` request.
The wasm-converted SQLite answers that request's `sqlite_version`: `3.53.3-wasm`.
The wasm build (`../apps/scripts/sqlite3.sh`) patches in that suffix.
So the output identifies the converted engine rather than looking like a native SQLite.

Pieces, individually:

- `build.sh`: wasm build + conversion only.
- `sqlite3/test_shim.rb`: the shim's own smoke.
  It covers the gem API surface: binds, column typing, error mapping, transactions, pragmas.
- `ar_smoke/`: ActiveRecord without Rails.
  It covers migration, CRUD, and type round-trips (UTF-8, blob, i64 boundaries, datetime).
  It also covers constraint→exception mapping, joins, and `insert_all`.
- `setup-app.sh`: regenerates `app/` (`rails new --minimal` plus the files in `app-template/`).

## The shim gem (`sqlite3/`)

A drop-in `sqlite3` gem (Bundler `path:` dependency) implementing the surface Rails 8.1 actually uses.
It is verified against the real gem 2.9.5 and the adapter source:

- `Database`: `prepare`/`execute`/`execute_batch2`, `changes`/`total_changes`, and `closed?`/`close`.
  It also has `encoding`, `busy_handler_timeout=`, and transactions.
- `Statement`: `bind_params`, `step`, `columns`, `types`, `reset!`, `column_count`.
  `step` returns array rows typed INTEGER→Integer, FLOAT→Float, and TEXT→UTF-8 String.
  It types BLOB→binary String and NULL→nil.
  `types` returns the declared types, which feed AR's type map.
- `Pragmas` as a real module with real setter methods, since Rails introspects `method_defined?`.
- The full exception hierarchy, keyed by result code.
  sqlite's own error strings pass through, which is what Rails' constraint-violation regexes match.
- `Constants::Open`, `ForkSafety`.

Each `Database` gets its own wasm instance, with its own linear memory and guest heap.
So a Rails connection-pool entry is a fully isolated SQLite.
A mutex serializes calls into each instance.
Values cross the host/guest boundary through the generated module's `invoke` + `Rt::Memory`.
Guest-side buffers use `sqlite3_malloc`, and i64 uses the masked-unsigned convention.

`libsqlite3.wasm` exports the C surface this needs.
The list is `SQLITE_EXPORTS` in `../apps/scripts/sqlite3.sh`.

## Deliberate gaps

- **No guest→host callbacks**: `busy_handler_timeout=` maps to sqlite's built-in `sqlite3_busy_timeout`.
  That gives the same observable behavior for Rails.
  `execute_batch2` is a prepare/step loop instead of `sqlite3_exec`.
  `create_function`, collations, tracing are out.
- **WAL silently degrades**: WASI has no shared memory.
  So Rails' default `journal_mode = wal` pragma leaves the database in rollback-journal mode.
  sqlite reports the old mode, and nothing errors.
- **No real file locking**: WASI p1 has no `fcntl` locks.
  So concurrent writers from *separate processes* are unprotected.
  Inside one server process the per-connection mutexes plus sqlite's busy timeout apply.
- `strict:` (needs varargs `sqlite3_db_config`) and `extensions:` are not supported.
  The shim ignores the former and raises on the latter.

## Numbers (M-series laptop, development mode)

- `ruby test_shim.rb` (open + 30 statements): ~0.9 s total.
- Rails boot (`rails runner`): ~1.6 s.
- Simple JSON request with 1-4 queries: ~20 ms end-to-end.
  Individual ActiveRecord queries log at 0.5-2 ms.
