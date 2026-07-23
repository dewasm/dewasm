//! End-to-end tests over real-world apps from the Wasmer registry
//! (examples/apps/, ADR-9): convert each cached app to standalone Ruby
//! and require byte-identical stdout and exit status against wasmtime.
//!
//! Self-skips when the cache (populated by examples/apps/fetch.sh),
//! `ruby`, or `wasmtime` is missing.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use dewasmify_backend::{Backend, GenOptions, Mode, RuntimeLinkage};
use dewasmify_backend_ruby::RubyBackend;

struct AppCase {
    name: &'static str,
    args: &'static [&'static str],
    stdin: &'static str,
}

const CASES: &[AppCase] = &[
    AppCase { name: "cowsay", args: &["Hello", "from", "dewasmify!"], stdin: "" },
    AppCase { name: "cowsay", args: &[], stdin: "moo via stdin\n" },
    AppCase {
        name: "qjs",
        args: &[
            "-e",
            r#"console.log("2^16 =", Math.pow(2, 16)); console.log(JSON.stringify([3,1,2].sort()));"#,
        ],
        stdin: "",
    },
    AppCase {
        name: "sqlite",
        args: &[],
        stdin: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);\n\
                INSERT INTO users (name) VALUES (\"alice\"), (\"bob\"), (\"carol\");\n\
                SELECT id, upper(name) FROM users WHERE id >= 2;\n\
                SELECT count(*), avg(id) FROM users;\n",
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
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let out = child.wait_with_output().expect("wait");
    (out.stdout, out.status.code())
}

#[test]
fn apps_match_wasmtime() {
    if !tool_available("ruby") || !tool_available("wasmtime") {
        eprintln!("ruby or wasmtime not found; skipping");
        return;
    }
    let cache = cache_dir();

    for case in CASES {
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
        let ruby_src = RubyBackend
            .generate(
                &module,
                &GenOptions {
                    mode: Mode::Standalone,
                    module_name: case.name.to_string(),
                    runtime: RuntimeLinkage::Embedded,
                    default_wasi: true,
                },
            )
            .expect("generate ruby")
            .remove(0)
            .contents;
        let rb_path = std::env::temp_dir().join(format!("dewasmify-app-{}.rb", case.name));
        std::fs::write(&rb_path, ruby_src).unwrap();

        let (ruby_out, ruby_code) =
            run(Command::new("ruby").arg(&rb_path).args(case.args), case.stdin);
        let (wt_out, wt_code) =
            run(Command::new("wasmtime").arg(&wasm_path).args(case.args), case.stdin);

        assert_eq!(
            String::from_utf8_lossy(&ruby_out),
            String::from_utf8_lossy(&wt_out),
            "{} {:?}: stdout differs from wasmtime",
            case.name,
            case.args
        );
        assert_eq!(ruby_code, wt_code, "{}: exit status differs", case.name);
        println!("{} {:?}: identical to wasmtime", case.name, case.args);
    }
}
