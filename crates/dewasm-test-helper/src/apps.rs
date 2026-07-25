//! End-to-end cases over real-world apps (examples/apps/, ADR-9): convert
//! each cached app with a backend and require byte-identical stdout and exit
//! status against a golden output captured once from wasmtime and checked
//! into `examples/apps/golden/` (ADR-15) — running these does not itself need
//! `wasmtime` installed.
//!
//! Per ADR-15, missing prerequisites (the interpreter, or the cache populated
//! by `examples/apps/fetch.sh`) fail the test, they don't skip it. Cases
//! marked `heavy` are skipped for slow backends by default (softfloat makes
//! QuickJS/SQLite take tens of seconds); `DEWASM_APPS_ALL=1` runs them anyway
//! (as does a backend whose `run_heavy_apps` is true, e.g. Ruby).

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::fixtures::{apps_cache_dir, convert_on_big_stack};

pub struct AppCase {
    pub name: &'static str,
    pub args: &'static [&'static str],
    pub stdin: &'static str,
    /// Captured once via `wasmtime run` (ADR-15); the golden reference this
    /// case's generated-language output must match exactly.
    pub expect_stdout: &'static str,
    pub expect_code: i32,
    /// Too slow for the gate on slow interpreters (bash); opt in with
    /// `DEWASM_APPS_ALL=1`.
    pub heavy: bool,
}

pub const APP_CASES: &[AppCase] = &[
    AppCase {
        name: "cowsay",
        args: &["Hello", "from", "dewasm!"],
        stdin: "",
        expect_stdout: include_str!("../../../examples/apps/golden/cowsay_args.stdout"),
        expect_code: 0,
        heavy: false,
    },
    AppCase {
        name: "cowsay",
        args: &[],
        stdin: "moo via stdin\n",
        expect_stdout: include_str!("../../../examples/apps/golden/cowsay_stdin.stdout"),
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
        expect_stdout: include_str!("../../../examples/apps/golden/qjs.stdout"),
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
        expect_stdout: include_str!("../../../examples/apps/golden/sqlite3_shell.stdout"),
        expect_code: 0,
        heavy: true,
    },
];

/// Convert every cached app with `lang`'s backend, run it, and diff against
/// the case's golden output. `heavy` cases run when the backend opts in
/// (`run_heavy_apps`) or `DEWASM_APPS_ALL` is set.
pub fn run_app_cases(lang: &dyn BackendUnderTest) {
    let run_heavy = lang.run_heavy_apps() || std::env::var("DEWASM_APPS_ALL").is_ok();
    let cache = apps_cache_dir();
    for case in APP_CASES {
        if case.heavy && !run_heavy {
            println!(
                "{} {:?}: heavy case skipped for {} (DEWASM_APPS_ALL=1 to run)",
                case.name,
                case.args,
                lang.name()
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
        let src = convert_on_big_stack(lang.backend(), &bytes, Mode::Standalone, case.name);
        let output = lang.run(&src, case.args, case.stdin);

        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            case.expect_stdout,
            "{} {:?} under {}: stdout differs from the golden output",
            case.name,
            case.args,
            lang.name()
        );
        assert_eq!(
            output.status.code(),
            Some(case.expect_code),
            "{} under {}: exit status differs",
            case.name,
            lang.name()
        );
        println!(
            "{} {:?} under {}: matches golden output",
            case.name,
            case.args,
            lang.name()
        );
    }
}
