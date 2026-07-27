//! The base backend-under-test abstraction (ADR-27) and the process
//! plumbing every suite shares. `BackendUnderTest` is the layer that even a
//! pre-spec backend (the "cowsay first" bring-up path of ADR-24) can
//! implement: name it, hand back its `Backend`, and say how to run generated
//! output. Interpreted backends get `run` for free from `interpreter`;
//! compiled targets override `run` itself.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use dewasm_backend::{Backend, Mode};

use crate::pty::PtyCommand;

/// A target backend wired into the shared case tables and the spec harness.
/// The spec layer ([`crate::SpecBackend`]) extends this with the
/// script-phrasing surface; everything a backend needs to run app/e2e suites
/// is here.
pub trait BackendUnderTest: Sync {
    fn name(&self) -> &'static str;
    /// The backend itself. `+ Sync` so app conversion can run on a scoped
    /// worker thread (`convert_on_big_stack`).
    fn backend(&self) -> &'static (dyn Backend + Sync);

    /// Interpreter used by `run`'s default implementation. Per ADR-15 a
    /// missing interpreter must panic (fail loud), never skip. Compiled
    /// backends override `run` directly and need not implement this.
    fn interpreter(&self) -> PathBuf {
        unimplemented!(
            "an interpreted backend must implement interpreter(); \
             a compiled backend must override run()"
        )
    }

    /// Run generated `source` with `args` and `stdin`, returning the raw
    /// process `Output`. The default writes `source` to a temp file (keyed
    /// by pid + counter so parallel test threads and concurrent `cargo test`
    /// processes never collide) and execs `interpreter`.
    fn run(&self, source: &str, args: &[&str], stdin: &str) -> Output {
        self.run_bytes(source, args, stdin.as_bytes())
    }

    /// Like `run`, but with raw-byte `stdin` (and a raw-byte stdout in the
    /// returned `Output`). Needed by the binary-stdio app cases (minigzip),
    /// whose input and output are not valid UTF-8 and so cannot travel
    /// through the `&str`-typed `run`/`APP_CASES` path.
    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        run_script_bytes(
            &self.interpreter(),
            source,
            self.backend().file_extension(),
            args,
            stdin,
        )
    }

    /// Produce the runnable artifact for cached app `name` (bytes read from
    /// the app cache) that the app/gzip suites then hand to `run`/`run_bytes`.
    /// The default converts the wasm to backend source on a roomy stack
    /// (`convert_on_big_stack`); an engine-under-test that runs the wasm
    /// binary directly (e.g. wasmtime) overrides this to skip codegen and
    /// return a path to the cached binary instead.
    fn convert_app(&self, bytes: &[u8], mode: Mode, name: &str) -> String {
        crate::convert_on_big_stack(self.backend(), bytes, mode, name)
    }

    /// Prepare a *pty* run of standalone `source` with `args` (see
    /// [`crate::run_under_pty`]). Returns the command to spawn on a pty. The
    /// default — for interpreted backends — writes `source` to a temp script
    /// and runs `interpreter <script> <args...>`, exactly mirroring
    /// [`Self::run_bytes`]'s default but without capturing through pipes.
    /// Compiled backends (Go, Java) override this to build `source` first and
    /// return the resulting run command. A missing toolchain fails loud
    /// (ADR-15).
    fn pty_command(&self, source: &str, args: &[&str]) -> PtyCommand {
        let script = write_temp_script(source, self.backend().file_extension());
        let mut argv = vec![script.to_string_lossy().into_owned()];
        argv.extend(args.iter().map(|a| a.to_string()));
        PtyCommand {
            program: self.interpreter(),
            args: argv,
            cwd: None,
        }
    }

    /// Whether the `apps` suite should run its `heavy` cases (QuickJS,
    /// SQLite) for this backend even without `DEWASM_APPS_ALL`. Fast
    /// interpreters (Ruby) run them by default; slow ones (Bash softfloat)
    /// opt out unless the env var forces them on.
    fn run_heavy_apps(&self) -> bool {
        true
    }

    /// Backend-specific glue that instantiates library-mode class `class`
    /// with `args` (argv, argv0 included), `env`, and `preopens` (guest path
    /// -> host directory), runs `_start`, and swallows a clean guest
    /// `proc_exit`. Appended after the generated program by the default
    /// [`Self::run_app_fs`]. A backend wired into the filesystem-app suite
    /// (`run_fs_app_cases`) must implement this.
    fn app_glue(
        &self,
        class: &str,
        args: &[&str],
        env: &[(&str, &str)],
        preopens: &[(&str, &Path)],
    ) -> String {
        let _ = (class, args, env, preopens);
        unimplemented!("a filesystem-app backend must implement app_glue()")
    }

    /// Compose several wat modules that share the backend's linkage model into
    /// one runnable source (no driver appended). `modules` is `(wat filename in
    /// examples/wat, class/type name)` pairs. `shared_runtime` selects the
    /// linkage: `true` emits every module against ONE shared runtime (so an
    /// imported table can cross modules — the `shared_table` case); `false`
    /// emits independent self-contained (Embedded) runtimes that coexist
    /// (the `embedded_coexist` case). Only backends wired into
    /// `multi_module_e2e!` implement this, each using its own crate's
    /// multi-module API (the test-helper crate cannot depend on a concrete
    /// backend). See [`crate::run_multi_module_case`].
    fn compose_modules(&self, modules: &[(&str, &str)], shared_runtime: bool) -> String {
        let _ = (modules, shared_runtime);
        unimplemented!("a multi-module backend must implement compose_modules()")
    }

    /// Run library-mode `program` (from [`Self::convert_app`]) as a
    /// filesystem app: instantiate `class` with `args`/`env`/`preopens`, feed
    /// `stdin`, and return the process `Output`. The default appends
    /// [`Self::app_glue`] to `program` and runs the result through
    /// `run_bytes`; an engine-under-test that runs the wasm binary directly
    /// (wasmtime) overrides this to exec the binary with host preopens.
    fn run_app_fs(
        &self,
        program: &str,
        class: &str,
        args: &[&str],
        env: &[(&str, &str)],
        stdin: &[u8],
        preopens: &[(&str, &Path)],
    ) -> Output {
        let glue = self.app_glue(class, args, env, preopens);
        self.run_bytes(&format!("{program}\n{glue}"), &[], stdin)
    }
}

/// Spawn `cmd` with `stdin` piped in and both output streams captured,
/// returning the raw `Output`.
pub fn run_command(cmd: &mut Command, stdin: &str) -> Output {
    run_command_bytes(cmd, stdin.as_bytes())
}

/// Like `run_command`, but pipes raw bytes to stdin (binary-stdio cases).
pub fn run_command_bytes(cmd: &mut Command, stdin: &[u8]) -> Output {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    use std::io::Write as _;
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().expect("wait")
}

/// Write `script` to a temp file (extension `ext`) and run it under
/// `interpreter`, returning the raw `Output`. The pid + counter pair keeps
/// paths unique across both parallel test threads and concurrent `cargo
/// test` processes.
pub fn run_script(
    interpreter: &Path,
    script: &str,
    ext: &str,
    args: &[&str],
    stdin: &str,
) -> Output {
    run_script_bytes(interpreter, script, ext, args, stdin.as_bytes())
}

/// Like `run_script`, but pipes raw bytes to stdin (binary-stdio cases).
pub fn run_script_bytes(
    interpreter: &Path,
    script: &str,
    ext: &str,
    args: &[&str],
    stdin: &[u8],
) -> Output {
    let path = write_temp_script(script, ext);
    run_command_bytes(Command::new(interpreter).arg(&path).args(args), stdin)
}

/// Write `script` to a fresh temp file with extension `ext` and return its
/// path. The pid + counter pair keeps paths unique across both parallel test
/// threads and concurrent `cargo test` processes.
pub fn write_temp_script(script: &str, ext: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "dewasm-test-{}-{}.{ext}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::write(&path, script).unwrap();
    path
}
