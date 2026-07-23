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
//! `DEWASMIFY_APPS_ALL=1` runs them anyway.
//!
//! `apps_golden_matches_wasmtime` re-validates the golden files
//! themselves against a live `wasmtime run` — the check the old
//! wasmtime-diffing design did on every run, now opt-in behind the
//! `wasmtime_test` feature (`#[ignore]`d otherwise) since `wasmtime` is
//! not part of the default suite's required tools.

use std::path::Path;
use std::process::{Command, Stdio};

use dewasmify_backend::{Backend, GenOptions, Mode, RuntimeLinkage};
use dewasmify_backend_bash::{find_bash5, BashBackend};
use dewasmify_backend_ruby::{find_ruby, RubyBackend};

use crate::support::apps_cache_dir;

struct AppCase {
    name: &'static str,
    args: &'static [&'static str],
    stdin: &'static str,
    /// Captured once via `wasmtime run` (ADR-15); the golden reference
    /// this case's generated-language output must match exactly.
    expect_stdout: &'static str,
    expect_code: i32,
    /// Too slow for the gate on slow interpreters (bash); opt in with
    /// `DEWASMIFY_APPS_ALL=1`.
    heavy: bool,
}

const CASES: &[AppCase] = &[
    AppCase {
        name: "cowsay",
        args: &["Hello", "from", "dewasmify!"],
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
        name: "sqlite",
        args: &[],
        stdin: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);\n\
                INSERT INTO users (name) VALUES (\"alice\"), (\"bob\"), (\"carol\");\n\
                SELECT id, upper(name) FROM users WHERE id >= 2;\n\
                SELECT count(*), avg(id) FROM users;\n",
        expect_stdout: include_str!("../../../../examples/apps/golden/sqlite.stdout"),
        expect_code: 0,
        heavy: true,
    },
];

fn run(cmd: &mut Command, stdin: &str) -> (Vec<u8>, Option<i32>) {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    use std::io::Write as _;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("wait");
    (out.stdout, out.status.code())
}

/// Convert every cached app with `backend`, run it with `interpreter`,
/// and diff against the case's golden output. `run_heavy` gates the
/// `heavy` cases.
fn run_cases(backend: &(dyn Backend + Sync), interpreter: &Path, run_heavy: bool) {
    let cache = apps_cache_dir();
    for case in CASES {
        if case.heavy && !run_heavy {
            println!(
                "{} {:?}: heavy case skipped for {} (DEWASMIFY_APPS_ALL=1 to run)",
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
        let module = dewasmify_core::build_module(&bytes).expect("build IR");
        // Codegen recurses with the IR's control-flow nesting; SQLite's
        // deepest functions exceed the 2 MiB test-thread default stack.
        let src = std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(64 << 20)
                .spawn_scoped(scope, || {
                    backend
                        .generate(
                            &module,
                            &GenOptions {
                                mode: Mode::Standalone,
                                module_name: case.name.to_string(),
                                runtime: RuntimeLinkage::Embedded,
                                default_wasi: true,
                            },
                        )
                        .expect("generate")
                        .remove(0)
                        .contents
                })
                .expect("spawn codegen thread")
                .join()
                .expect("codegen thread")
        });
        let out_path = std::env::temp_dir().join(format!(
            "dewasmify-app-{}-{}.{}",
            backend.name(),
            case.name,
            backend.file_extension()
        ));
        std::fs::write(&out_path, src).unwrap();

        let (our_out, our_code) = run(
            Command::new(interpreter).arg(&out_path).args(case.args),
            case.stdin,
        );

        assert_eq!(
            String::from_utf8_lossy(&our_out),
            case.expect_stdout,
            "{} {:?} under {}: stdout differs from the golden output",
            case.name,
            case.args,
            backend.name()
        );
        assert_eq!(
            our_code,
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

#[test]
fn apps_bash() {
    let bash = find_bash5().expect("bash >= 5 not found — see docs/testing.md");
    run_cases(
        &BashBackend,
        &bash,
        std::env::var("DEWASMIFY_APPS_ALL").is_ok(),
    );
}

/// Golden-file freshness check: does `examples/apps/golden/*.stdout`
/// still match what the currently-cached binary actually produces under
/// a real `wasmtime run`? Run after re-pinning an app version in
/// `examples/apps/fetch.sh`, or any time you doubt a golden file (see
/// docs/testing.md):
///
/// ```console
/// $ cargo test -p dewasmify-cli --test e2e --features wasmtime_test apps_golden_matches_wasmtime
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
        let (wt_out, wt_code) = run(
            Command::new("wasmtime").arg(&wasm_path).args(case.args),
            case.stdin,
        );
        assert_eq!(
            String::from_utf8_lossy(&wt_out),
            case.expect_stdout,
            "{} {:?}: golden stdout is stale — regenerate it (docs/testing.md)",
            case.name,
            case.args
        );
        assert_eq!(
            wt_code,
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
