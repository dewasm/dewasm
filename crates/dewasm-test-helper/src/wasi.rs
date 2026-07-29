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
//!     per-backend instantiation glue: a single template string per backend
//!     (`wasi_suite!(Lang, Fs, TEMPLATE)`) whose `{guest}`/`{host}`
//!     placeholders the runner fills with the preopen pair (ADR-27 revision).
//!     The one case that does not fit the template — the root-preopen
//!     containment probe, which calls the WASI resolver directly rather than
//!     running a guest — is dissolved into its own `wasi_root_containment_e2e!`
//!     macro with its own glue const.

use std::path::Path;

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::fixtures::{convert, examples_dir};
use crate::glue::fill;

/// The WASI p1 feature units a fixture exercises. Public API of the helper
/// crate, so unused variants are not dead code — a backend selects the kinds
/// it supports at `wasi_suite!` sites.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WasiKind {
    Stdio,
    ArgsEnv,
    ClockRandom,
    /// poll_oneoff. A whole-program standalone kind like the others, but split
    /// out because bash resolves poll_oneoff to ENOSYS (ADR-12/ADR-14) and so
    /// does not wire this kind; the four backends that implement it do.
    Poll,
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
    // poll_oneoff: a clock subscription (relative 1 ms) plus an fd_write
    // readiness subscription on stdout. stdout is always ready, so the call
    // reports at least one event with error 0 and the module prints its
    // success line without blocking on the timer.
    WasiCase {
        name: "poll_oneoff",
        wat: "wasi_poll_oneoff.wat",
        kind: WasiKind::Poll,
        args: &[],
        stdin: "",
        check: WasiCheck::Standalone {
            stdout: "poll ok\n",
            code: 0,
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

/// Run the `WasiCheck::Fs` cases: create a scratch dir, run the host-side
/// setup, convert in library mode, append `template` with its `{guest}`/`{host}`
/// placeholders filled (guest `/`, host the preopened dir), run, then apply the
/// case's stdout check and host-state assertions. Every non-containment Fs case
/// shares the one template (preopen the host dir at `/`, run `_start`, surface a
/// `proc_exit` code as a trailing decimal line); the containment probe is a
/// separate case (see [`run_wasi_containment`]).
pub fn run_wasi_fs(lang: &dyn BackendUnderTest, template: &str) {
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
        let glue = fill(
            template,
            &[("guest", "/"), ("host", &dir.to_string_lossy())],
        );
        let output = lang.run(&format!("{src}\n{glue}"), case.args, case.stdin);
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

/// Exercise the standalone runtime interface's `--dir` end to end (ADR-31):
/// convert `wasi_standalone_dir.wat` in *standalone* mode, run it with a
/// `--dir HOST::GUEST` mount of a fresh scratch dir at guest `/`, and require
/// the guest to round-trip a file through it — the echoed stdout and the host
/// file the guest wrote must both be correct. Shared by the four filesystem
/// backends and re-run under wasmtime as ground truth (its `run_standalone_dir`
/// override consumes `--dir` as a host flag). No glue: standalone needs none.
pub fn run_standalone_dir(lang: &dyn BackendUnderTest) {
    let scratch = scratch_dir(&format!("standalone-dir-{}", lang.name()));
    let bytes = wat::parse_file(examples_dir().join("wasi_standalone_dir.wat")).expect("parse wat");
    let program = lang.convert_app(&bytes, Mode::Standalone, "standalone_dir");
    let output = lang.run_standalone_dir(&program, &[("/", scratch.as_path())], &[], b"");
    assert!(
        output.status.success(),
        "standalone --dir under {}: nonzero exit {}\n{}",
        lang.name(),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "hello, wasi fs!",
        "standalone --dir under {}: stdout differs from the wasmtime ground truth",
        lang.name()
    );
    assert_eq!(
        std::fs::read_to_string(scratch.join("hello.txt")).unwrap_or_default(),
        "hello, wasi fs!",
        "standalone --dir under {}: the host file the guest wrote through the preopen is wrong",
        lang.name()
    );
    println!(
        "standalone --dir under {}: round-trips through the preopen",
        lang.name()
    );
}

/// Run the deep-recursion standalone case (`deep_recursion_e2e!`): convert
/// `deep_recursion.wat` — whose `_start` recurses 5000 wasm frames, far past
/// e.g. CPython's default ~1000-frame recursion limit — in *standalone* mode
/// and run it with no arguments. The generated entrypoint must survive the
/// recursion (Python: ADR-28's raised recursion limit plus a big-stack guest
/// thread) and still surface the guest's `proc_exit(42)` as the process exit
/// code (ADR-31). Like `run_standalone_dir`, this exercises the emitted
/// entrypoint itself, so no glue.
pub fn run_deep_recursion(lang: &dyn BackendUnderTest) {
    let src = convert(
        lang.backend(),
        &examples_dir().join("deep_recursion.wat"),
        Mode::Standalone,
        "deep_recursion",
    );
    let output = lang.run(&src, &[], "");
    assert_eq!(
        output.status.code(),
        Some(42),
        "deep_recursion under {}: exit code\n{}",
        lang.name(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "",
        "deep_recursion under {}: stdout",
        lang.name()
    );
}

/// Run the root-preopen containment probe (`wasi_root_containment_e2e!`): a
/// preopen whose realpath is the filesystem root must not reject every path
/// (the containment check would otherwise build the prefix "//" and never
/// match). This exercises a WASI-model *internal* (the path-resolution helper)
/// rather than a guest fixture — no host files are touched — so `glue` probes
/// the resolver directly with a `"/" => "/"` preopen instead of running the
/// converted module's `_start`. `wasi_path_open_roundtrip.wat` is converted only
/// to bring the runtime's WASI class into scope; `glue` normalizes the outcome
/// to `contained`. Unix-only: a realpath of `/` is a unix notion, so it is a
/// no-op elsewhere (the way `#[cfg(unix)]` used to compile it out).
pub fn run_wasi_containment(lang: &dyn BackendUnderTest, glue: &str) {
    if !cfg!(unix) {
        return;
    }
    let src = convert(
        lang.backend(),
        &examples_dir().join("wasi_path_open_roundtrip.wat"),
        Mode::Library,
        "prog",
    );
    let output = lang.run(&format!("{src}\n{glue}"), &[], "");
    assert!(
        output.status.success(),
        "fs_root_preopen_containment under {}: failed: {}\n{}",
        lang.name(),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "contained\n",
        "fs_root_preopen_containment under {}: stdout",
        lang.name()
    );
}
