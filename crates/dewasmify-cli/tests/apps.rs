//! End-to-end tests over real-world apps from the Wasmer registry
//! (examples/apps/, ADR-9): convert each cached app to standalone Ruby
//! (and, for the fast cases, Bash) and require byte-identical stdout and
//! exit status against wasmtime.
//!
//! Self-skips when the cache (populated by examples/apps/fetch.sh),
//! the interpreter, or `wasmtime` is missing.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use dewasmify_backend::{Backend, GenOptions, Mode, RuntimeLinkage};
use dewasmify_backend_bash::BashBackend;
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

/// A bash >= 5 interpreter, if one is installed.
fn bash5() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(env) = std::env::var("DEWASMIFY_BASH") {
        candidates.push(PathBuf::from(env));
    }
    candidates.push(PathBuf::from("bash"));
    candidates.push(PathBuf::from("/opt/homebrew/bin/bash"));
    candidates.push(PathBuf::from("/usr/local/bin/bash"));
    for candidate in candidates {
        let Ok(out) = Command::new(&candidate)
            .args(["-c", "echo ${BASH_VERSINFO[0]}"])
            .output()
        else {
            continue;
        };
        if String::from_utf8_lossy(&out.stdout).trim().parse::<u32>().unwrap_or(0) >= 5 {
            return Some(candidate);
        }
    }
    None
}

/// The Bash counterpart, cowsay only: QuickJS/SQLite run but take tens of
/// seconds under bash — fine for demos, too slow for the gate.
#[test]
fn apps_match_wasmtime_bash() {
    let Some(bash) = bash5() else {
        eprintln!("bash >= 5 not found; skipping");
        return;
    };
    if !tool_available("wasmtime") {
        eprintln!("wasmtime not found; skipping");
        return;
    }
    let wasm_path = cache_dir().join("cowsay.wasm");
    if !wasm_path.exists() {
        eprintln!("cowsay not cached (run examples/apps/fetch.sh); skipping");
        return;
    }
    let bytes = std::fs::read(&wasm_path).expect("read wasm");
    let module = dewasmify_core::build_module(&bytes).expect("build IR");
    let bash_src = BashBackend
        .generate(
            &module,
            &GenOptions {
                mode: Mode::Standalone,
                module_name: "cowsay".to_string(),
                runtime: RuntimeLinkage::Embedded,
                default_wasi: true,
            },
        )
        .expect("generate bash")
        .remove(0)
        .contents;
    let sh_path = std::env::temp_dir().join("dewasmify-app-cowsay.sh");
    std::fs::write(&sh_path, bash_src).unwrap();

    for (args, stdin) in
        [(&["Hello", "from", "Bash!"][..], ""), (&[][..], "moo via stdin\n")]
    {
        let (bash_out, bash_code) =
            run(Command::new(&bash).arg(&sh_path).args(args), stdin);
        let (wt_out, wt_code) =
            run(Command::new("wasmtime").arg(&wasm_path).args(args), stdin);
        assert_eq!(
            String::from_utf8_lossy(&bash_out),
            String::from_utf8_lossy(&wt_out),
            "cowsay {args:?}: stdout differs from wasmtime"
        );
        assert_eq!(bash_code, wt_code, "cowsay {args:?}: exit status differs");
        println!("cowsay {args:?} under bash: identical to wasmtime");
    }
}
