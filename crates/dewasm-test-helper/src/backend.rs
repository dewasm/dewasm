//! The base backend-under-test abstraction and the process plumbing every suite shares. `BackendUnderTest` is the layer that even a pre-spec backend (the "cowsay first" bring-up path) can implement: name it, hand back its `Backend`, and say how to run generated output. Interpreted backends get `run` for free from `interpreter`; compiled targets override `run` itself.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use dewasm_backend::{Backend, Mode};

use crate::pty::PtyCommand;

/// How a backend spells a module name: the two shapes the product grammars ask for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModuleNameStyle {
    /// `sqlite3-shell` -> `Sqlite3Shell`. Ruby needs the leading capital; Python, Perl and Java accept it and read conventionally with it. Go too: it derives *both* names from this one (package `sqlite3shell`, type `Sqlite3Shell`), and only a Pascal input yields a conventional pair.
    Pascal,
    /// `sqlite3-shell` -> `sqlite3_shell`. Bash only, whose prefix is the name lowercased: the underscores are what keep the prefix readable (`sqlite3_shell_`), and its glue constants already spell it that way.
    Snake,
}

/// Turn a kebab-case test-world name into a module name valid for `style`. Test and tooling names are kebab/stem case (`sqlite3-shell`, `ruby-packed`, `prog`); the product refuses to guess what such a name should become, so the test side converts explicitly. Total on the documented input domain `[a-z0-9]+(-[a-z0-9]+)*`; a name outside it panics rather than producing something the backend would then reject with a confusing message.
///
/// This lives in the test helper on purpose: it is test infrastructure, not a product surface. Nothing in `crates/dewasm-backend*` or the CLI performs any name transformation.
pub fn derive_module_name(style: ModuleNameStyle, kebab: &str) -> String {
    assert!(
        !kebab.is_empty()
            && kebab.split('-').all(|seg| {
                !seg.is_empty()
                    && seg
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            }),
        "test module names are kebab-case ([a-z0-9]+(-[a-z0-9]+)*), got {kebab:?}"
    );
    match style {
        ModuleNameStyle::Pascal => kebab
            .split('-')
            .map(|seg| {
                let mut chars = seg.chars();
                let first = chars.next().expect("no segment is empty");
                first.to_ascii_uppercase().to_string() + chars.as_str()
            })
            .collect(),
        ModuleNameStyle::Snake => kebab.replace('-', "_"),
    }
}

/// The style each backend's grammar wants, keyed by [`Backend::name`] so a backend gets the right one without restating it in every `BackendUnderTest` impl. Bash is the lone snake case; everything else — including Go, which builds its package *and* type name out of this one string — reads best from Pascal. Unknown names (the wasmtime stand-in, which never generates) take the majority style.
pub fn module_name_style(backend: &str) -> ModuleNameStyle {
    match backend {
        "bash" => ModuleNameStyle::Snake,
        _ => ModuleNameStyle::Pascal,
    }
}

/// A target backend wired into the shared case tables and the spec harness. The spec layer ([`crate::SpecBackend`]) extends this with the script-phrasing surface; everything a backend needs to run app/e2e suites is here.
pub trait BackendUnderTest: Sync {
    fn name(&self) -> &'static str;
    /// The backend itself. `+ Sync` so app conversion can run on a scoped worker thread (`convert_on_big_stack`).
    fn backend(&self) -> &'static (dyn Backend + Sync);

    /// The library-mode module name this backend should be handed for the kebab-case test name `kebab`. The default routes [`Backend::name`] through [`module_name_style`]; a backend overrides only if its grammar wants something else.
    fn module_name(&self, kebab: &str) -> String {
        derive_module_name(module_name_style(self.backend().name()), kebab)
    }

    /// Interpreter used by `run`'s default implementation. A missing interpreter must panic (fail loud), never skip. Compiled backends override `run` directly and need not implement this.
    fn interpreter(&self) -> PathBuf {
        unimplemented!(
            "an interpreted backend must implement interpreter(); \
             a compiled backend must override run()"
        )
    }

    /// Run generated `source` with `args` and `stdin`, returning the raw process `Output`. The default writes `source` to a temp file (keyed by pid + counter so parallel test threads and concurrent `cargo test` processes never collide) and execs `interpreter`.
    fn run(&self, source: &str, args: &[&str], stdin: &str) -> Output {
        self.run_bytes(source, args, stdin.as_bytes())
    }

    /// Like `run`, but with raw-byte `stdin` (and a raw-byte stdout in the returned `Output`). Needed by the binary-stdio app cases (minigzip), whose input and output are not valid UTF-8 and so cannot travel through the `&str`-typed `run`/`AppCase` path.
    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        run_script_bytes(
            &self.interpreter(),
            source,
            self.backend().file_extension(),
            args,
            stdin,
        )
    }

    /// Produce the runnable artifact for cached app `name` (bytes read from the app cache) that the app/gzip suites then hand to `run`/`run_bytes`. The default converts the wasm to backend source on a roomy stack (`convert_on_big_stack`); an engine-under-test that runs the wasm binary directly (e.g. wasmtime) overrides this to skip codegen and return a path to the cached binary instead.
    fn convert_app(&self, bytes: &[u8], mode: Mode, name: &str) -> String {
        crate::convert_on_big_stack(self.backend(), bytes, mode, name)
    }

    /// Prepare a *pty* run of standalone `source` with `args` (see [`crate::run_under_pty`]). Returns the command to spawn on a pty. The default — for interpreted backends — writes `source` to a temp script and runs `interpreter <script> <args...>`, exactly mirroring [`Self::run_bytes`]'s default but without capturing through pipes. Compiled backends (Go, Java) override this to build `source` first and return the resulting run command. A missing toolchain fails loud.
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

    /// Write each of `modules` into `dir` as its own file, the way a user of this backend would ship several converted artifacts, and return the *driver preamble*: the source that loads those files in the host language. The runner appends the case glue to that preamble and runs the result from `dir` through [`Self::run_in_dir`]. `modules` is `(wat filename in examples/wat, class/type name)` pairs. `shared_runtime` selects the linkage: `true` emits every module against ONE shared runtime, written out as its own file too (so an imported table can cross modules — the `shared_table` case); `false` converts each module independently in library mode, each carrying its own runtime (the `embedded_coexist` case). Only backends wired into the multi-module macros implement this, each using its own crate's API (the test-helper crate cannot depend on a concrete backend). See [`crate::run_multi_module_case`].
    fn compose_modules(
        &self,
        dir: &Path,
        modules: &[(&str, &str)],
        shared_runtime: bool,
    ) -> String {
        let _ = (dir, modules, shared_runtime);
        unimplemented!("a multi-module backend must implement compose_modules()")
    }

    /// Run the multi-module `driver` (the preamble from [`Self::compose_modules`] plus the case glue) against the module files sitting in `dir`. The default — every interpreted backend — writes the driver as `dir/driver.<ext>` and execs the interpreter on it with `dir` as the working directory, so the language's own relative loading (`require_relative`, `sys.path[0]`, `source`) finds the files beside it. The compiled backends override this to build the directory instead.
    fn run_in_dir(&self, dir: &Path, driver: &str) -> Output {
        let path = dir.join(format!("driver.{}", self.backend().file_extension()));
        std::fs::write(&path, driver).unwrap();
        run_command(
            Command::new(self.interpreter()).arg(&path).current_dir(dir),
            "",
        )
    }

    /// Run library-mode `program` (from [`Self::convert_app`]) as a filesystem app: append the already-filled instantiation `glue` (a named backend const with its `{scratch}`/`{cache}` placeholders resolved by the runner), feed `stdin`, and return the process `Output`. The default runs `program` + `glue` through `run_bytes`, so the static `args`/`env`/`preopens` are written literally inside `glue` and ignored here; an engine-under-test that runs the wasm binary directly (wasmtime) overrides this to exec the binary (`program` is the wasm path) with those host `args`/`env`/`preopens` and ignores `glue`.
    fn run_app_fs(
        &self,
        program: &str,
        glue: &str,
        args: &[&str],
        env: &[(&str, &str)],
        stdin: &[u8],
        preopens: &[(&str, &Path)],
    ) -> Output {
        let _ = (args, env, preopens);
        self.run_bytes(&format!("{program}\n{glue}"), &[], stdin)
    }

    /// Run a *standalone*-mode `program` through the standalone runtime interface: the generated main parses a leading run of `--dir HOST::GUEST` flags itself, then hands the rest to the guest as `argv[1..]`. The default (every generated backend) execs the program with those `--dir` flags followed by `args` — exactly what a user types — via [`Self::run_bytes`] (compiled backends build first through their `run_bytes` override). wasmtime overrides this: `--dir` is a *host* runtime flag there, so it runs `wasmtime run --dir HOST::GUEST... <wasm> args` instead. Same case feeds both, mirroring [`Self::run_app_fs`].
    fn run_standalone_dir(
        &self,
        program: &str,
        preopens: &[(&str, &Path)],
        args: &[&str],
        stdin: &[u8],
    ) -> Output {
        let mut flags: Vec<String> = Vec::new();
        for (guest, host) in preopens {
            flags.push("--dir".to_string());
            flags.push(format!("{}::{}", host.display(), guest));
        }
        flags.extend(args.iter().map(|a| a.to_string()));
        let flag_refs: Vec<&str> = flags.iter().map(String::as_str).collect();
        self.run_bytes(program, &flag_refs, stdin)
    }

    /// Run a *standalone*-mode `program` through the standalone interface like [`Self::run_standalone_dir`], but additionally set `env` on the child process and return the full [`Output`] — the surface the WASI-testsuite harness needs ([`crate::wasi_testsuite`]), which [`Self::run_standalone_dir`] cannot give (it inherits the parent env and its callers discard the exit code). `dirs` are `(guest, host)` preopens rendered as the leading `--dir HOST::GUEST` flags the generated `main` parses; `args` follow as guest `argv[1..]`.
    ///
    /// The default reuses the backend's own launch recipe ([`Self::pty_command`] — interpreter + temp script for the interpreted backends, a freshly built binary for the compiled ones) so no per-backend override is needed, then drives it through pipes with exactly `env` as the child environment and `stdin` fed.
    fn run_standalone_wasi(
        &self,
        program: &str,
        dirs: &[(&str, &Path)],
        args: &[&str],
        env: &[(&str, &str)],
        stdin: &[u8],
    ) -> Output {
        let mut argv: Vec<String> = Vec::new();
        for (guest, host) in dirs {
            argv.push("--dir".to_string());
            argv.push(format!("{}::{}", host.display(), guest));
        }
        argv.extend(args.iter().map(|a| a.to_string()));
        let argv_refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        let cmd = self.pty_command(program, &argv_refs);
        // The guest must observe exactly the manifest env (upstream's wasmtime adapter passes only `--env k=v`, isolating the guest from the host environment); dewasm's standalone programs pass the whole process env through, so the isolation has to happen here instead. With the child PATH gone, exec would fall back to the OS default path and pick the *system* interpreter (ruby 2.6 / bash 3.2 on macOS), so a bare program name is resolved against the parent PATH first.
        let resolved = resolve_in_parent_path(&cmd.program);
        let mut command = Command::new(resolved);
        command.args(&cmd.args);
        if let Some(cwd) = &cmd.cwd {
            command.current_dir(cwd);
        }
        command.env_clear();
        for (k, v) in env {
            command.env(k, v);
        }
        run_command_bytes(&mut command, stdin)
    }
}

/// Resolve a bare program name to an absolute path using the *parent* process's PATH, so an `env_clear`ed child still runs the interpreter the test environment selected. Names already containing a separator pass through untouched.
fn resolve_in_parent_path(program: &Path) -> PathBuf {
    if program.components().count() > 1 {
        return program.to_path_buf();
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(program);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    program.to_path_buf()
}

/// Spawn `cmd` with `stdin` piped in and both output streams captured, returning the raw `Output`.
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

/// Write `script` to a temp file (extension `ext`) and run it under `interpreter`, returning the raw `Output`.
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

/// Write `script` to a fresh temp file with extension `ext` and return its path. The pid + counter pair keeps paths unique across both parallel test threads and concurrent `cargo test` processes.
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

#[cfg(test)]
mod tests {
    use super::{derive_module_name, module_name_style, ModuleNameStyle};

    /// The derivation reproduces exactly the names the deleted per-backend sanitizers produced for these inputs, which is what lets the glue consts stay untouched.
    #[test]
    fn kebab_derivation_matches_the_names_the_glue_consts_spell() {
        for (kebab, pascal, snake) in [
            ("add", "Add", "add"),
            ("prog", "Prog", "prog"),
            ("sqlite3-shell", "Sqlite3Shell", "sqlite3_shell"),
            ("sqlite3-binding", "Sqlite3Binding", "sqlite3_binding"),
            ("ruby-packed", "RubyPacked", "ruby_packed"),
            ("libsqlite3", "Libsqlite3", "libsqlite3"),
        ] {
            assert_eq!(derive_module_name(ModuleNameStyle::Pascal, kebab), pascal);
            assert_eq!(derive_module_name(ModuleNameStyle::Snake, kebab), snake);
        }
    }

    #[test]
    fn styles_are_keyed_by_backend_name() {
        for backend in ["ruby", "python", "perl", "java", "go"] {
            assert_eq!(module_name_style(backend), ModuleNameStyle::Pascal);
        }
        assert_eq!(module_name_style("bash"), ModuleNameStyle::Snake);
    }

    /// A name outside the documented kebab domain is a bug in the case table, not something to reshape.
    #[test]
    #[should_panic(expected = "kebab-case")]
    fn a_non_kebab_name_panics() {
        derive_module_name(ModuleNameStyle::Pascal, "Sqlite3Shell");
    }
}
