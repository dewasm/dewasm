//! C-API-driving app cases: a converted library-mode artifact whose C API is driven directly from host-language glue — `sqlite3_malloc`, guest-memory pointer plumbing, and (for the callback case) an imported `env.host_row` provider. Unlike the `apps`/`fs_apps` suites there is **no wasmtime snapshot**: the CLI cannot drive a C-API flow whose results live in guest memory, so each case pins a fixed expected string, every value in it anchored by the amalgamation version pinned in `examples/apps/setup.sh`.
//!
//! Each case is a `pub const` [`CApiCase`] driven by a per-case macro (`libsqlite3_c_api_e2e!`, `pcap_compile_e2e!`, `zeroperl_eval_e2e!`, …). The per-language variation is the named glue const passed to that macro (malloc/pointer plumbing/memory access/provider registration written out literally in the backend's language); the file-backed case's runtime scratch path (and the app-cache root) arrive through the `{scratch}`/`{cache}` placeholders the runner fills, with staged fixtures (the exiftool image) copied into that scratch dir. Which backends invoke a macro is the capability declaration; every backend does, Bash included — a guest pointer is just a decimal in its `R0` result global and guest memory is the module's byte array (issue #138). These cases reconvert artifacts ranging from 1.5 MB (tree-sitter) to 25 MB (zeroperl), so each per-case macro expands its generated `#[test]` as `#[ignore]`d unless the expanding backend crate's `slow_test` feature is enabled; [`run_capi_case`] itself just runs the case unconditionally.

use std::path::{Path, PathBuf};

use dewasm_backend::Mode;

use crate::apps_fs::{stage_into, Stage};
use crate::backend::BackendUnderTest;
use crate::fixtures::{apps_cache_dir, apps_fixtures_dir, fresh_scratch_dir};
use crate::glue::fill;

/// A C-API-driving case: convert `wasm` (cache stem) to library class `class`, append the backend's glue, run it, and require exactly `expect_stdout`.
pub struct CApiCase {
    pub name: &'static str,
    /// Cache-binary stem (`examples/apps/cache/<wasm>.wasm`), also the kebab-case name [`BackendUnderTest::module_name`] converts into the conversion module name.
    pub wasm: &'static str,
    /// The library class name the glue instantiates — what the Pascal derivation of `wasm` yields (the Bash glue instead spells the snake derivation's `<name>_` prefix).
    pub class: &'static str,
    /// The fixed stdout the drive must produce (no wasmtime snapshot possible).
    pub expect_stdout: &'static str,
    /// Fixtures staged into the fresh scratch dir before the run (relative to [`apps_fixtures_dir`]); the glue reaches them through `{scratch}`. Empty for the in-memory cases.
    pub stage: &'static [Stage],
    /// Host-side assertion over the scratch dir after the run (file cases); `assert_none` when there is nothing to check.
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

/// The library half of the sqlite3 build: the C API driven in memory. version + two SELECT rows + a sentinel, all pinned by the amalgamation version. In-memory (`:memory:`), so the `{scratch}` placeholder goes unused.
pub const LIBSQLITE3_C_API: CApiCase = CApiCase {
    name: "libsqlite3_c_api",
    wasm: "libsqlite3",
    class: "Libsqlite3",
    expect_stdout: "version: 3.53.3\n20|y\n10|x\nC-API-OK\n",
    stage: &[],
    assert_host: assert_none,
};

/// The same C API opening a *file* under a preopen: create+insert, close, reopen, select — proving the C-API path hits the same fs stack as the shell. The glue preopens the fresh scratch dir via `{scratch}` and leaves a nonzero DB file on the host.
pub const SQLITE3_FILE_C_API: CApiCase = CApiCase {
    name: "sqlite3_file_c_api",
    wasm: "libsqlite3",
    class: "Libsqlite3",
    expect_stdout: "10|x\n20|y\nFILE-OK\n",
    stage: &[],
    assert_host: assert_dbfile,
};

/// Guest->host callback round trip: our own committed C (examples/apps/src/sqlite3_binding.c) exports `run_query`, which calls `sqlite3_exec` with a C callback forwarding each row to the *imported* `env.host_row`. The glue provides `host_row` via the import provider and collects the rows.
pub const SQLITE3_CALLBACK_BINDING: CApiCase = CApiCase {
    name: "sqlite3_callback_binding",
    wasm: "sqlite3-binding",
    class: "Sqlite3Binding",
    expect_stdout: "row: 2|y\nrow: 3|z\nCALLBACK-OK\n",
    stage: &[],
    assert_host: assert_none,
};

/// libpcap BPF filter compilation: our own committed C (examples/apps/src/pcap_binding.c) exports `compile_filter`, which runs libpcap's platform-independent BPF compiler (`pcap_compile_nopcap`) on a textual filter and serializes the resulting program into guest memory as `[u32 bf_len][bf_len × {u16 code; u8 jt; u8 jf; u32 k}]`. The glue drives `compile_filter("tcp port 80", DLT_EN10MB=1, 65535)`, prints each insn as `code jt jf k`, and a sentinel. The pinned output is the canonical tcp-port-80 filter (ethertype IPv6 0x86dd/IPv4 0x0800, IP proto TCP=6, port 80), deterministic because BPF programs hold offsets/constants only, no addresses. In-memory, so `{scratch}` goes unused.
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

/// tree-sitter JSON parse: our own committed C (examples/apps/src/treesitter_binding.c) exports `parse_source`, which parses a source string with the tree-sitter runtime + the pre-generated tree-sitter-json grammar and returns the parse tree's S-expression (a malloc'd C string) via `ts_node_string`. The glue parses the fixed snippet `{"key": [1, true, null]}`, prints the S-expression, and a sentinel. The output is deterministic (tree-sitter's node naming is fixed by the pinned grammar). In-memory, so `{scratch}` goes unused.
pub const TREESITTER_PARSE: CApiCase = CApiCase {
    name: "treesitter_parse",
    wasm: "treesitter",
    class: "Treesitter",
    expect_stdout: "(document (object (pair key: (string (string_content)) \
                    value: (array (number) (true) (null)))))\nTS-OK\n",
    stage: &[],
    assert_host: assert_none,
};

/// zeroperl Perl-5.42 eval (retraction, issue #67): the prebuilt `@6over3/zeroperl-ts` reactor exposes an embedding C API; the glue drives `_initialize` → `zeroperl_init` → `malloc` + copy a Perl program into guest memory → `zeroperl_eval` → `zeroperl_flush`. The imported `env.call_host_function` is a zero-returning stub (called only if the guest registers host callbacks, which this program does not); `zeroperl_init` needs `/dev/null` resolvable, so the glue preopens it (mapped guest→host `/dev/null`) — without it, init returns 1. The pinned output is a regex + `printf` line, deterministic.
pub const ZEROPERL_EVAL: CApiCase = CApiCase {
    name: "zeroperl_eval",
    wasm: "zeroperl",
    class: "Zeroperl",
    expect_stdout: "m=hello|world|42 sum=50\n",
    stage: &[],
    assert_host: assert_none,
};

/// ExifTool 13.42 on zeroperl (issue #70): the flattened `exiftool` CLI driver (6over3/exiftool `src/exiftool`, fetched into `cache/exiftool-lib/`) runs on the *same* `cache/zeroperl.wasm` reactor, whose `@6over3/zeroperl-ts` SFS blob already embeds the full `Image::ExifTool` module tree, so `use Image::ExifTool` resolves in-guest with no module preopen. The glue drives `_initialize` → `zeroperl_init` → `zeroperl_eval` of a driver snippet that sets `@ARGV`/`$0` and `do`es the script, then `zeroperl_flush`. Deterministic tags only (`-S -Make -Model -DateTimeOriginal`) over the committed `exif_fixture.jpg`, cross-checked against host exiftool; the image is staged at `/img`, the driver at `/work` (`/dev/null` preopened for `zeroperl_init` as in [`ZEROPERL_EVAL`]).
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

/// Run one [`CApiCase`] for `lang` with its per-language `glue` unconditionally (the perf opt-out lives at the macro/feature level, see the module docs). Stages `case.stage` into a fresh scratch dir and fills `{scratch}`/`{cache}` in `glue` (the file-backed and exiftool cases' preopens; unused by the in-memory cases).
pub fn run_capi_case(lang: &dyn BackendUnderTest, case: &CApiCase, glue: &str) {
    let cache = apps_cache_dir();
    let wasm_path = cache.join(format!("{}.wasm", case.wasm));
    assert!(
        wasm_path.exists(),
        "{} not cached — run examples/apps/setup.sh (see docs/testing.md)",
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
