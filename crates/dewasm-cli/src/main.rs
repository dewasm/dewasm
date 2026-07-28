use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Parser;
use dewasm_backend::{Backend, DataFileConfig, GenOptions, Mode, RuntimeLinkage};
use dewasm_backend_bash::BashBackend;
use dewasm_backend_go::GoBackend;
use dewasm_backend_java::JavaBackend;
use dewasm_backend_python::PythonBackend;
use dewasm_backend_ruby::RubyBackend;

/// Translate a WebAssembly binary into source code of various languages.
#[derive(Parser)]
#[command(name = "dewasm", version)]
struct Cli {
    /// Input file (.wasm or .wat)
    input: PathBuf,

    /// Target language
    #[arg(short, long, default_value = "ruby")]
    target: String,

    /// Output mode: "library" exposes exports to the host language,
    /// "standalone" wires up WASI and runs _start.
    #[arg(short, long, default_value = "library")]
    mode: String,

    /// Name used for the generated class/module (defaults to the input
    /// file stem)
    #[arg(long)]
    module_name: Option<String>,

    /// Output file path ("-" for stdout)
    #[arg(short, long, default_value = "-")]
    output: PathBuf,

    /// Do not bundle the built-in WASI implementation as a fallback for
    /// wasi_snapshot_preview1 imports (all imports must then be provided
    /// by the embedder). Incompatible with --mode standalone.
    #[arg(long)]
    no_default_wasi: bool,

    /// Externalize data-segment bytes into a binary sidecar written to this
    /// path instead of embedding them as literals in the source (ADR-37).
    /// The generated program loads the sidecar relative to itself, so keep it
    /// next to the output file. Supported for ruby and go; incompatible with
    /// `-o -`.
    #[arg(long)]
    data_file: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let backend: Box<dyn Backend> = match cli.target.as_str() {
        "ruby" => Box::new(RubyBackend),
        "bash" => Box::new(BashBackend),
        "python" => Box::new(PythonBackend),
        "go" => Box::new(GoBackend),
        "java" => Box::new(JavaBackend),
        other => bail!("unsupported target language: {other}"),
    };

    let mode = match cli.mode.as_str() {
        "library" => Mode::Library,
        "standalone" => Mode::Standalone,
        other => bail!("unsupported mode: {other} (expected library or standalone)"),
    };
    if cli.no_default_wasi && mode == Mode::Standalone {
        bail!("--no-default-wasi cannot be combined with --mode standalone");
    }

    // Data-segment externalization (ADR-37): opt-in, ruby/go only, needs a
    // real sidecar path (not stdout). Reject the unsupported combinations at
    // the front with a clear, attributed error rather than mis-emitting.
    let data_file = match &cli.data_file {
        Some(path) => {
            match cli.target.as_str() {
                "ruby" | "go" => {}
                "bash" => bail!(
                    "--data-file is not supported for the bash target: the bash \
                     backend embeds data segments in its runtime, not as a sidecar; \
                     only ruby and go externalize today (ADR-37)"
                ),
                "python" | "java" => bail!(
                    "--data-file is not yet supported for the {} target: it is \
                     deliberately deferred — ruby and go land first (ADR-37)",
                    cli.target
                ),
                other => bail!("--data-file is not supported for target {other}"),
            }
            if cli.output == Path::new("-") {
                bail!(
                    "--data-file cannot be combined with -o - (stdout): the sidecar \
                     must be written to a real path next to the generated program"
                );
            }
            let sidecar_name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .with_context(|| format!("--data-file path has no filename: {}", path.display()))?;
            Some(DataFileConfig { sidecar_name })
        }
        None => None,
    };

    let module_name = cli.module_name.unwrap_or_else(|| {
        cli.input
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "module".to_string())
    });

    let input = std::fs::read(&cli.input)
        .with_context(|| format!("failed to read {}", cli.input.display()))?;
    // Accept .wat text input as well; wat::parse_bytes passes .wasm through.
    let bytes = wat::parse_bytes(&input).context("failed to parse input")?;

    let opts = GenOptions {
        mode,
        module_name,
        runtime: RuntimeLinkage::Embedded,
        default_wasi: !cli.no_default_wasi,
        data_file,
    };
    // Component-model binaries (layer 1) are out of scope (ADR-24): reject
    // them at conversion time with a clear, attributed error (ADR-0).
    if dewasm_core::is_component(&bytes) {
        return Err(dewasm_core::feature::UnsupportedError::new(
            dewasm_core::feature::Feature::ComponentModel,
            "component-model binaries are not supported; convert the core module instead",
        )
        .into());
    }
    let module = dewasm_core::build_module(&bytes)?;
    let files = backend.generate(&module, &opts)?;

    // Route by name: the data sidecar (its `name` is the configured
    // `sidecar_name`) goes to `--data-file`'s path, the primary source to
    // `-o` (ADR-37).
    let sidecar_name = opts.data_file.as_ref().map(|c| c.sidecar_name.as_str());
    for file in files {
        if Some(file.name.as_str()) == sidecar_name {
            let path = cli
                .data_file
                .as_ref()
                .expect("sidecar requires --data-file");
            std::fs::write(path, &file.contents)
                .with_context(|| format!("failed to write {}", path.display()))?;
        } else if cli.output == Path::new("-") {
            use std::io::Write as _;
            std::io::stdout()
                .write_all(&file.contents)
                .context("failed to write to stdout")?;
        } else {
            std::fs::write(&cli.output, &file.contents)
                .with_context(|| format!("failed to write {}", cli.output.display()))?;
        }
    }
    Ok(())
}
