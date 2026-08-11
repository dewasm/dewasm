//! The WASI command runner behind `cargo xtask run-wasi`.
//!
//! The flags mirror the `wasmtime run` subset the snapshot cases were captured with, so the runner is a drop-in replacement for a `wasmtime` CLI on `PATH`:
//!
//! * `argv[0]` is the wasm file's base name, the rest of the command line is guest `argv[1..]`;
//! * the guest environment holds exactly what `--env K=V` passes, nothing of the host's;
//! * `--dir HOST::GUEST` preopens `HOST` at guest path `GUEST` with full permissions;
//! * a guest `proc_exit` status becomes the process's exit status.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use wasmtime::{Engine, Linker, Module, Store};
use wasmtime_wasi::p1::{self, WasiP1Ctx};
use wasmtime_wasi::{DirPerms, FilePerms, I32Exit, WasiCtxBuilder};

/// One WASI command invocation: a wasm file plus the guest-visible environment `wasmtime run` would build for it.
pub struct WasiRun {
    pub wasm: PathBuf,
    /// Guest `argv[1..]`.
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// `(host, guest)` preopen pairs.
    pub dirs: Vec<(PathBuf, String)>,
}

impl WasiRun {
    /// Parse the `run-wasi` command line: option flags first, then the wasm path, then guest `argv[1..]` verbatim (a guest argument may repeat an option name).
    pub fn parse(mut argv: impl Iterator<Item = String>) -> Result<Self> {
        let mut env = Vec::new();
        let mut dirs = Vec::new();
        let wasm = loop {
            let Some(arg) = argv.next() else {
                bail!("run-wasi: missing the wasm file to run");
            };
            match arg.as_str() {
                "--dir" => {
                    let spec = argv
                        .next()
                        .context("run-wasi: --dir needs a HOST::GUEST argument")?;
                    let (host, guest) = match spec.split_once("::") {
                        Some((host, guest)) => (host.to_string(), guest.to_string()),
                        None => (spec.clone(), spec),
                    };
                    dirs.push((PathBuf::from(host), guest));
                }
                "--env" => {
                    let spec = argv
                        .next()
                        .context("run-wasi: --env needs a KEY=VALUE argument")?;
                    let (key, value) = spec
                        .split_once('=')
                        .with_context(|| format!("run-wasi: --env {spec:?} is not KEY=VALUE"))?;
                    env.push((key.to_string(), value.to_string()));
                }
                flag if flag.starts_with("--") => bail!("run-wasi: unknown option {flag}"),
                _ => break PathBuf::from(arg),
            }
        };
        Ok(Self {
            wasm,
            args: argv.collect(),
            env,
            dirs,
        })
    }

    /// Run with the host's stdin/stdout/stderr, returning the guest's exit status. The guest sees whatever those three descriptors are — a pty slave included, which is what makes the interactive-REPL transcript reproducible.
    pub fn run_inheriting_stdio(&self) -> Result<i32> {
        let mut builder = self.builder()?;
        builder.inherit_stdio();
        execute(builder.build_p1(), &self.wasm).map_err(from_wasmtime)
    }

    /// The guest-visible context minus the stdio wiring: argv, environment, preopens.
    fn builder(&self) -> Result<WasiCtxBuilder> {
        let argv0 = self
            .wasm
            .file_name()
            .with_context(|| format!("run-wasi: {} has no file name", self.wasm.display()))?
            .to_string_lossy()
            .into_owned();
        let mut builder = WasiCtxBuilder::new();
        builder.arg(argv0).args(&self.args);
        for (key, value) in &self.env {
            builder.env(key, value);
        }
        for (host, guest) in &self.dirs {
            builder
                .preopened_dir(host, guest, DirPerms::all(), FilePerms::all())
                .map_err(from_wasmtime)
                .with_context(|| format!("run-wasi: preopen {}", host.display()))?;
        }
        builder.allow_blocking_current_thread(true);
        Ok(builder)
    }
}

/// wasmtime carries its own error type; its debug rendering is what holds the context chain behind a trap.
fn from_wasmtime(err: wasmtime::Error) -> anyhow::Error {
    anyhow::anyhow!("{err:?}")
}

/// Instantiate `wasm` against WASI p1 and call `_start`, returning the guest's exit status (a `proc_exit` unwinds as [`I32Exit`]; a normal return is 0). Kept in `wasmtime::Result` so wasmtime's `?` composes; the callers lift it to anyhow.
fn execute(ctx: WasiP1Ctx, wasm: &Path) -> wasmtime::Result<i32> {
    let engine = Engine::default();
    let module = Module::from_file(&engine, wasm)?;
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    p1::add_to_linker_sync(&mut linker, |ctx| ctx)?;
    let mut store = Store::new(&engine, ctx);
    let instance = linker.instantiate(&mut store, &module)?;
    let start = instance.get_typed_func::<(), ()>(&mut store, "_start")?;
    match start.call(&mut store, ()) {
        Ok(()) => Ok(0),
        Err(trap) => match trap.downcast_ref::<I32Exit>() {
            Some(I32Exit(code)) => Ok(*code),
            None => Err(trap),
        },
    }
}

/// `cargo xtask run-wasi [--dir HOST::GUEST]... [--env K=V]... <wasm> [args...]`.
pub fn main(argv: impl Iterator<Item = String>) -> Result<()> {
    let code = WasiRun::parse(argv)?.run_inheriting_stdio()?;
    // Rust's stdout is line-buffered, so a guest's trailing partial line (a binary stream has none at all) would be lost across `exit`, which runs no destructors.
    std::io::stdout().flush()?;
    std::io::stderr().flush()?;
    std::process::exit(code);
}
