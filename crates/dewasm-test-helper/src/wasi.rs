//! The project's own WASI preview-1 conformance suite (ADR-27): WASI has no
//! official testsuite, so the WASI-exercising fixtures are grouped by feature
//! unit — stdio, args/env, clock/random, filesystem. Each backend crate wires
//! up exactly the kinds it supports via `wasi_suite!`.
//!
//! Two execution shapes share one table:
//!   * `WasiCheck::Standalone` — a whole-program standalone run checked by
//!     stdout + exit code (stdio, args/env, clock/random). No glue; every
//!     backend runs these.
//!   * `WasiCheck::Fs` — a library-mode run against a preopened host scratch
//!     directory, with host-side setup before and assertions after. Needs
//!     per-backend instantiation glue (only Ruby has WASI filesystem support
//!     today, ADR-14), passed to the runner.

use std::path::Path;

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::fixtures::{convert, examples_dir};

/// The WASI p1 feature units a fixture exercises. Public API of the helper
/// crate, so unused variants are not dead code — a backend selects the kinds
/// it supports at `wasi_suite!` sites.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WasiKind {
    Stdio,
    ArgsEnv,
    ClockRandom,
    Fs,
}

pub struct WasiCase {
    pub name: &'static str,
    pub wat: &'static str,
    pub kind: WasiKind,
    pub args: &'static [&'static str],
    pub stdin: &'static str,
    pub check: WasiCheck,
}

pub enum WasiCheck {
    /// Whole-program standalone run: exact stdout + exit code.
    Standalone { stdout: &'static str, code: i32 },
    /// Library-mode filesystem run against a preopened scratch directory.
    Fs {
        /// Preopen a subdirectory of the scratch root instead of the root
        /// itself, so a canary can sit *outside* the sandbox (the escape
        /// test). `setup`/`assert_host`/glue all receive the preopened dir;
        /// its `.parent()` reaches the scratch root.
        preopen_subdir: Option<&'static str>,
        /// Prepare the scratch layout before the run (create a symlink, drop
        /// a canary file, ...). Receives the preopened dir.
        setup: fn(&Path),
        /// Check the run's captured stdout (exact match vs. `contains` is the
        /// closure's own business — the exact original assertion).
        check_stdout: fn(&str),
        /// Assert host filesystem state after the run. Receives the
        /// preopened dir.
        assert_host: fn(&Path),
        /// Restrict to unix (the symlink fixture); skipped elsewhere the way
        /// `#[cfg(unix)]` used to compile it out.
        unix_only: bool,
    },
}

/// A fresh, empty scratch directory keyed by `name`, so cases running in
/// parallel never share host state.
fn scratch_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("dewasm-wasi-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub const WASI_CASES: &[WasiCase] = &[
    // Stdio: fd_write to stdout.
    WasiCase {
        name: "hello",
        wat: "hello.wat",
        kind: WasiKind::Stdio,
        args: &[],
        stdin: "",
        check: WasiCheck::Standalone {
            stdout: "Hello, WASI!\n",
            code: 0,
        },
    },
    // Args/env: argc (program name + arguments) becomes the exit code via
    // args_sizes_get + proc_exit.
    WasiCase {
        name: "argc",
        wat: "args_proc_exit.wat",
        kind: WasiKind::ArgsEnv,
        args: &["foo", "bar"],
        stdin: "",
        check: WasiCheck::Standalone {
            stdout: "",
            code: 3,
        },
    },
    // Filesystem (ADR-14). path_open (create+write, then reopen+read)
    // round-trips real file content through a preopened host directory.
    WasiCase {
        name: "fs_path_open_roundtrip",
        wat: "wasi_path_open_roundtrip.wat",
        kind: WasiKind::Fs,
        args: &[],
        stdin: "",
        check: WasiCheck::Fs {
            preopen_subdir: None,
            setup: |_dir| {},
            check_stdout: |out| assert_eq!(out, "hello, wasi fs!"),
            assert_host: |dir| {
                assert_eq!(
                    std::fs::read_to_string(dir.join("hello.txt")).unwrap(),
                    "hello, wasi fs!"
                )
            },
            unix_only: false,
        },
    },
    // path_create_directory + fd_readdir + path_unlink_file /
    // path_remove_directory, verified from both sides: the dirent listing the
    // guest saw, and the host filesystem after cleanup.
    WasiCase {
        name: "fs_mkdir_readdir_unlink",
        wat: "wasi_mkdir_readdir_unlink.wat",
        kind: WasiKind::Fs,
        args: &[],
        stdin: "",
        check: WasiCheck::Fs {
            preopen_subdir: None,
            setup: |_dir| {},
            check_stdout: |out| {
                assert!(
                    out.contains("sub"),
                    "expected \"sub\" in dirent listing: {out:?}"
                )
            },
            assert_host: |dir| assert!(!dir.join("sub").exists(), "sub/ should have been removed"),
            unix_only: false,
        },
    },
    // path_open with oflags::DIRECTORY on a missing path is ENOENT (44), not
    // ENOTDIR (54) — guests (e.g. wasi-libc's opendir) branch on the
    // difference. The fixture exits with the errno.
    WasiCase {
        name: "fs_dir_open_missing",
        wat: "wasi_dir_open_missing.wat",
        kind: WasiKind::Fs,
        args: &[],
        stdin: "",
        check: WasiCheck::Fs {
            preopen_subdir: None,
            setup: |_dir| {},
            check_stdout: |out| assert_eq!(out, "44\n"),
            assert_host: |_dir| {},
            unix_only: false,
        },
    },
    // path_filestat_get without SYMLINK_FOLLOW stats the symlink itself
    // (filetype 7), not its target: resolution must not follow the final
    // component. The fixture exits with the reported filetype.
    WasiCase {
        name: "fs_filestat_nofollow_symlink",
        wat: "wasi_filestat_nofollow.wat",
        kind: WasiKind::Fs,
        args: &[],
        stdin: "",
        check: WasiCheck::Fs {
            preopen_subdir: None,
            setup: |dir| {
                #[cfg(unix)]
                {
                    std::fs::write(dir.join("file"), "x").unwrap();
                    std::os::unix::fs::symlink("file", dir.join("link")).unwrap();
                }
                #[cfg(not(unix))]
                let _ = dir;
            },
            check_stdout: |out| assert_eq!(out, "7\n"),
            assert_host: |_dir| {},
            unix_only: true,
        },
    },
    // A `..`-escaping guest path must be rejected (ERRNO_NOTCAPABLE, not a
    // host filesystem escape): a canary file sitting just outside the
    // preopened directory must stay unreadable and untouched. The scratch
    // dir is a `sandbox` subdir; the canary sits beside it.
    WasiCase {
        name: "fs_escape_rejected",
        wat: "wasi_escape_rejected.wat",
        kind: WasiKind::Fs,
        args: &[],
        stdin: "",
        check: WasiCheck::Fs {
            preopen_subdir: Some("sandbox"),
            setup: |sandbox| {
                let canary_dir = sandbox.parent().unwrap().join("escape-canary");
                std::fs::create_dir_all(&canary_dir).unwrap();
                std::fs::write(canary_dir.join("canary.txt"), "secret").unwrap();
            },
            check_stdout: |out| assert_eq!(out, "BLOCKED\n"),
            assert_host: |sandbox| {
                let canary = sandbox
                    .parent()
                    .unwrap()
                    .join("escape-canary")
                    .join("canary.txt");
                assert_eq!(std::fs::read_to_string(canary).unwrap(), "secret");
            },
            unix_only: false,
        },
    },
];

/// Run the `WasiCheck::Standalone` cases of `kind` (stdio, args/env,
/// clock/random): convert standalone, run, check stdout + exit code. Works
/// for every backend; no glue.
pub fn run_wasi_standalone(lang: &dyn BackendUnderTest, kind: WasiKind) {
    for case in WASI_CASES.iter().filter(|c| c.kind == kind) {
        let WasiCheck::Standalone { stdout, code } = case.check else {
            panic!(
                "{}: {kind:?} case must use WasiCheck::Standalone",
                case.name
            );
        };
        let src = convert(
            lang.backend(),
            &examples_dir().join(case.wat),
            Mode::Standalone,
            case.name,
        );
        let output = lang.run(&src, case.args, case.stdin);
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            stdout,
            "{} under {}: stdout",
            case.name,
            lang.name()
        );
        assert_eq!(
            output.status.code(),
            Some(code),
            "{} under {}: exit code",
            case.name,
            lang.name()
        );
    }
}

/// Glue for a filesystem case: given the case and the preopened host
/// directory, produce the (backend-specific) instantiation-and-run source
/// appended to the converted module. It must preopen `host` at guest `/`,
/// invoke `_start`, and surface a `proc_exit` code as a trailing decimal line
/// (the fixtures either print to stdout or exit with a code).
pub type WasiFsGlue = fn(&WasiCase, &Path) -> String;

/// Run the `WasiCheck::Fs` cases: create a scratch dir, run the host-side
/// setup, convert in library mode, append `glue`, run, then apply the case's
/// stdout check and host-state assertions.
pub fn run_wasi_fs(lang: &dyn BackendUnderTest, glue: WasiFsGlue) {
    for case in WASI_CASES.iter().filter(|c| c.kind == WasiKind::Fs) {
        let WasiCheck::Fs {
            preopen_subdir,
            setup,
            check_stdout,
            assert_host,
            unix_only,
        } = case.check
        else {
            panic!("{}: Fs case must use WasiCheck::Fs", case.name);
        };
        if unix_only && !cfg!(unix) {
            continue;
        }
        let root = scratch_dir(case.name);
        let dir = match preopen_subdir {
            Some(sub) => {
                let d = root.join(sub);
                std::fs::create_dir_all(&d).unwrap();
                d
            }
            None => root,
        };
        setup(&dir);
        let src = convert(
            lang.backend(),
            &examples_dir().join(case.wat),
            Mode::Library,
            "prog",
        );
        let output = lang.run(
            &format!("{src}\n{}", glue(case, &dir)),
            case.args,
            case.stdin,
        );
        assert!(
            output.status.success(),
            "{} under {}: failed: {}\n{}",
            case.name,
            lang.name(),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        check_stdout(&String::from_utf8_lossy(&output.stdout));
        assert_host(&dir);
    }
}
