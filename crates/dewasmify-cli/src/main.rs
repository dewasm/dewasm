use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Parser;
use dewasmify_backend::{Backend, GenOptions, Mode, RuntimeLinkage};
use dewasmify_backend_bash::BashBackend;
use dewasmify_backend_ruby::RubyBackend;

/// Translate a WebAssembly binary into source code of various languages.
#[derive(Parser)]
#[command(name = "dewasmify", version)]
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let backend: Box<dyn Backend> = match cli.target.as_str() {
        "ruby" => Box::new(RubyBackend),
        "bash" => Box::new(BashBackend),
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

    let module = dewasmify_core::build_module(&bytes)?;
    let files = backend.generate(
        &module,
        &GenOptions {
            mode,
            module_name,
            runtime: RuntimeLinkage::Embedded,
            default_wasi: !cli.no_default_wasi,
        },
    )?;

    for file in files {
        if cli.output == PathBuf::from("-") {
            print!("{}", file.contents);
        } else {
            std::fs::write(&cli.output, &file.contents)
                .with_context(|| format!("failed to write {}", cli.output.display()))?;
        }
    }
    Ok(())
}
