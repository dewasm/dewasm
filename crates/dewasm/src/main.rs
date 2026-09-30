use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Parser;
use dewasm_backend::{Backend, DataFileConfig, GenOptions, Mode, RuntimeLinkage};
use dewasm_backend_bash::BashBackend;
use dewasm_backend_codon::CodonBackend;
use dewasm_backend_go::GoBackend;
use dewasm_backend_java::JavaBackend;
use dewasm_backend_perl::PerlBackend;
use dewasm_backend_python::PythonBackend;
use dewasm_backend_ruby::RubyBackend;

/// Translate a WebAssembly binary into source code of various languages.
#[derive(Parser)]
#[command(name = "dewasm", version)]
struct Cli {
    /// Input file (.wasm or .wat)
    input: PathBuf,

    /// Target language: ruby, bash, python, perl, go, java, or codon.
    #[arg(short, long)]
    target: String,

    /// Output mode: "library" exposes exports to the host language.
    /// "standalone" sets up WASI and runs _start.
    #[arg(short, long, default_value = "library")]
    mode: String,

    /// Output file path ("-" for stdout)
    #[arg(short, long, default_value = "-")]
    output: PathBuf,

    /// Library-mode name of the generated class/module/package, used verbatim.
    /// A name that does not fit the target language's grammar is rejected.
    /// Required for --mode library.
    /// Incompatible with --mode standalone, whose internal name is fixed.
    #[arg(long)]
    module_name: Option<String>,

    /// Do not bundle the built-in WASI implementation for wasi_snapshot_preview1 imports.
    /// Incompatible with --mode standalone.
    #[arg(long)]
    no_default_wasi: bool,

    /// Externalize data-segment bytes into a binary data file written to this path.
    /// Without it they are embedded as literals in the source.
    #[arg(long)]
    data_file: Option<PathBuf>,

    /// Parse the module's DWARF `.debug_*` sections and emit source-position markers.
    #[arg(long)]
    dwarf_line: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let backend: Box<dyn Backend> = match cli.target.as_str() {
        "ruby" => Box::new(RubyBackend),
        "bash" => Box::new(BashBackend),
        "python" => Box::new(PythonBackend),
        "codon" => Box::new(CodonBackend),
        "perl" => Box::new(PerlBackend),
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
    // A standalone artifact is a self-contained program.
    // Its internal class/package/prefix name is not part of any interface.
    // So the name is fixed per backend.
    // Naming it is then a mistake worth reporting rather than ignoring.
    if cli.module_name.is_some() && mode == Mode::Standalone {
        bail!("standalone output has a fixed internal name; --module-name applies to library mode");
    }

    // Data-segment externalization is opt-in and supported for ruby/go/python/java/perl only.
    // It needs a real data-file path (not stdout).
    // Reject unsupported combinations first with a clear, attributed error instead of mis-emitting.
    let data_file = match &cli.data_file {
        Some(path) => {
            match cli.target.as_str() {
                "ruby" | "go" | "python" | "java" | "perl" => {}
                "bash" => bail!(
                    "--data-file is not supported for the bash target: the bash \
                     backend embeds data segments in its runtime, not as a data file"
                ),
                other => bail!("--data-file is not supported for target {other}"),
            }
            if cli.output == Path::new("-") {
                bail!(
                    "--data-file cannot be combined with -o - (stdout): the data file \
                     must be written to a real path next to the generated program"
                );
            }
            // A --data-file resolving to the same file as -o would clobber the generated source.
            // Fail before anything is written.
            if resolve_for_collision(path) == resolve_for_collision(&cli.output) {
                bail!(
                    "--data-file {} resolves to the same file as the output path {}: \
                     the data file would overwrite the generated source \
                     (choose a different --data-file path)",
                    path.display(),
                    cli.output.display()
                );
            }
            let data_file_name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .with_context(|| format!("--data-file path has no filename: {}", path.display()))?;
            Some(DataFileConfig { data_file_name })
        }
        None => None,
    };

    // Library mode requires an explicit name.
    // Deriving one from the file name is an implicit mapping.
    // Its result depends on how the input is stored, not on what the caller wants to embed.
    // Standalone output never reads the name beyond the OutputFile label (checked above).
    // So a fixed placeholder matching the fixed internal name is used.
    let module_name = match (mode, cli.module_name) {
        (Mode::Standalone, _) => "program".to_string(),
        (Mode::Library, Some(name)) => name,
        (Mode::Library, None) => {
            bail!("library mode requires --module-name (the class/package name the output defines)")
        }
    };

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
    // Component-model binaries (layer 1) are out of scope.
    // Reject them at conversion time with a clear, attributed error.
    if dewasm_core::is_component(&bytes) {
        return Err(dewasm_core::feature::UnsupportedError::new(
            dewasm_core::feature::Feature::ComponentModel,
            "component-model binaries are not supported; convert the core module instead",
        )
        .into());
    }
    let module = dewasm_core::build_module_with_options(
        &bytes,
        &dewasm_core::BuildOptions {
            debug_line: cli.dwarf_line,
        },
    )?;
    let files = backend.generate(&module, &opts)?;

    // Route by name: the data file goes to `--data-file`'s path, the primary source to `-o`.
    // The data file is the one whose `name` is the configured `data_file_name`.
    let data_file_name = opts.data_file.as_ref().map(|c| c.data_file_name.as_str());
    // A generated source sharing `data_file_name` would be misrouted and clobbered.
    // An example is java's fixed `Main.java`.
    // `matching > 1` means the source and the data file collide.
    // `matching == files.len()` means no data file was emitted and the match is the source itself.
    if let Some(name) = data_file_name {
        let matching = files.iter().filter(|f| f.name == name).count();
        if matching > 1 || matching == files.len() {
            bail!(
                "--data-file name {name:?} collides with a generated output file \
                 of the {} backend; choose a different data-file name",
                backend.name()
            );
        }
    }
    for file in files {
        if Some(file.name.as_str()) == data_file_name {
            let path = cli
                .data_file
                .as_ref()
                .expect("a data-file output requires --data-file");
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

/// Canonicalize for the --data-file/-o collision check, so differently spelled paths compare equal.
/// It tries the file, else parent + final component, else the cwd-anchored absolute path.
fn resolve_for_collision(path: &Path) -> PathBuf {
    if let Ok(resolved) = path.canonicalize() {
        return resolved;
    }
    if let Some(name) = path.file_name() {
        let parent = match path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => Path::new("."),
        };
        if let Ok(parent) = parent.canonicalize() {
            return parent.join(name);
        }
    }
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}
