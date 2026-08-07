/*
 * sqlite3_binding.c — our own committed C source (first-party source is
 * fine to commit; only third-party *artifacts* stay out of the tree).
 *
 * Proves a guest->host callback round trip in library mode (Phase 5a): the
 * exported run_query() calls sqlite3_exec() with a C callback that forwards
 * every result row to an *imported* host function, env.host_row. The Ruby
 * apps e2e supplies host_row via the import-provider mechanism and
 * collects the rows. Built into cache/sqlite3-binding.wasm by
 * examples/apps/scripts/sqlite3.sh (a third artifact alongside the shell and the plain
 * reactor library) from the same pinned amalgamation, with the same
 * zig build flags.
 */
#include "sqlite3.h"

/*
 * Imported host callback. Minimal ABI: (argc, argv) where argv is a
 * guest-memory pointer to an array of argc NUL-terminated C-string pointers
 * (the sqlite3_exec column-text values for one row). The host reads the
 * strings straight out of guest memory. The import lands as env.host_row so
 * the Ruby side resolves it with imports = { "env" => { "host_row" => ... } }.
 */
__attribute__((import_module("env"), import_name("host_row")))
extern void host_row(int argc, char **argv);

/* sqlite3_exec's per-row trampoline: hand each row to the imported host. */
static int forward_row(void *unused, int argc, char **argv, char **colnames) {
  (void)unused;
  (void)colnames;
  host_row(argc, argv);
  return 0;
}

/*
 * Run sql on db, forwarding every result row to env.host_row. Returns the
 * sqlite3_exec result code (0 == SQLITE_OK).
 */
int run_query(sqlite3 *db, const char *sql) {
  return sqlite3_exec(db, sql, forward_row, 0, 0);
}
