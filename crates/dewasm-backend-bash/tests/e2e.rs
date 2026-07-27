//! Bash end-to-end suites (ADR-27): the shared library / WASI / apps case
//! consts (`dewasm-test-helper`) wired up for the Bash backend. Per the
//! ADR-27 revision this file holds ONLY the [`BackendUnderTest`] impl, named
//! glue string constants, and per-case macro invocations. Bash has no WASI
//! filesystem support and no host-language object model (ADR-12), so it invokes
//! only the two always-available library cases (glue is Bash function calls over
//! the R0.. result globals, ADR-11) and the whole-program WASI kinds it covers.

use std::path::PathBuf;

use dewasm_backend::Backend;
use dewasm_backend_bash::{find_bash5, BashBackend};
use dewasm_test_helper::{
    cowsay_args_e2e, cowsay_stdin_e2e, gzip_e2e, library_add_e2e, qjs_eval_e2e, sqlite3_shell_e2e,
    standalone_dir_e2e, wasi_import_override_e2e, wasi_root_containment_e2e, wasi_suite,
    BackendUnderTest,
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

/// The `wasi_suite!(Bash, Fs, ...)` template (ADR-32): fill `WASI_DIRS` with
/// the one preopen pair, init, invoke `_start`, then surface a `proc_exit`
/// call the same way the standalone main does — `invoke` returns status 133
/// (ADR-12) with the code in `$EXIT_CODE` — as a trailing decimal line, the
/// same observable the Ruby glue's `rescue Prog::Rt::Exit` produces. A case
/// that never calls `proc_exit` just falls off the end of `_start`, so
/// nothing is appended and the script exits 0.
const BASH_FS_GLUE: &str = r#"WASI_DIRS=('{host}::{guest}')
prog_init || { echo "init failed" >&2; exit 1; }
prog_invoke '_start'
status=$?
if [[ $status -eq 133 ]]; then
  echo "$EXIT_CODE"
fi
exit 0
"#;

/// The root-preopen containment probe's glue: preopen the filesystem root at
/// guest `/` and call the WASI resolver directly (`wasi_resolve_path <p>
/// <dirfd> <path> <follow>`, ADR-32) instead of running a guest — the bash
/// analogue of Ruby's `wasi.send(:resolve_path, ...)`. `follow=1` matches
/// Ruby's `resolve_path`'s `follow_last: true` default.
const BASH_CONTAINMENT_GLUE: &str = r#"WASI_DIRS=('/::/')
prog_init || { echo "init failed" >&2; exit 1; }
wasi_resolve_path prog_ 3 etc 1
if [[ $R0 -eq 0 ]]; then
  echo "contained"
else
  echo "rejected"
fi
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
wasi_suite!(Bash, Poll);
wasi_suite!(Bash, Fs, BASH_FS_GLUE);
wasi_root_containment_e2e!(Bash, BASH_CONTAINMENT_GLUE);
standalone_dir_e2e!(Bash);

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
// No fs_apps / capi / multi-module macros: Bash has no WASI filesystem and no
// host-language object model to plumb a C API or nested runtimes through
// (ADR-12).
