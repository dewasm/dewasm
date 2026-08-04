//! The runner matrix: every execution environment a workload is measured on, how to tell whether it is usable on this host, and how to turn a `.wasm` into something runnable on it.
//!
//! Four families:
//!
//! * **wasmtime** — the AOT ceiling and the correctness reference.
//! * **native runtimes** — wasmer, wasmedge, wazero, wasm3: consume the `.wasm` directly; [`Native`] holds the per-runtime command-line spelling. Cross-checked like everything else.
//! * **dewasm-\*** — generated source on the host language. Codegen goes through the [`Backend`] trait, never the CLI binary. Go and Java build first, mirroring their e2e suites (`go run` swallows the guest exit code; generated Java requires the file to be named `Main.java`).
//! * **pywasm / wardite** — third-party interpreters, driven via `benchmarks/drivers/`, provisioned by `benchmarks/setup.sh`.
//!
//! Availability is a `Result<(), String>` whose error is the setup instruction that would fix it (ADR-15 applied to tools: the harness keeps going, the gap is named in both outputs).

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use anyhow::{bail, Context, Result};
use dewasm_backend::{Backend, GenOptions, Mode, RuntimeLinkage};

use crate::bench::{bench_cache_dir, display_path, drivers_dir};

/// A launchable recipe: `program args... <workload args...>`, with `env` overlaid on the inherited environment.
#[derive(Clone)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Which dewasm backend a `dewasm-*` runner generates with, and on what.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// CRuby with the given JIT flag (`--disable-yjit`, `--yjit`, `--zjit`).
    Ruby(&'static str),
    /// The Python backend's output on CPython.
    Python,
    /// The *same* Python output on PyPy — a JIT'd Python, and by far the fastest interpreted-language runner we have. Not installed by `benchmarks/setup.sh`; it has to already be on the host.
    PyPy,
    Perl,
    Bash,
    Go,
    Java,
}

/// Which third-party wasm interpreter a driver runner drives, and on which host interpreter.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Driver {
    /// `benchmarks/drivers/pywasm.py` on the `benchmarks/setup.sh` venv.
    PywasmCPython,
    /// The same driver on a host PyPy, which needs `pywasm` importable there.
    PywasmPyPy,
    /// `benchmarks/drivers/wardite.rb` with the given ruby JIT flag, against the `GEM_HOME` under `benchmarks/cache/`.
    Wardite(&'static str),
}

/// A third-party wasm runtime that consumes the `.wasm` directly, described entirely by how its command line is spelled.
///
/// The three fields are the whole difference between these runtimes: `wasmer` wants `run <module> -- <guest args>`, `wazero` wants `run <module> <guest args>`, and `wasmedge` and `wasm3` take the module as their first argument with no subcommand at all. The version flag differs too — `wazero` answers `version`, not `--version`.
#[derive(Clone, Copy)]
pub struct Native {
    /// Executable name, overridable through `DEWASM_<NAME uppercased>` like [`wasmtime_bin`].
    bin: &'static str,
    /// Argv between the executable and the module path.
    lead: &'static [&'static str],
    /// Argv between the module path and the guest's own arguments.
    separator: &'static [&'static str],
    /// How this runtime is asked for its version.
    version_args: &'static [&'static str],
}

const WASMER: Native = Native {
    bin: "wasmer",
    lead: &["run"],
    // Without `--`, wasmer parses the guest's arguments as its own.
    separator: &["--"],
    version_args: &["--version"],
};

/// Measured at its default, which is the interpreter — each runtime runs as shipped. `--run-mode jit` was tried: 13x faster on `wat/i32_alu`, but it segfaults on `sqlite3-shell.wasm` (exit 139, reproducible; also logs to stdout, needing `--log-tier=off`). A JIT column, if ever wanted, would be a separately labeled runner like `dewasm-ruby-yjit`, not a substitution.
const WASMEDGE: Native = Native {
    bin: "wasmedge",
    lead: &[],
    separator: &[],
    version_args: &["--version"],
};

const WAZERO: Native = Native {
    bin: "wazero",
    lead: &["run"],
    separator: &[],
    version_args: &["version"],
};

const WASM3: Native = Native {
    bin: "wasm3",
    lead: &[],
    separator: &[],
    version_args: &["--version"],
};

pub enum Kind {
    Wasmtime,
    Native(Native),
    Dewasm(Target),
    Driver(Driver),
}

pub struct Runner {
    pub label: &'static str,
    pub kind: Kind,
}

/// The full matrix, in report order: the ceiling first, then the other native runtimes beside it, then dewasm's backends, then the third-party interpreters we are actually competing with.
pub fn runners() -> Vec<Runner> {
    let r = |label, kind| Runner { label, kind };
    vec![
        r("wasmtime", Kind::Wasmtime),
        r("wasmer", Kind::Native(WASMER)),
        r("wasmedge", Kind::Native(WASMEDGE)),
        r("wazero", Kind::Native(WAZERO)),
        r("wasm3", Kind::Native(WASM3)),
        r("dewasm-ruby", Kind::Dewasm(Target::Ruby("--disable-yjit"))),
        r("dewasm-ruby-yjit", Kind::Dewasm(Target::Ruby("--yjit"))),
        r("dewasm-ruby-zjit", Kind::Dewasm(Target::Ruby("--zjit"))),
        r("dewasm-python", Kind::Dewasm(Target::Python)),
        r("dewasm-pypy", Kind::Dewasm(Target::PyPy)),
        r("dewasm-perl", Kind::Dewasm(Target::Perl)),
        r("dewasm-go", Kind::Dewasm(Target::Go)),
        r("dewasm-java", Kind::Dewasm(Target::Java)),
        r("dewasm-bash", Kind::Dewasm(Target::Bash)),
        r("pywasm-cpython", Kind::Driver(Driver::PywasmCPython)),
        r("pywasm-pypy", Kind::Driver(Driver::PywasmPyPy)),
        r("wardite", Kind::Driver(Driver::Wardite("--disable-yjit"))),
        r("wardite-yjit", Kind::Driver(Driver::Wardite("--yjit"))),
    ]
}

impl Runner {
    /// `Ok(())` when this runner can run here; otherwise the setup instruction that would make it available. Never silently downgraded — the caller reports the reason in both outputs.
    pub fn availability(&self) -> Result<(), String> {
        match &self.kind {
            Kind::Wasmtime => wasmtime_bin()
                .map(|_| ())
                .ok_or_else(|| "wasmtime not found on PATH — see docs/testing.md".to_string()),
            Kind::Native(native) => native.bin_path().map(|_| ()).ok_or_else(|| {
                format!(
                    "{} not found on PATH (or ${})",
                    native.bin,
                    native.env_var()
                )
            }),
            Kind::Dewasm(target) => target.availability(),
            Kind::Driver(driver) => driver.availability(),
        }
    }

    /// A version string captured by *executing* the runtime, so the result file records what actually ran rather than what was pinned.
    pub fn version(&self) -> Option<String> {
        match &self.kind {
            Kind::Wasmtime => capture_version(&wasmtime_bin()?, &["--version"]),
            Kind::Native(native) => capture_version(&native.bin_path()?, native.version_args),
            Kind::Dewasm(target) => target.version(),
            Kind::Driver(driver) => driver.version(),
        }
    }
}

impl Native {
    /// The environment variable that overrides this runtime's executable, matching `DEWASM_WASMTIME`.
    fn env_var(&self) -> String {
        format!("DEWASM_{}", self.bin.to_uppercase())
    }

    /// The executable, if it runs at all here. Probed with the version command, which is the one invocation every one of these accepts without a module.
    fn bin_path(&self) -> Option<PathBuf> {
        let candidate =
            std::env::var_os(self.env_var()).map_or_else(|| PathBuf::from(self.bin), PathBuf::from);
        probe(&candidate, self.version_args).then_some(candidate)
    }

    /// The argv prefix that runs `wasm` on this runtime, up to but excluding the guest's own arguments.
    fn launch(&self, wasm: &Path) -> Result<Launch> {
        let bin = self
            .bin_path()
            .with_context(|| format!("{} not found on PATH", self.bin))?;
        let mut args: Vec<String> = self.lead.iter().copied().map(String::from).collect();
        args.push(path_arg(wasm));
        args.extend(self.separator.iter().copied().map(String::from));
        Ok(Launch {
            program: bin,
            args,
            env: Vec::new(),
        })
    }
}

impl Target {
    fn availability(&self) -> Result<(), String> {
        match self {
            Target::Ruby(flag) => {
                let ruby = dewasm_backend_ruby::find_ruby().ok_or_else(|| {
                    "ruby >= 3.4 not found on PATH — see docs/testing.md".to_string()
                })?;
                ruby_jit_available(&ruby, flag)
            }
            Target::Python => dewasm_backend_python::find_python()
                .map(|_| ())
                .ok_or_else(|| {
                    "python3 >= 3.9 not found on PATH — see docs/testing.md".to_string()
                }),
            Target::PyPy => pypy_bin().map(|_| ()).ok_or_else(|| {
                "pypy3 not found on PATH (or $DEWASM_PYPY); benchmarks/setup.sh does not install it"
                    .to_string()
            }),
            Target::Perl => dewasm_backend_perl::find_perl()
                .map(|_| ())
                .ok_or_else(|| "perl >= 5.26 not found on PATH — see docs/testing.md".to_string()),
            Target::Bash => dewasm_backend_bash::find_bash5()
                .map(|_| ())
                .ok_or_else(|| "bash >= 5 not found on PATH — see docs/testing.md".to_string()),
            Target::Go => dewasm_backend_go::find_go()
                .map(|_| ())
                .ok_or_else(|| "go toolchain not found on PATH — see docs/testing.md".to_string()),
            Target::Java => {
                dewasm_backend_java::find_java().ok_or_else(|| {
                    "java not found on PATH (or $DEWASM_JAVA) — see docs/testing.md".to_string()
                })?;
                dewasm_backend_java::find_javac()
                    .map(|_| ())
                    .ok_or_else(|| {
                        "javac not found on PATH (or $DEWASM_JAVAC) — see docs/testing.md"
                            .to_string()
                    })
            }
        }
    }

    fn version(&self) -> Option<String> {
        match self {
            Target::Ruby(_) => capture_version(&dewasm_backend_ruby::find_ruby()?, &["-v"]),
            Target::Python => capture_version(&dewasm_backend_python::find_python()?, &["-VV"]),
            Target::PyPy => capture_version(&pypy_bin()?, &["-VV"]),
            Target::Perl => {
                capture_version(&dewasm_backend_perl::find_perl()?, &["-e", "print $^V"])
            }
            Target::Bash => capture_version(
                &dewasm_backend_bash::find_bash5()?,
                &["-c", "echo $BASH_VERSION"],
            ),
            Target::Go => capture_version(&dewasm_backend_go::find_go()?, &["version"]),
            // `java -version` writes to stderr on every JDK that predates the `--version` spelling; `capture_version` reads both streams for exactly this reason.
            Target::Java => capture_version(&dewasm_backend_java::find_java()?, &["-version"]),
        }
    }

    /// The dewasm backend behind this target. Ruby's three JIT modes and Python/PyPy share one backend, so they also share one generated artifact.
    fn backend(&self) -> &'static (dyn Backend + Sync) {
        match self {
            Target::Ruby(_) => &dewasm_backend_ruby::RubyBackend,
            Target::Python | Target::PyPy => &dewasm_backend_python::PythonBackend,
            Target::Perl => &dewasm_backend_perl::PerlBackend,
            Target::Bash => &dewasm_backend_bash::BashBackend,
            Target::Go => &dewasm_backend_go::GoBackend,
            Target::Java => &dewasm_backend_java::JavaBackend,
        }
    }
}

impl Driver {
    fn availability(&self) -> Result<(), String> {
        let script = self.script();
        if !script.is_file() {
            return Err(format!(
                "{} missing — it ships with the repo",
                display_path(&script)
            ));
        }
        match self {
            Driver::PywasmCPython => {
                let python = venv_python().ok_or_else(|| {
                    "benchmarks/cache/venv missing — run benchmarks/setup.sh".to_string()
                })?;
                probe(&python, &["-c", "import pywasm"])
                    .then_some(())
                    .ok_or_else(|| {
                        "pywasm not importable in benchmarks/cache/venv — run benchmarks/setup.sh"
                            .to_string()
                    })
            }
            Driver::PywasmPyPy => {
                let pypy = pypy_bin().ok_or_else(|| {
                    "pypy3 not found on PATH (or $DEWASM_PYPY); benchmarks/setup.sh does not install it"
                        .to_string()
                })?;
                probe(&pypy, &["-c", "import pywasm"]).then_some(()).ok_or_else(|| {
                    "pywasm not importable under pypy3 — install it there (benchmarks/setup.sh only provisions the CPython venv)"
                        .to_string()
                })
            }
            Driver::Wardite(flag) => {
                let ruby = dewasm_backend_ruby::find_ruby().ok_or_else(|| {
                    "ruby >= 3.4 not found on PATH — see docs/testing.md".to_string()
                })?;
                ruby_jit_available(&ruby, flag)?;
                let gem_home = wardite_gem_home().ok_or_else(|| {
                    "no GEM_HOME with wardite under benchmarks/cache/ — run benchmarks/setup.sh"
                        .to_string()
                })?;
                let ok = Command::new(&ruby)
                    .args([flag, "-e", "require 'wardite'"])
                    .env("GEM_HOME", &gem_home)
                    .env("GEM_PATH", &gem_home)
                    .output()
                    .map(|out| out.status.success())
                    .unwrap_or(false);
                ok.then_some(()).ok_or_else(|| {
                    format!(
                        "wardite not loadable from {} — run benchmarks/setup.sh",
                        display_path(&gem_home)
                    )
                })
            }
        }
    }

    fn version(&self) -> Option<String> {
        match self {
            Driver::PywasmCPython => pywasm_version(&venv_python()?, &[]),
            Driver::PywasmPyPy => pywasm_version(&pypy_bin()?, &[]),
            Driver::Wardite(flag) => {
                let ruby = dewasm_backend_ruby::find_ruby()?;
                let gem_home = wardite_gem_home()?;
                let out = Command::new(ruby)
                    .args([
                        flag,
                        "-e",
                        "require 'wardite'; print Gem.loaded_specs['wardite']&.version",
                    ])
                    .env("GEM_HOME", &gem_home)
                    .env("GEM_PATH", &gem_home)
                    .output()
                    .ok()?;
                let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
                (!v.is_empty()).then(|| format!("wardite {v}"))
            }
        }
    }

    /// The driver script this runner executes.
    fn script(&self) -> PathBuf {
        match self {
            Driver::PywasmCPython | Driver::PywasmPyPy => drivers_dir().join("pywasm.py"),
            Driver::Wardite(_) => drivers_dir().join("wardite.rb"),
        }
    }
}

/// Prepares (and caches) the runnable artifact for each `(module, backend)` pair.
///
/// Two tiers of caching. The in-process map (keyed by wasm bytes + backend name) lets the three Ruby JIT modes convert once per run — safe, because one process holds one backend build. The `/tmp` cache is keyed by the hash of the **generated source**, never the input wasm: a wasm-keyed cache once served artifacts generated by an older backend build and silently measured the wrong lowering across three separate comparison runs. Conversion is cheap enough to redo every run; only the expensive `go build`/`javac` step is worth remembering, and the source hash invalidates it exactly when the backend's output changes.
#[derive(Default)]
pub struct Workshop {
    artifacts: HashMap<(u64, &'static str), Artifact>,
}

/// What a backend's output is once it is ready to run.
#[derive(Clone)]
enum Artifact {
    /// An interpreted backend: a generated script on disk.
    Script(PathBuf),
    /// Go: a built executable.
    Binary(PathBuf),
    /// Java: a class directory holding `Main.class`.
    ClassDir(PathBuf),
}

impl Workshop {
    /// The launch recipe for `runner` on `wasm`. For a dewasm runner this converts (and, for Go/Java, compiles) on first use.
    pub fn launch(&mut self, runner: &Runner, wasm: &Path, module_name: &str) -> Result<Launch> {
        match &runner.kind {
            Kind::Wasmtime => Ok(Launch {
                program: wasmtime_bin().context("wasmtime not found on PATH")?,
                args: vec!["run".to_string(), path_arg(wasm)],
                env: Vec::new(),
            }),
            Kind::Native(native) => native.launch(wasm),
            Kind::Dewasm(target) => self.dewasm_launch(*target, wasm, module_name),
            Kind::Driver(driver) => driver_launch(*driver, wasm),
        }
    }

    fn dewasm_launch(&mut self, target: Target, wasm: &Path, module_name: &str) -> Result<Launch> {
        let bytes =
            std::fs::read(wasm).with_context(|| format!("failed to read {}", wasm.display()))?;
        let backend = target.backend();
        let key = (hash_bytes(&bytes), backend.name());
        let artifact = match self.artifacts.get(&key) {
            Some(cached) => cached.clone(),
            None => {
                let built = build_artifact(target, &bytes, module_name)?;
                self.artifacts.insert(key, built.clone());
                built
            }
        };
        Ok(match (target, artifact) {
            (Target::Ruby(flag), Artifact::Script(path)) => Launch {
                program: dewasm_backend_ruby::find_ruby().context("ruby not found")?,
                args: vec![flag.to_string(), path_arg(&path)],
                env: Vec::new(),
            },
            (Target::Python, Artifact::Script(path)) => Launch {
                program: dewasm_backend_python::find_python().context("python3 not found")?,
                args: vec![path_arg(&path)],
                env: Vec::new(),
            },
            (Target::PyPy, Artifact::Script(path)) => Launch {
                program: pypy_bin().context("pypy3 not found")?,
                args: vec![path_arg(&path)],
                env: Vec::new(),
            },
            (Target::Perl, Artifact::Script(path)) => Launch {
                program: dewasm_backend_perl::find_perl().context("perl not found")?,
                args: vec![path_arg(&path)],
                env: Vec::new(),
            },
            (Target::Bash, Artifact::Script(path)) => Launch {
                program: dewasm_backend_bash::find_bash5().context("bash >= 5 not found")?,
                args: vec![path_arg(&path)],
                env: Vec::new(),
            },
            // `go run` prints "exit status N" and exits 1 instead of propagating the guest's exit code, so the built binary is executed directly (same reason the Go e2e suite does).
            (Target::Go, Artifact::Binary(bin)) => Launch {
                program: bin,
                args: Vec::new(),
                env: Vec::new(),
            },
            (Target::Java, Artifact::ClassDir(dir)) => Launch {
                program: dewasm_backend_java::find_java().context("java not found")?,
                args: vec!["-cp".to_string(), path_arg(&dir), "Main".to_string()],
                env: Vec::new(),
            },
            _ => bail!("internal: artifact kind does not match the target"),
        })
    }
}

/// Convert `bytes` with `target`'s backend and get it into runnable shape, reusing the content-addressed `/tmp` cache when a previous run already produced it.
fn build_artifact(target: Target, bytes: &[u8], module_name: &str) -> Result<Artifact> {
    let backend = target.backend();
    let cache = bench_tmp_dir()?;
    // Convert unconditionally and key the disk cache by what came out: the artifact must reflect *this* build's codegen, and only the compile step after it is expensive enough to be worth remembering.
    let source = convert(backend, bytes, module_name)?;
    let stem = format!("{}-{:016x}", backend.name(), hash_bytes(source.as_bytes()));

    match target {
        Target::Go => {
            let bin = cache.join(format!("{stem}.bin"));
            if !bin.is_file() {
                let src = cache.join(format!("{stem}.go"));
                write_if_absent(&src, &source)?;
                let go = dewasm_backend_go::find_go()
                    .context("go toolchain not found on PATH (or $DEWASM_GO)")?;
                let tmp = cache.join(format!("{stem}.bin.tmp"));
                let out = Command::new(go)
                    .arg("build")
                    .arg("-o")
                    .arg(&tmp)
                    .arg(&src)
                    .output()
                    .context("spawn go build")?;
                if !out.status.success() {
                    bail!("go build failed:\n{}", String::from_utf8_lossy(&out.stderr));
                }
                std::fs::rename(&tmp, &bin).context("install the built go binary")?;
            }
            Ok(Artifact::Binary(bin))
        }
        Target::Java => {
            let dir = cache.join(&stem);
            if !dir.join("Main.class").is_file() {
                // The generated Java always declares `public class Main`, so the source file must literally be `Main.java` whatever the module name is.
                let tmp = cache.join(format!("{stem}.build"));
                let _ = std::fs::remove_dir_all(&tmp);
                std::fs::create_dir_all(&tmp)?;
                let src = tmp.join("Main.java");
                std::fs::write(&src, &source)?;
                let out = dewasm_backend_java::javac_command()
                    .arg("-d")
                    .arg(&tmp)
                    .arg(&src)
                    .output()
                    .context("spawn javac")?;
                if !out.status.success() {
                    bail!("javac failed:\n{}", String::from_utf8_lossy(&out.stderr));
                }
                let _ = std::fs::remove_dir_all(&dir);
                std::fs::rename(&tmp, &dir).context("install the compiled class dir")?;
            }
            Ok(Artifact::ClassDir(dir))
        }
        _ => {
            let path = cache.join(format!("{stem}.{}", backend.file_extension()));
            write_if_absent(&path, &source)?;
            Ok(Artifact::Script(path))
        }
    }
}

/// Convert `bytes` to standalone source with `backend`, on a 64 MiB stack.
///
/// Codegen recurses with the IR's control-flow nesting, and a SQLite-class module's deepest functions overflow the default stack — the same reason `dewasm_test_helper::convert_on_big_stack` exists. That helper panics on a codegen error, which here would take down the whole suite instead of marking one cell failed, so this mirrors it over `Result`.
fn convert(backend: &(dyn Backend + Sync), bytes: &[u8], module_name: &str) -> Result<String> {
    let source = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn_scoped(scope, || -> Result<Vec<u8>> {
                let module = dewasm_core::build_module(bytes)?;
                Ok(backend
                    .generate(
                        &module,
                        &GenOptions {
                            mode: Mode::Standalone,
                            module_name: module_name.to_string(),
                            runtime: RuntimeLinkage::Embedded,
                            default_wasi: true,
                            data_file: None,
                        },
                    )?
                    .remove(0)
                    .contents)
            })
            .context("spawn the codegen thread")?
            .join()
            .map_err(|_| anyhow::anyhow!("the codegen thread panicked"))?
    })?;
    String::from_utf8(source).context("generated source is not valid UTF-8")
}

/// Write `contents` to `path` via a unique temp file plus a rename, so two concurrent suites never let one read a half-written script.
fn write_if_absent(path: &Path, contents: &str) -> Result<()> {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, contents).with_context(|| format!("failed to write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("failed to install {}", path.display()))
}

/// The launch recipe for a third-party interpreter: its host interpreter, the driver script, and the module path. Guest args are appended by the caller, matching the drivers' `<module.wasm> [guest-args...]` contract.
fn driver_launch(driver: Driver, wasm: &Path) -> Result<Launch> {
    let script = path_arg(&driver.script());
    Ok(match driver {
        Driver::PywasmCPython => Launch {
            program: venv_python().context("benchmarks/cache/venv missing")?,
            args: vec![script, path_arg(wasm)],
            env: Vec::new(),
        },
        Driver::PywasmPyPy => Launch {
            program: pypy_bin().context("pypy3 not found")?,
            args: vec![script, path_arg(wasm)],
            env: Vec::new(),
        },
        Driver::Wardite(flag) => {
            let gem_home =
                wardite_gem_home().context("no GEM_HOME with wardite under benchmarks/cache/")?;
            Launch {
                program: dewasm_backend_ruby::find_ruby().context("ruby not found")?,
                args: vec![flag.to_string(), script, path_arg(wasm)],
                env: vec![
                    ("GEM_HOME".to_string(), path_arg(&gem_home)),
                    ("GEM_PATH".to_string(), path_arg(&gem_home)),
                ],
            }
        }
    })
}

/// `wasmtime` on PATH, if it runs at all.
pub fn wasmtime_bin() -> Option<PathBuf> {
    static BIN: OnceLock<Option<PathBuf>> = OnceLock::new();
    BIN.get_or_init(|| {
        let candidate = std::env::var_os("DEWASM_WASMTIME")
            .map_or_else(|| PathBuf::from("wasmtime"), PathBuf::from);
        probe(&candidate, &["--version"]).then_some(candidate)
    })
    .clone()
}

/// A host PyPy 3. Deliberately not provisioned by `benchmarks/setup.sh` — it is a whole alternative Python — so its absence is a normal, reported skip.
fn pypy_bin() -> Option<PathBuf> {
    static BIN: OnceLock<Option<PathBuf>> = OnceLock::new();
    BIN.get_or_init(|| {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(env) = std::env::var_os("DEWASM_PYPY") {
            candidates.push(PathBuf::from(env));
        }
        candidates.extend(["pypy3", "pypy3.11", "pypy3.10", "pypy"].map(PathBuf::from));
        candidates
            .into_iter()
            .find(|candidate| probe(candidate, &["-c", "import sys"]))
    })
    .clone()
}

/// The interpreter inside `benchmarks/cache/venv`, where `benchmarks/setup.sh` pins pywasm.
fn venv_python() -> Option<PathBuf> {
    let bin = bench_cache_dir().join("venv/bin");
    ["python3", "python"]
        .iter()
        .map(|name| bin.join(name))
        .find(|path| path.is_file())
}

/// The `GEM_HOME` under `benchmarks/cache/` that holds wardite. Found by looking for the `<gem_home>/gems/wardite-*` layout rather than by hardcoding a directory name, so the exact name `benchmarks/setup.sh` picks does not matter here.
fn wardite_gem_home() -> Option<PathBuf> {
    let entries = std::fs::read_dir(bench_cache_dir()).ok()?;
    entries.flatten().map(|entry| entry.path()).find(|dir| {
        std::fs::read_dir(dir.join("gems")).is_ok_and(|gems| {
            gems.flatten()
                .any(|gem| gem.file_name().to_string_lossy().starts_with("wardite-"))
        })
    })
}

/// The pywasm version as the installed distribution metadata reports it, under whichever host interpreter is asked.
fn pywasm_version(python: &Path, extra: &[&str]) -> Option<String> {
    let out = Command::new(python)
        .args(extra)
        .args([
            "-c",
            "import importlib.metadata as m; print('pywasm ' + m.version('pywasm'))",
        ])
        .output()
        .ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!v.is_empty()).then_some(v)
}

/// The temp directory holding generated sources and built binaries.
fn bench_tmp_dir() -> Result<PathBuf> {
    let dir = std::env::temp_dir().join("dewasm-bench");
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    Ok(dir)
}

/// Whether `ruby <flag>` actually delivers the JIT the flag names. Exit codes cannot be trusted here: a ruby built without YJIT accepts `--yjit`, prints a warning, and exits 0, which would put no-JIT timings in a JIT column. So ask the VM itself.
fn ruby_jit_available(ruby: &Path, flag: &str) -> Result<(), String> {
    let check = match flag {
        "--yjit" => "exit(defined?(RubyVM::YJIT) && RubyVM::YJIT.enabled? ? 0 : 1)",
        "--zjit" => "exit(defined?(RubyVM::ZJIT) && RubyVM::ZJIT.enabled? ? 0 : 1)",
        _ => "exit 0",
    };
    probe(ruby, &[flag, "-e", check])
        .then_some(())
        .ok_or_else(|| {
            format!("this ruby does not enable the JIT behind {flag} (built without it?)")
        })
}

/// Whether `program args...` runs and exits 0. Used for every availability probe.
fn probe(program: &Path, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .output()
        .is_ok_and(|out| out.status.success())
}

/// The first non-empty line of `program args...`, reading stdout and stderr both — `java -version` and `perl -v` each pick a different one.
fn capture_version(program: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// A path as a process argument. Benchmark artifacts live under paths the harness itself chose, so a lossy conversion cannot lose anything real here.
fn path_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
