//! Bash end-to-end suites (ADR-27): the shared standalone / library / WASI /
//! apps case tables (`dewasm-test-helper`) wired up for the Bash backend.
//! Bash has no WASI filesystem support (ADR-12), so there is no
//! `wasi_suite!(Fs)` invocation; the library glue is Bash function calls over
//! the R0.. result globals (ADR-11).

use std::path::PathBuf;

use dewasm_backend::Backend;
use dewasm_backend_bash::{find_bash5, BashBackend};
use dewasm_test_helper::{
    apps_e2e, library_e2e, standalone_e2e, wasi_suite, BackendUnderTest, LibraryCase,
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

    // Bash's softfloat makes QuickJS/SQLite take tens of seconds, so the heavy
    // apps cases only run under DEWASM_APPS_ALL.
    fn run_heavy_apps(&self) -> bool {
        false
    }
}

/// Per-case Bash glue: results come back through the R0 global (ADR-11). A
/// case Bash is wired to run but has no glue for panics loudly (ADR-15).
fn bash_glue(case: &LibraryCase) -> &'static str {
    match case.name {
        "add" => {
            "add_init || exit 1\n\
                  add_invoke add 2 3; echo $R0\n\
                  add_invoke add 4294967295 1; echo $R0\n\
                  add_invoke fib 10; echo $R0\n"
        }
        "wasi_import_override" => BASH_OVERRIDE_GLUE,
        other => panic!("{other}: no bash glue"),
    }
}

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

standalone_e2e!(Bash);
library_e2e!(Bash, bash_glue);
wasi_suite!(Bash, Stdio);
wasi_suite!(Bash, ArgsEnv);
apps_e2e!(Bash);
