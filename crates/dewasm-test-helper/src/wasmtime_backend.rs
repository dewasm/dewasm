//! wasmtime as a [`BackendUnderTest`]: the snapshot-vs-wasmtime freshness checks run through the *same* shared app/gzip runners every real backend uses, rather than a hand-written per-case loop. wasmtime does not generate source — it runs the cached `.wasm` binary directly — so its `convert_app` returns the path to the exact cache binary the snapshots were captured from, and its `run`/`run_bytes` spawn the `xtask` WASI runner on it.
//!
//! That runner embeds the `wasmtime` crate pinned by `Cargo.lock` (`cargo xtask test-wasmtime-wasi`), so no `wasmtime` CLI has to be installed and the engine behind every snapshot is the same on every host. This crate keeps no `wasmtime` dependency of its own: it only spawns the binary, which the developer builds once with `cargo build -p xtask`.
//!
//! Public (not conditional behind the `wasmtime_test` feature, which only applies to the *tests* in `tests/apps_wasmtime.rs`) so that both that test file and `cargo xtask update-snapshots` can drive the same wasmtime-backed [`BackendUnderTest`] to (re)capture the execution snapshots.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dewasm_backend::{Backend, GenOptions, Mode, OutputFile};
use dewasm_core::ir;

use crate::backend::{run_command_bytes, BackendUnderTest};
use crate::pty::PtyCommand;

/// A [`Backend`] that only exists to satisfy `BackendUnderTest::backend()`. wasmtime runs the cached binary directly, so codegen is never reached; if anything routes into `generate()` it is a wiring bug, not a supported path.
struct NeverBackend;

impl Backend for NeverBackend {
    fn name(&self) -> &str {
        "wasmtime"
    }

    fn file_extension(&self) -> &str {
        "wasm"
    }

    fn generate(
        &self,
        _module: &ir::Module,
        _opts: &GenOptions,
    ) -> anyhow::Result<Vec<OutputFile>> {
        panic!("wasmtime runs the cache binary directly; generate() must never be called")
    }
}

/// The arguments `xtask test-wasmtime-wasi` takes for one run: the preopens, the environment, the wasm path, and the guest's `argv[1..]` (the runner sets `argv[0]` to the wasm file's base name itself). Every wasmtime-engine call site builds its command from this one function — the spawning [`Wasmtime`] here and the in-process engine `cargo xtask update-snapshots` drives — so the two cannot drift apart.
pub fn wasi_runner_argv(
    wasm: &str,
    args: &[&str],
    env: &[(&str, &str)],
    preopens: &[(&str, &Path)],
) -> Vec<String> {
    let mut argv = Vec::new();
    for (guest, host) in preopens {
        argv.push("--dir".to_string());
        argv.push(format!("{}::{}", host.display(), guest));
    }
    for (key, value) in env {
        argv.push("--env".to_string());
        argv.push(format!("{key}={value}"));
    }
    argv.push(wasm.to_string());
    argv.extend(args.iter().map(|arg| arg.to_string()));
    argv
}

/// The built `xtask` binary that carries the WASI runner. Resolved beside the running test binary (`<target>/<profile>/deps/<test>`), so it honors whatever target directory cargo used, `$CARGO_TARGET_DIR` included, and matches the profile the tests themselves were built with.
///
/// The suite never builds it: a missing binary fails loud with the command that produces it, the same way a missing interpreter or an unpopulated apps cache does.
pub fn wasi_runner_bin() -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let profile_dir = exe
        .parent()
        .and_then(Path::parent)
        .expect("a test binary sits in <target>/<profile>/deps/");
    let bin = profile_dir.join(format!("xtask{}", std::env::consts::EXE_SUFFIX));
    assert!(
        bin.is_file(),
        "the wasmtime engine runs wasm through the xtask runner, and {} does not exist — \
         build it once with `cargo build -p xtask` (see docs/testing.md)",
        bin.display()
    );
    bin
}

/// The command that runs `argv` (from [`wasi_runner_argv`]) through the runner.
fn runner_command(argv: &[String]) -> Command {
    let mut cmd = Command::new(wasi_runner_bin());
    cmd.arg("test-wasmtime-wasi").args(argv);
    cmd
}

/// The wasmtime engine wired into the shared app/gzip runners. `convert_app` hands back the cache-binary path (no codegen); `run_bytes` runs the `xtask` WASI runner on that exact binary.
pub struct Wasmtime;

impl BackendUnderTest for Wasmtime {
    fn name(&self) -> &'static str {
        "wasmtime"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &NeverBackend
    }

    /// Skip codegen entirely: write the exact bytes the shared runner read from the cache to a temp `.wasm` (keyed by content hash so identical apps share one file) and return that path, which the runner then feeds to `run`/`run_app_fs`. Writing the bytes rather than rebuilding the cache path from `name` keeps this independent of the conversion module name, which no longer always equals the cache stem (e.g. CRuby: cache `ruby.wasm`, class `Cruby`).
    fn convert_app(&self, bytes: &[u8], _mode: Mode, _name: &str) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        let path =
            std::env::temp_dir().join(format!("dewasm-wasmtime-{:016x}.wasm", hasher.finish()));
        if !path.exists() {
            std::fs::write(&path, bytes).expect("write temp wasm");
        }
        path.to_string_lossy().into_owned()
    }

    /// `source` is a cache-binary path (from `convert_app`), not generated code: run it with `args` and `stdin` piped in.
    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        assert!(
            Path::new(source).exists(),
            "{} not cached — run examples/apps/setup.sh (see docs/testing.md)",
            source
        );
        run_command_bytes(
            &mut runner_command(&wasi_runner_argv(source, args, &[], &[])),
            stdin,
        )
    }

    /// Run the filesystem app directly on the cache binary: `program` is the wasm path (from `convert_app`), so ignore the appended `glue` the default composes and instead run the binary under the case's host `preopens` and `env`. `args[0]` is dropped: the runner supplies `argv[0]` itself, exactly as `wasmtime run` does.
    fn run_app_fs(
        &self,
        program: &str,
        _glue: &str,
        args: &[&str],
        env: &[(&str, &str)],
        stdin: &[u8],
        preopens: &[(&str, &Path)],
    ) -> Output {
        run_command_bytes(
            &mut runner_command(&wasi_runner_argv(program, &args[1..], env, preopens)),
            stdin,
        )
    }

    /// Standalone `--dir` ground truth: `program` is the wasm path (from `convert_app`), and `--dir` is a host-runtime flag there, so the preopens go to the runner rather than to the guest. This is what the generated backends' own `--dir` parsing must reproduce.
    fn run_standalone_dir(
        &self,
        program: &str,
        preopens: &[(&str, &Path)],
        args: &[&str],
        stdin: &[u8],
    ) -> Output {
        run_command_bytes(
            &mut runner_command(&wasi_runner_argv(program, args, &[], preopens)),
            stdin,
        )
    }

    /// Drive the cached wasm binary directly under a pty: the runner inherits the pty slave as its stdio, so the guest reads a character device — the ground-truth interactive session the backends must match. The runner supplies `argv[0]`, so a bare no-args call is the interactive REPL invocation.
    fn pty_command(&self, source: &str, args: &[&str]) -> PtyCommand {
        let mut argv = vec!["test-wasmtime-wasi".to_string()];
        argv.extend(wasi_runner_argv(source, args, &[], &[]));
        PtyCommand {
            program: wasi_runner_bin(),
            args: argv,
            cwd: None,
        }
    }
}
