//! End-to-end tests over real-world apps (examples/apps/, ADR-9): convert
//! each cached app with a backend and require byte-identical stdout and
//! exit status against a golden output captured once from wasmtime and
//! checked into `examples/apps/golden/` (ADR-15) — running these tests
//! does not itself need `wasmtime` installed.
//!
//! Per ADR-15, missing prerequisites (the interpreter, or the cache
//! populated by `examples/apps/fetch.sh`) fail the test, they don't skip
//! it — see docs/testing.md. Cases marked `heavy` are skipped under bash
//! by default (softfloat makes QuickJS/SQLite take tens of seconds);
//! `DEWASM_APPS_ALL=1` runs them anyway.
//!
//! `apps_golden_matches_wasmtime` re-validates the golden files
//! themselves against a live `wasmtime run` — the check the old
//! wasmtime-diffing design did on every run, now opt-in behind the
//! `wasmtime_test` feature (`#[ignore]`d otherwise) since `wasmtime` is
//! not part of the default suite's required tools.

use std::path::Path;
use std::process::Command;

use dewasm_backend::{Backend, Mode};
use dewasm_backend_bash::{find_bash5, BashBackend};
use dewasm_backend_ruby::{find_ruby, RubyBackend};

use crate::support::{apps_cache_dir, convert_bytes, run_command, run_script};

struct AppCase {
    name: &'static str,
    args: &'static [&'static str],
    stdin: &'static str,
    /// Captured once via `wasmtime run` (ADR-15); the golden reference
    /// this case's generated-language output must match exactly.
    expect_stdout: &'static str,
    expect_code: i32,
    /// Too slow for the gate on slow interpreters (bash); opt in with
    /// `DEWASM_APPS_ALL=1`.
    heavy: bool,
}

const CASES: &[AppCase] = &[
    AppCase {
        name: "cowsay",
        args: &["Hello", "from", "dewasm!"],
        stdin: "",
        expect_stdout: include_str!("../../../../examples/apps/golden/cowsay_args.stdout"),
        expect_code: 0,
        heavy: false,
    },
    AppCase {
        name: "cowsay",
        args: &[],
        stdin: "moo via stdin\n",
        expect_stdout: include_str!("../../../../examples/apps/golden/cowsay_stdin.stdout"),
        expect_code: 0,
        heavy: false,
    },
    AppCase {
        name: "qjs",
        args: &[
            "-e",
            r#"console.log("2^16 =", Math.pow(2, 16)); console.log(JSON.stringify([3,1,2].sort()));"#,
        ],
        stdin: "",
        expect_stdout: include_str!("../../../../examples/apps/golden/qjs.stdout"),
        expect_code: 0,
        heavy: true,
    },
    AppCase {
        name: "sqlite3-shell",
        args: &[],
        stdin: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);\n\
                INSERT INTO users (name) VALUES (\"alice\"), (\"bob\"), (\"carol\");\n\
                SELECT id, upper(name) FROM users WHERE id >= 2;\n\
                SELECT count(*), avg(id) FROM users;\n",
        expect_stdout: include_str!("../../../../examples/apps/golden/sqlite3_shell.stdout"),
        expect_code: 0,
        heavy: true,
    },
];

/// Codegen recurses with the IR's control-flow nesting; SQLite's deepest
/// functions exceed the 2 MiB test-thread default stack.
fn convert_on_big_stack(
    backend: &(dyn Backend + Sync),
    bytes: &[u8],
    mode: Mode,
    name: &str,
) -> String {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn_scoped(scope, || convert_bytes(backend, bytes, mode, name))
            .expect("spawn codegen thread")
            .join()
            .expect("codegen thread")
    })
}

/// Convert every cached app with `backend`, run it with `interpreter`,
/// and diff against the case's golden output. `run_heavy` gates the
/// `heavy` cases.
fn run_cases(backend: &(dyn Backend + Sync), interpreter: &Path, run_heavy: bool) {
    let cache = apps_cache_dir();
    for case in CASES {
        if case.heavy && !run_heavy {
            println!(
                "{} {:?}: heavy case skipped for {} (DEWASM_APPS_ALL=1 to run)",
                case.name,
                case.args,
                backend.name()
            );
            continue;
        }
        let wasm_path = cache.join(format!("{}.wasm", case.name));
        assert!(
            wasm_path.exists(),
            "{} not cached — run examples/apps/fetch.sh (see docs/testing.md)",
            case.name
        );
        let bytes = std::fs::read(&wasm_path).expect("read wasm");
        let src = convert_on_big_stack(backend, &bytes, Mode::Standalone, case.name);
        let output = run_script(
            interpreter,
            &src,
            backend.file_extension(),
            case.args,
            case.stdin,
        );

        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            case.expect_stdout,
            "{} {:?} under {}: stdout differs from the golden output",
            case.name,
            case.args,
            backend.name()
        );
        assert_eq!(
            output.status.code(),
            Some(case.expect_code),
            "{} under {}: exit status differs",
            case.name,
            backend.name()
        );
        println!(
            "{} {:?} under {}: matches golden output",
            case.name,
            case.args,
            backend.name()
        );
    }
}

#[test]
fn apps_ruby() {
    let ruby = find_ruby().expect("ruby not found on PATH — see docs/testing.md");
    run_cases(&RubyBackend, &ruby, true);
}

/// Ruby glue driving the sqlite3 C API of the reactor library build:
/// `_initialize`, guest-memory pointer plumbing via `sqlite3_malloc` +
/// `Rt::Memory`, and the open/exec/prepare/step/column/finalize/close
/// flow — the shape a sqlite3-gem-compatible shim will use.
const LIBSQLITE3_GLUE: &str = r##"
db_mod = Libsqlite3.new
db_mod.invoke("_initialize")
mem = db_mod.memory

def read_cstr(mem, ptr)
  return nil if ptr.zero?
  fin = mem.bytes.index("\0".b, ptr)
  mem.read_string(ptr, fin - ptr)
end

def cstr(db_mod, mem, s)
  p = db_mod.invoke("sqlite3_malloc", s.bytesize + 1)
  mem.init(p, "#{s}\0", 0, s.bytesize + 1)
  p
end

puts "version: #{read_cstr(mem, db_mod.invoke('sqlite3_libversion'))}"

pp_db = db_mod.invoke("sqlite3_malloc", 4)
rc = db_mod.invoke("sqlite3_open", cstr(db_mod, mem, ":memory:"), pp_db)
raise "open rc=#{rc}" unless rc.zero?
db = mem.i32_load(pp_db)

sql = "create table t(a,b); insert into t values (1,'x'),(2,'y');"
rc = db_mod.invoke("sqlite3_exec", db, cstr(db_mod, mem, sql), 0, 0, 0)
raise "exec rc=#{rc}: #{read_cstr(mem, db_mod.invoke('sqlite3_errmsg', db))}" unless rc.zero?

pp_stmt = db_mod.invoke("sqlite3_malloc", 4)
rc = db_mod.invoke("sqlite3_prepare_v2", db,
                   cstr(db_mod, mem, "select a*10, b from t order by a desc"),
                   0xffffffff, pp_stmt, 0) # -1 as masked-unsigned i32
raise "prepare rc=#{rc}" unless rc.zero?
stmt = mem.i32_load(pp_stmt)

while db_mod.invoke("sqlite3_step", stmt) == 100 # SQLITE_ROW
  row = (0...db_mod.invoke("sqlite3_column_count", stmt)).map do |i|
    read_cstr(mem, db_mod.invoke("sqlite3_column_text", stmt, i))
  end
  puts row.join("|")
end
db_mod.invoke("sqlite3_finalize", stmt)
db_mod.invoke("sqlite3_close", db)
puts "C-API-OK"
"##;

/// The library half of the sqlite3 build (ADR-22): the C API driven from
/// Ruby. No golden-vs-wasmtime here — the wasmtime CLI cannot drive this
/// flow (results live in guest memory) — so the expectation is a fixed
/// string; every value in it is pinned by the amalgamation version in
/// examples/apps/fetch.sh.
#[test]
fn libsqlite3_c_api_ruby() {
    let ruby = find_ruby().expect("ruby not found on PATH — see docs/testing.md");
    let wasm_path = apps_cache_dir().join("libsqlite3.wasm");
    assert!(
        wasm_path.exists(),
        "libsqlite3 not cached — run examples/apps/fetch.sh (see docs/testing.md)"
    );
    let bytes = std::fs::read(&wasm_path).expect("read wasm");
    let class = convert_on_big_stack(&RubyBackend, &bytes, Mode::Library, "libsqlite3");
    let output = run_script(&ruby, &format!("{class}\n{LIBSQLITE3_GLUE}"), "rb", &[], "");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "version: 3.53.3\n20|y\n10|x\nC-API-OK\n",
        "libsqlite3 C API drive under ruby: output differs\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn apps_bash() {
    let bash = find_bash5().expect("bash >= 5 not found — see docs/testing.md");
    run_cases(
        &BashBackend,
        &bash,
        std::env::var("DEWASM_APPS_ALL").is_ok(),
    );
}

/// Golden-file freshness check: does `examples/apps/golden/*.stdout`
/// still match what the currently-cached binary actually produces under
/// a real `wasmtime run`? Run after re-pinning an app version in
/// `examples/apps/fetch.sh`, or any time you doubt a golden file (see
/// docs/testing.md):
///
/// ```console
/// $ cargo test -p dewasm-cli --test e2e --features wasmtime_test apps_golden_matches_wasmtime
/// ```
///
/// Ignored by default and behind the `wasmtime_test` feature (rather
/// than always-on) because `wasmtime` is deliberately not one of the
/// default suite's required tools (ADR-15) — this test exists to check
/// the checker, not to run on every `cargo test`.
#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn apps_golden_matches_wasmtime() {
    assert!(
        Command::new("wasmtime").arg("--version").output().is_ok(),
        "wasmtime not found on PATH — required when running with --features wasmtime_test"
    );
    let cache = apps_cache_dir();
    for case in CASES {
        let wasm_path = cache.join(format!("{}.wasm", case.name));
        assert!(
            wasm_path.exists(),
            "{} not cached — run examples/apps/fetch.sh (see docs/testing.md)",
            case.name
        );
        let output = run_command(
            Command::new("wasmtime").arg(&wasm_path).args(case.args),
            case.stdin,
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            case.expect_stdout,
            "{} {:?}: golden stdout is stale — regenerate it (docs/testing.md)",
            case.name,
            case.args
        );
        assert_eq!(
            output.status.code(),
            Some(case.expect_code),
            "{} {:?}: golden exit code is stale — regenerate it (docs/testing.md)",
            case.name,
            case.args
        );
        println!(
            "{} {:?}: golden output matches wasmtime",
            case.name, case.args
        );
    }
}
