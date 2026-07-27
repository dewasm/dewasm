//! Bash end-to-end suites (ADR-27): the shared library / WASI / apps case
//! consts (`dewasm-test-helper`) wired up for the Bash backend. Per the
//! ADR-27 revision this file holds ONLY the [`BackendUnderTest`] impl, named
//! glue string constants, and per-case macro invocations. Bash has no
//! host-language object model (ADR-12), so it invokes only the two
//! always-available library cases (glue is Bash function calls over the R0..
//! result globals, ADR-11), the whole-program WASI kinds it covers, and the
//! flat-namespace multi-module case (ADR-35).

use std::collections::BTreeSet;
use std::path::PathBuf;

use dewasm_backend::Backend;
use dewasm_backend_bash::{find_bash5, BashBackend};
use dewasm_test_helper::{
    cowsay_args_e2e, cowsay_stdin_e2e, examples_dir, gzip_e2e, library_add_e2e, qjs_eval_e2e,
    shared_table_e2e, sqlite3_shell_e2e, standalone_dir_unsupported_e2e, wasi_import_override_e2e,
    wasi_suite, BackendUnderTest,
};

pub struct Bash;

impl BackendUnderTest for Bash {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &BashBackend
    }

    fn interpreter(&self) -> PathBuf {
        find_bash5().expect("bash >= 5 not found — see docs/testing.md")
    }

    /// Compose several `.wat` modules for the multi-module cases (ADR-35).
    /// Bash has no namespacing at all — every generated module is already a
    /// bare, prefix-scoped family of global functions/arrays sharing one flat
    /// process, so there is no separate "shared runtime" linkage to build:
    /// generate each module (`generate_module_with_units`, prefixed by its
    /// name via `func_prefix`), union the runtime units they reference, bundle
    /// that union once, and concatenate the bundle with the module bodies.
    /// `shared_runtime=false` (independent Embedded runtimes) is never
    /// exercised for Bash: there is only ever one flat namespace, so two
    /// independent runtimes cannot coexist without their `rt_*`/`mem_*`
    /// function names colliding (`embedded_coexist_e2e!` is not invoked).
    fn compose_modules(&self, modules: &[(&str, &str)], shared_runtime: bool) -> String {
        assert!(
            shared_runtime,
            "bash multi-module: shared_runtime=false is excluded — bash has one \
             flat global namespace, so two independent runtimes cannot coexist \
             without their rt_*/mem_* names colliding (ADR-11/ADR-35)"
        );
        let mut units = BTreeSet::new();
        let mut decls = Vec::new();
        for (wat, name) in modules {
            let bytes = wat::parse_file(examples_dir().join(wat)).expect("parse wat");
            let module = dewasm_core::build_module(&bytes).expect("build IR");
            let prefix = dewasm_backend_bash::func_prefix(name);
            let (src, u) = dewasm_backend_bash::generate_module_with_units(&module, &prefix, false)
                .expect("generate");
            units.extend(u);
            decls.push(src);
        }
        format!(
            "{}\n{}",
            dewasm_backend_bash::shared_runtime(&units).expect("bundle runtime"),
            decls.join("\n")
        )
    }
}

// ---------------------------------------------------------------------
// Library-case glue: results come back through the R0 global (ADR-11).

/// `add.wat`: call the exported functions and echo each result global.
const BASH_ADD_GLUE: &str = r#"add_init || exit 1
add_invoke add 2 3; echo $R0
add_invoke add 4294967295 1; echo $R0
add_invoke fib 10; echo $R0
"#;

/// The ADR-7 override/fallback glue: an explicit `fd_write` import wins,
/// `random_get` falls back to the bundled WASI. Captures and prints the actual
/// bytes the module wrote — the same observable proof of interception the other
/// backends' override glues use.
const BASH_OVERRIDE_GLUE: &str = r#"my_fd_write() {
  # (fd, iovs, iovs_len, nwritten_ptr): capture and print the actual
  # bytes the module wrote, the same observable proof of interception
  # `RUBY_OVERRIDE_GLUE` uses, via the same byte-reconstruction the
  # bundled fd_write unit itself uses (runtime/bash/units/wasi/fd_write.sh).
  mem_i32_load prog_ "$2" || return $?
  local ptr=$R0
  mem_i32_load prog_ $(( $2 + 4 )) || return $?
  local len=$R0
  local -n mem=prog_mem
  local out='' chunk bytes=() j
  for (( j = 0; j < len; j++ )); do
    bytes+=("$(( mem[ptr + j] ))")
  done
  printf -v chunk '\\x%02x' "${bytes[@]}"
  out+=$chunk
  printf "$out"
  mem_i32_store prog_ "$4" "$len" || return $?
  R0=0
  return 0
}
declare -A IMPORTS=(['wasi_snapshot_preview1.fd_write']=my_fd_write)
prog_init || { echo "init failed" >&2; exit 1; }
prog_invoke '_start'
"#;

/// `SHARED_TABLE`'s driver: `shared_table_a.wat`'s module has no wasm-level
/// name, so `compose_modules` prefixes it from the case's "TableExp" label
/// (`func_prefix`); `shared_table_b.wat` imports it under the *wasm* module
/// name `"a"` (unrelated to "TableExp"), so PROVIDERS maps that literal key
/// to the exporter's generation prefix (ADR-35).
const BASH_SHARED_TABLE_GLUE: &str = r#"tableexp_init || { echo "init failed" >&2; exit 1; }
declare -A PROVIDERS=([a]=tableexp_)
tableimp_init || { echo "init failed" >&2; exit 1; }
tableimp_invoke call0
echo $R0
"#;

// ---------------------------------------------------------------------
// Suite wiring (ADR-27): each per-case macro invocation declares participation.

library_add_e2e!(Bash, BASH_ADD_GLUE);
wasi_import_override_e2e!(Bash, BASH_OVERRIDE_GLUE);
// custom_wasi_provider_e2e! / partial_override_e2e! / stdio_capture_e2e!: not
// invoked — Bash has no host-language object model to replace WASI wholesale,
// probe its lazy construction, or redirect stdio into an in-memory object
// (ADR-12).

wasi_suite!(Bash, Stdio);
wasi_suite!(Bash, ArgsEnv);
// wasi_suite!(Bash, Poll) / wasi_suite!(Bash, Fs) / wasi_root_containment_e2e!:
// not invoked — Bash resolves poll_oneoff to ENOSYS and has no WASI filesystem
// (ADR-12/ADR-14). Instead of standalone_dir_e2e! (which mounts a --dir preopen),
// Bash asserts that --dir fails loudly (ADR-31).
standalone_dir_unsupported_e2e!(Bash);

cowsay_args_e2e!(Bash);
cowsay_stdin_e2e!(Bash);
// qjs_eval_e2e! / sqlite3_shell_e2e!: invoked, but heavy — Bash's softfloat
// makes QuickJS/SQLite take tens of seconds, so the generated tests are
// `#[ignore]`d by default; `--features heavy_test` or `-- --include-ignored` runs
// them anyway (same as every other backend, ADR-27 revision).
qjs_eval_e2e!(Bash);
sqlite3_shell_e2e!(Bash);
// minigzip is integer-only (no softfloat), so it runs under Bash by default,
// unlike the heavy floating-point apps (QuickJS/SQLite).
gzip_e2e!(Bash);
// No fs_apps / capi macros: Bash has no host-language C API to plumb a
// callback binding through, and (on this branch) no WASI filesystem to back
// an fs app (ADR-12; see ADR-14-equivalent work in progress on a sibling
// branch).

shared_table_e2e!(Bash, BASH_SHARED_TABLE_GLUE);
// embedded_coexist_e2e!: not invoked — Bash has one flat global namespace
// (no nested runtime/class construct), so two independently-generated
// runtimes cannot coexist in one process without their rt_*/mem_* function
// names colliding (ADR-11).
