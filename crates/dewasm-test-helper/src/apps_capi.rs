//! App cases that drive a C API: a converted library-mode artifact with its C API driven directly.
//! Host-language glue drives it.
//! The glue calls `sqlite3_malloc` and handles guest-memory pointers.
//! For the callback case it also provides an imported `env.host_row`.
//! Unlike the `apps`/`fs_apps` suites there is **no Wasmtime snapshot**.
//! The CLI cannot drive a C-API flow whose results live in guest memory.
//! So each case holds a fixed expected string.
//! Every value in it is set by the fixed amalgamation version in `examples/apps/setup.sh`.
//! It is also set by the `-wasm` suffix the build patches into that version string.
//!
//! Each case is a `pub const` [`CApiCase`] driven by a per-case macro.
//! Examples are `libsqlite3_c_api_e2e!`, `pcap_compile_e2e!`, and `zeroperl_eval_e2e!`.
//! The per-language variation is the named glue constant passed to that macro.
//! The glue writes out `malloc`, pointer handling, memory access, and provider registration.
//! It writes them literally, in the backend's language.
//! The runner fills the `{scratch}`/`{cache}` placeholders.
//! They carry the file-backed case's runtime scratch path and the app-cache root.
//! Staged fixtures (the ExifTool image) are copied into that scratch directory.
//! Which backends call a macro is the capability declaration; every backend does, Bash included.
//! In Bash a guest pointer is just a decimal in its `R0` result global (issue #138).
//! Guest memory there is the module's byte array.
//! These cases reconvert artifacts ranging from 1.5 MB (tree-sitter) to 25 MB (`zeroperl`).
//! So each per-case macro expands its generated `#[test]` as `#[ignore]`d by default.
//! It does so unless the expanding backend crate's `slow_test` feature is enabled.
//! [`run_capi_case`] itself just runs the case unconditionally.

use std::path::{Path, PathBuf};

use dewasm_backend::Mode;

use crate::apps_fs::{stage_into, Stage};
use crate::backend::BackendUnderTest;
use crate::fixtures::{apps_cache_dir, apps_fixtures_dir, fresh_scratch_dir};
use crate::glue::fill;

/// A C-API-driving case: convert `wasm` (cache stem) to library class `class`.
/// Append the backend's glue, run it, and require exactly `expect_stdout`.
pub struct CApiCase {
    pub name: &'static str,
    /// Cache-binary stem (`examples/apps/cache/<wasm>.wasm`).
    /// It is also the kebab-case name [`BackendUnderTest::module_name`] converts.
    /// The result is the conversion module name.
    pub wasm: &'static str,
    /// The library class name the glue instantiates: what the Pascal derivation of `wasm` yields.
    /// The Bash glue instead spells the snake derivation's `<name>_` prefix.
    pub class: &'static str,
    /// The fixed `stdout` the drive must produce (no Wasmtime snapshot possible).
    pub expect_stdout: &'static str,
    /// Fixtures staged into the fresh scratch directory before the run.
    /// Their paths are relative to [`apps_fixtures_dir`].
    /// The glue reaches them through `{scratch}`.
    /// Empty for the in-memory cases.
    pub stage: &'static [Stage],
    /// Host-side assertion over the scratch directory after the run (file cases).
    /// `assert_none` when there is nothing to check.
    pub assert_host: fn(&Path),
}

/// No host-side effect to assert for this case.
fn assert_none(_: &Path) {}

/// The file-backed drive must leave a nonzero DB file behind at `<scratch>/data.db`.
fn assert_dbfile(scratch: &Path) {
    assert!(
        scratch
            .join("data.db")
            .metadata()
            .map(|m| m.len() > 0)
            .unwrap_or(false),
        "sqlite3 file C API: no nonzero DB file left on the host"
    );
}

/// The library half of the sqlite3 build: the C API driven in memory.
/// It prints the version, two SELECT rows, and an end marker, all set by the amalgamation version.
/// The reported version carries the `-wasm` suffix `examples/apps/scripts/sqlite3.sh` patches in.
/// So example output identifies the converted engine.
/// In-memory (`:memory:`), so the `{scratch}` placeholder goes unused.
pub const LIBSQLITE3_C_API: CApiCase = CApiCase {
    name: "libsqlite3_c_api",
    wasm: "libsqlite3",
    class: "Libsqlite3",
    expect_stdout: "version: 3.53.3-wasm\n20|y\n10|x\nC-API-OK\n",
    stage: &[],
    assert_host: assert_none,
};

/// The same C API opening a *file* under a preopen.
/// It runs create+insert, close, reopen, and select.
/// This proves the C-API path hits the same file system stack as the shell.
/// The glue preopens the fresh scratch directory via `{scratch}`.
/// It leaves a nonzero DB file on the host.
pub const SQLITE3_FILE_C_API: CApiCase = CApiCase {
    name: "sqlite3_file_c_api",
    wasm: "libsqlite3",
    class: "Libsqlite3",
    expect_stdout: "10|x\n20|y\nFILE-OK\n",
    stage: &[],
    assert_host: assert_dbfile,
};

/// Guest->host callback round trip: our own committed C exports `run_query`.
/// That C is `examples/apps/src/sqlite3_binding.c`.
/// `run_query` calls `sqlite3_exec` with a C callback.
/// The callback forwards each row to the *imported* `env.host_row`.
/// The glue provides `host_row` via the import provider and collects the rows.
pub const SQLITE3_CALLBACK_BINDING: CApiCase = CApiCase {
    name: "sqlite3_callback_binding",
    wasm: "sqlite3-binding",
    class: "Sqlite3Binding",
    expect_stdout: "row: 2|y\nrow: 3|z\nCALLBACK-OK\n",
    stage: &[],
    assert_host: assert_none,
};

/// `libpcap` BPF filter compilation: our own committed C exports `compile_filter`.
/// That C is `examples/apps/src/pcap_binding.c`.
/// `compile_filter` runs `libpcap`'s platform-independent BPF compiler (`pcap_compile_nopcap`).
/// It compiles a filter given as text and serializes the resulting program into guest memory.
/// The layout is `[u32 bf_len][bf_len × {u16 code; u8 jt; u8 jf; u32 k}]`.
/// The glue drives `compile_filter("tcp port 80", DLT_EN10MB=1, 65535)`.
/// It prints each instruction as `code jt jf k`, and an end marker.
/// The expected output is the canonical tcp-port-80 filter.
/// It checks EtherType IPv6 0x86dd/IPv4 0x0800, IP protocol TCP=6, and port 80.
/// It is deterministic because BPF programs hold offsets/constants only, no addresses.
/// In-memory, so `{scratch}` goes unused.
pub const PCAP_COMPILE: CApiCase = CApiCase {
    name: "pcap_compile",
    wasm: "libpcap",
    class: "Libpcap",
    expect_stdout: "40 0 0 12\n21 0 6 34525\n48 0 0 20\n21 0 15 6\n40 0 0 54\n\
                    21 12 0 80\n40 0 0 56\n21 10 11 80\n21 0 10 2048\n48 0 0 23\n\
                    21 0 8 6\n40 0 0 20\n69 6 0 8191\n177 0 0 14\n72 0 0 14\n\
                    21 2 0 80\n72 0 0 16\n21 0 1 80\n6 0 0 65535\n6 0 0 0\nBPF-OK\n",
    stage: &[],
    assert_host: assert_none,
};

/// tree-sitter JSON parse: our own committed C exports `parse_source`.
/// That C is `examples/apps/src/treesitter_binding.c`.
/// `parse_source` parses a source string with the tree-sitter runtime.
/// It uses the pre-generated `tree-sitter-json` grammar.
/// It returns the parse tree's S-expression (a `malloc`'d C string) via `ts_node_string`.
/// The glue parses the fixed snippet `{"key": [1, true, null]}`.
/// It prints the S-expression, and an end marker.
/// The output is deterministic (the grammar at its fixed version sets tree-sitter's node naming).
/// In-memory, so `{scratch}` goes unused.
pub const TREESITTER_PARSE: CApiCase = CApiCase {
    name: "treesitter_parse",
    wasm: "treesitter",
    class: "Treesitter",
    expect_stdout: "(document (object (pair key: (string (string_content)) \
                    value: (array (number) (true) (null)))))\nTS-OK\n",
    stage: &[],
    assert_host: assert_none,
};

/// `zeroperl` Perl-5.42 evaluation (retraction, issue #67).
/// The prebuilt `@6over3/zeroperl-ts` reactor exposes an embedding C API.
/// The glue drives `_initialize` → `zeroperl_init` → `malloc`.
/// It copies a Perl program into guest memory.
/// It then drives `zeroperl_eval` → `zeroperl_flush`.
/// The imported `env.call_host_function` is a zero-returning stub.
/// It is called only if the guest registers host callbacks, which this program does not.
/// `zeroperl_init` needs `/dev/null` resolvable, so the glue preopens it.
/// It is mapped guest→host `/dev/null`; without it, `zeroperl_init` returns 1.
/// The expected output is a regular expression + `printf` line, deterministic.
pub const ZEROPERL_EVAL: CApiCase = CApiCase {
    name: "zeroperl_eval",
    wasm: "zeroperl",
    class: "Zeroperl",
    expect_stdout: "m=hello|world|42 sum=50\n",
    stage: &[],
    assert_host: assert_none,
};

/// ExifTool 13.42 on `zeroperl` (issue #70).
/// The flattened `exiftool` CLI driver runs on the *same* `cache/zeroperl.wasm` reactor.
/// The driver is `src/exiftool` of `6over3/exiftool`, fetched into `cache/exiftool-lib/`.
/// Its `@6over3/zeroperl-ts` SFS blob already embeds the full `Image::ExifTool` module tree.
/// So `use Image::ExifTool` resolves in-guest with no module preopen.
/// The glue drives `_initialize` → `zeroperl_init` → `zeroperl_eval` of a driver snippet.
/// The snippet sets `@ARGV`/`$0` and `do`es the script; `zeroperl_flush` follows.
/// Deterministic tags only (`-S -Make -Model -DateTimeOriginal`).
/// They are read from the committed `exif_fixture.jpg`.
/// They are cross-checked against host `exiftool`.
/// The image is staged at `/img`, and the driver at `/work`.
/// `/dev/null` is preopened for `zeroperl_init` as in [`ZEROPERL_EVAL`].
pub const EXIFTOOL_EXTRACT: CApiCase = CApiCase {
    name: "exiftool_extract",
    wasm: "zeroperl",
    class: "Zeroperl",
    expect_stdout: "Make: DewasmCam\nModel: Model-X\nDateTimeOriginal: 2020:01:02 03:04:05\n",
    stage: &[Stage::File {
        src: "exif_fixture.jpg",
        dst: "exif_fixture.jpg",
    }],
    assert_host: assert_none,
};

/// Run one [`CApiCase`] for `lang` with its per-language `glue` unconditionally.
/// The skip for speed lives at the macro/feature level, see the module documentation.
/// Stages `case.stage` into a fresh scratch directory and fills `{scratch}`/`{cache}` in `glue`.
/// Those are the file-backed and ExifTool cases' preopens; the in-memory cases leave them unused.
pub fn run_capi_case(lang: &dyn BackendUnderTest, case: &CApiCase, glue: &str) {
    let cache = apps_cache_dir();
    let wasm_path = cache.join(format!("{}.wasm", case.wasm));
    assert!(
        wasm_path.exists(),
        "{} not cached: run examples/apps/setup.sh (see docs/testing.md)",
        case.wasm
    );
    let bytes = std::fs::read(&wasm_path).expect("read wasm");
    let class = lang.convert_app(&bytes, Mode::Library, &lang.module_name(case.wasm));

    let scratch: PathBuf = fresh_scratch_dir(&format!("{}-{}", lang.name(), case.name));
    stage_into(&apps_fixtures_dir(), &scratch, case.stage);
    let glue = fill(
        glue,
        &[
            ("scratch", &scratch.to_string_lossy()),
            ("cache", &cache.to_string_lossy()),
        ],
    );
    let output = lang.run(&format!("{class}\n{glue}"), &[], "");
    assert!(
        output.status.success(),
        "{} under {}: nonzero exit {}\n{}",
        case.name,
        lang.name(),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        case.expect_stdout,
        "{} under {}: C-API drive output differs\nstderr: {}",
        case.name,
        lang.name(),
        String::from_utf8_lossy(&output.stderr)
    );
    (case.assert_host)(&scratch);
    println!("{} under {}: C-API drive matches", case.name, lang.name());
}
