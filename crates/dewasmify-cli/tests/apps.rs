//! End-to-end tests over real-world apps from the Wasmer registry
//! (examples/apps/, ADR-9): convert each cached app with a backend and
//! require byte-identical stdout and exit status against wasmtime.
//!
//! Self-skips when the cache (populated by examples/apps/fetch.sh),
//! the interpreter, or `wasmtime` is missing. Cases marked `heavy` are
//! skipped under bash by default (softfloat makes QuickJS/SQLite take
//! tens of seconds); `DEWASMIFY_APPS_ALL=1` runs them anyway.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use dewasmify_backend::{Backend, GenOptions, Mode, RuntimeLinkage};
use dewasmify_backend_bash::{find_bash5, BashBackend};
use dewasmify_backend_ruby::RubyBackend;

struct AppCase {
    name: &'static str,
    args: &'static [&'static str],
    stdin: &'static str,
    /// Too slow for the gate on slow interpreters (bash); opt in with
    /// `DEWASMIFY_APPS_ALL=1`.
    heavy: bool,
}

const CASES: &[AppCase] = &[
    AppCase {
        name: "cowsay",
        args: &["Hello", "from", "dewasmify!"],
        stdin: "",
        heavy: false,
    },
    AppCase {
        name: "cowsay",
        args: &[],
        stdin: "moo via stdin\n",
        heavy: false,
    },
    AppCase {
        name: "qjs",
        args: &[
            "-e",
            r#"console.log("2^16 =", Math.pow(2, 16)); console.log(JSON.stringify([3,1,2].sort()));"#,
        ],
        stdin: "",
        heavy: true,
    },
    AppCase {
        name: "sqlite",
        args: &[],
        stdin: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);\n\
                INSERT INTO users (name) VALUES (\"alice\"), (\"bob\"), (\"carol\");\n\
                SELECT id, upper(name) FROM users WHERE id >= 2;\n\
                SELECT count(*), avg(id) FROM users;\n",
        heavy: true,
    },
];

fn tool_available(name: &str) -> bool {
    Command::new(name).arg("--version").output().is_ok()
}

fn cache_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/apps/cache")
}

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
/// and diff against wasmtime. `run_heavy` gates the `heavy` cases.
fn run_cases(backend: &(dyn Backend + Sync), interpreter: &Path, run_heavy: bool) {
    let cache = cache_dir();
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
        if !wasm_path.exists() {
            eprintln!(
                "{} not cached (run examples/apps/fetch.sh); skipping",
                case.name
            );
            continue;
        }
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
        let (wt_out, wt_code) = run(
            Command::new("wasmtime").arg(&wasm_path).args(case.args),
            case.stdin,
        );

        assert_eq!(
            String::from_utf8_lossy(&our_out),
            String::from_utf8_lossy(&wt_out),
            "{} {:?} under {}: stdout differs from wasmtime",
            case.name,
            case.args,
            backend.name()
        );
        assert_eq!(
            our_code,
            wt_code,
            "{} under {}: exit status differs",
            case.name,
            backend.name()
        );
        println!(
            "{} {:?} under {}: identical to wasmtime",
            case.name,
            case.args,
            backend.name()
        );
    }
}

#[test]
fn apps_match_wasmtime() {
    if !tool_available("ruby") || !tool_available("wasmtime") {
        eprintln!("ruby or wasmtime not found; skipping");
        return;
    }
    run_cases(&RubyBackend, Path::new("ruby"), true);
}

#[test]
fn apps_match_wasmtime_bash() {
    let Some(bash) = find_bash5() else {
        eprintln!("bash >= 5 not found; skipping");
        return;
    };
    if !tool_available("wasmtime") {
        eprintln!("wasmtime not found; skipping");
        return;
    }
    run_cases(
        &BashBackend,
        &bash,
        std::env::var("DEWASMIFY_APPS_ALL").is_ok(),
    );
}
