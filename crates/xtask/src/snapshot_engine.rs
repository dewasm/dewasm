//! The engine `update-snapshots` captures with: the [`BackendUnderTest`] surface the shared case runners expect, backed by [`crate::wasi_run`] in this process.
//! The freshness suite spawns the same runner as a child instead, so capture and comparison execute the identical wasm under the identical WASI configuration.
//!
//! The interactive-REPL transcript is the one case that still needs a child process (a pty session has to hand its slave to one), and there it runs this very binary as the runner.

use std::path::Path;
use std::process::{ExitStatus, Output};

use dewasm_backend::{Backend, Mode};
use dewasm_test_helper::{wasi_runner_argv, BackendUnderTest, PtyCommand, Wasmtime};

use crate::wasi_run::WasiRun;

/// The wasmtime engine behind every regenerated execution snapshot.
pub struct EmbeddedWasmtime;

impl BackendUnderTest for EmbeddedWasmtime {
    fn name(&self) -> &'static str {
        Wasmtime.name()
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        Wasmtime.backend()
    }

    fn convert_app(&self, bytes: &[u8], mode: Mode, name: &str) -> String {
        Wasmtime.convert_app(bytes, mode, name)
    }

    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        capture(wasi_runner_argv(source, args, &[], &[]), stdin)
    }

    fn run_app_fs(
        &self,
        program: &str,
        _glue: &str,
        args: &[&str],
        env: &[(&str, &str)],
        stdin: &[u8],
        preopens: &[(&str, &Path)],
    ) -> Output {
        capture(wasi_runner_argv(program, &args[1..], env, preopens), stdin)
    }

    fn run_standalone_dir(
        &self,
        program: &str,
        preopens: &[(&str, &Path)],
        args: &[&str],
        stdin: &[u8],
    ) -> Output {
        capture(wasi_runner_argv(program, args, &[], preopens), stdin)
    }

    fn pty_command(&self, source: &str, args: &[&str]) -> PtyCommand {
        let mut argv = vec!["test-wasmtime-wasi".to_string()];
        argv.extend(wasi_runner_argv(source, args, &[], &[]));
        PtyCommand {
            program: std::env::current_exe().expect("current_exe"),
            args: argv,
            cwd: None,
        }
    }
}

/// Run `argv` (as built for `xtask test-wasmtime-wasi`) in this process and shape the result like a child process's [`Output`], which is what the shared runners compare.
fn capture(argv: Vec<String>, stdin: &[u8]) -> Output {
    let run = WasiRun::parse(argv.into_iter()).expect("runner arguments");
    let captured = run
        .capture(stdin)
        .unwrap_or_else(|err| panic!("capture {} failed: {err:?}", run.wasm.display()));
    Output {
        status: exit_status(captured.code),
        stdout: captured.stdout,
        stderr: captured.stderr,
    }
}

/// An [`ExitStatus`] carrying `code`.
/// It has no portable constructor, so each platform builds the raw status its `code()` decodes.
#[cfg(unix)]
fn exit_status(code: i32) -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    ExitStatus::from_raw(code << 8)
}

#[cfg(windows)]
fn exit_status(code: i32) -> ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    ExitStatus::from_raw(code as u32)
}
