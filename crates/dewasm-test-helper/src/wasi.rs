//! The project's own WASI preview-1 conformance suite: WASI has no official testsuite, so the WASI-exercising fixtures are grouped by feature unit: stdio, args/env, clock/random, filesystem.
//! Each backend crate wires up exactly the kinds it supports via `wasi_suite!`.
//!
//! Two execution shapes share one table: * `WasiCheck::Standalone`: a whole-program standalone run checked by stdout + exit code (stdio, args/env, clock/random).
//! No glue; every backend runs these. * `WasiCheck::Fs`: a library-mode run against a preopened host scratch directory, with host-side setup before and assertions after.
//! Needs per-backend instantiation glue: a single template string per backend (`wasi_suite!(Lang, Fs, TEMPLATE)`) whose `{guest}`/`{host}` placeholders the runner fills with the preopen pair.
//! The one case that does not fit the template (the root-preopen containment probe, which calls the WASI resolver directly rather than running a guest) is dissolved into its own `wasi_root_containment_e2e!` macro with its own glue const.

use std::path::Path;

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::fixtures::{convert, examples_dir};
use crate::glue::fill;

/// The WASI p1 feature units a fixture exercises.
/// Public API of the helper crate, so unused variants are not dead code: a backend selects the kinds it supports at `wasi_suite!` sites.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WasiKind {
    Stdio,
    ArgsEnv,
    ClockRandom,
    /// poll_oneoff kept a separate variant so a backend without it could wire the other three; every backend currently wires it.
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
        /// Preopen a subdirectory of the scratch root instead of the root itself, so a canary can sit *outside* the sandbox (the escape test).
        /// `setup`/`assert_host`/glue all receive the preopened dir; its `.parent()` reaches the scratch root.
        preopen_subdir: Option<&'static str>,
        /// Prepare the scratch layout before the run (create a symlink, drop a canary file, ...).
        /// Receives the preopened dir.
        setup: fn(&Path),
        /// Check the run's captured stdout (exact match vs. `contains` is the closure's own business, the exact original assertion).
        check_stdout: fn(&str),
        /// Assert host filesystem state after the run.
        /// Receives the preopened dir.
        assert_host: fn(&Path),
        /// Restrict to unix (the symlink fixture); skipped elsewhere.
        unix_only: bool,
    },
}

/// A fresh, empty scratch directory keyed by `name`, so cases running in parallel never share host state.
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
    // Args/env: argc (program name + arguments) becomes the exit code via args_sizes_get + proc_exit.
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
    // poll_oneoff: a clock subscription (relative 1 ms) plus an fd_write readiness subscription on stdout. stdout is always ready, so the call reports at least one event with error 0 and the module prints its success line without blocking on the timer.
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
    // Filesystem. path_open (create+write, then reopen+read) round-trips real file content through a preopened host directory.
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
    // path_create_directory + fd_readdir + path_unlink_file / path_remove_directory, verified from both sides: the dirent listing the guest saw, and the host filesystem after cleanup.
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
    // fd_readdir with a dircookie whose high bit is set (u64 2^63): the cookie is an unsigned position past any snapshot's end, so the call succeeds with bufused 0 (matching wasmtime).
    // Runtimes holding the cookie in a signed type must not index negatively (issue #27).
    WasiCase {
        name: "fs_readdir_high_cookie",
        wat: "wasi_readdir_high_cookie.wat",
        kind: WasiKind::Fs,
        args: &[],
        stdin: "",
        check: WasiCheck::Fs {
            preopen_subdir: None,
            setup: |dir| std::fs::write(dir.join("entry.txt"), "x").unwrap(),
            check_stdout: |out| assert_eq!(out, "past-end ok\n"),
            assert_host: |_dir| {},
            unix_only: false,
        },
    },
    // path_open with oflags::DIRECTORY on a missing path is ENOENT (44), not ENOTDIR (54): guests (e.g. wasi-libc's opendir) branch on the difference.
    // The fixture exits with the errno.
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
    // path_filestat_get without SYMLINK_FOLLOW stats the symlink itself (filetype 7), not its target: resolution must not follow the final component.
    // The fixture exits with the reported filetype.
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
    // A `..`-escaping guest path must be rejected (ERRNO_NOTCAPABLE, not a host filesystem escape): a canary file sitting just outside the preopened directory must stay unreadable and untouched.
    // The scratch dir is a `sandbox` subdir; the canary sits beside it.
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
    // Trailing-slash shapes (issue #42): pinned to wasmtime 47 on both hosts.
    // Probe k is wasmtime's own host split, asserted per host; the fixture header lists the unpinned shapes.
    // The bash-only PR #41 pins live in crates/dewasm-backend-bash/tests/wasi_fs_regressions.rs.
    WasiCase {
        name: "fs_trailing_slash",
        wat: "wasi_trailing_slash.wat",
        kind: WasiKind::Fs,
        args: &[],
        stdin: "",
        check: WasiCheck::Fs {
            preopen_subdir: None,
            setup: |dir| {
                std::fs::write(dir.join("file"), "keep me").unwrap();
                std::fs::write(dir.join("file2"), "also kept").unwrap();
                std::fs::create_dir(dir.join("dir")).unwrap();
            },
            check_stdout: |out| {
                let k = if cfg!(target_os = "macos") {
                    "k28"
                } else {
                    "k31"
                };
                assert_eq!(
                    out,
                    format!(
                        "a54\nb54\nc54\nd44\ne54\nf54\ng54\nh54\ni44\nj00\n{k}\nl20\nm00\nn44\no28\np00\n"
                    )
                )
            },
            assert_host: |dir| {
                // The failing probes must have left the filesystem alone; the succeeding ones (j, m, p) must have taken effect.
                assert_eq!(
                    std::fs::read_to_string(dir.join("file")).unwrap(),
                    "keep me"
                );
                assert_eq!(
                    std::fs::read_to_string(dir.join("newd")).unwrap(),
                    "also kept",
                    "j must rename file2 to a plain file newd (wasmtime strips the slash)"
                );
                assert!(!dir.join("file2").exists(), "j must move file2 away");
                assert!(!dir.join("renamed").exists(), "a must not rename");
                assert!(!dir.join("lnk").exists(), "h must not link");
                assert!(!dir.join("newf").exists(), "k must not create a file");
                assert!(dir.join("newdir").is_dir(), "m must create newdir/");
                assert!(!dir.join("dir").exists(), "o must keep dir, p removes it");
            },
            unix_only: false,
        },
    },
    // The symlink side: readlink through a slash follows to the target; a slash-suffixed symlink destination errs by what sits behind the slash.
    WasiCase {
        name: "fs_trailing_slash_symlink",
        wat: "wasi_trailing_slash_symlink.wat",
        kind: WasiKind::Fs,
        args: &[],
        stdin: "",
        check: WasiCheck::Fs {
            preopen_subdir: None,
            setup: |dir| {
                #[cfg(unix)]
                {
                    std::fs::write(dir.join("file"), "x").unwrap();
                    std::os::unix::fs::symlink("file", dir.join("linkfile")).unwrap();
                    std::fs::create_dir(dir.join("sd1")).unwrap();
                }
                #[cfg(not(unix))]
                let _ = dir;
            },
            check_stdout: |out| assert_eq!(out, "t54\nu20\nv54\nw44\n"),
            assert_host: |dir| {
                assert!(
                    dir.join("linkfile").symlink_metadata().is_ok(),
                    "the probes must not remove the link"
                );
                assert!(
                    dir.join("dang").symlink_metadata().is_err(),
                    "w must not create a symlink"
                );
            },
            unix_only: true,
        },
    },
    // Per-fd rights enforcement.
    // A fd narrowed by fd_fdstat_set_rights must refuse fd_pread/fd_pwrite (NOTCAPABLE, like fd_read/fd_write); a dirfd stripped of PATH_FILESTAT_SET_SIZE must refuse an O_TRUNC open without touching the file; and one stripped of PATH_OPEN must refuse any open.
    // The fixture prints "<tag><errno>" per probe (76 = NOTCAPABLE), so the expected stdout pins every probe.
    WasiCase {
        name: "fs_rights_notcapable",
        wat: "wasi_rights_notcapable.wat",
        kind: WasiKind::Fs,
        args: &[],
        stdin: "",
        check: WasiCheck::Fs {
            preopen_subdir: None,
            setup: |dir| std::fs::write(dir.join("data.txt"), "keep me").unwrap(),
            check_stdout: |out| assert_eq!(out, "a00\nb00\np76\nw76\nc00\nt76\nd00\no76\n"),
            assert_host: |dir| {
                assert_eq!(
                    std::fs::read_to_string(dir.join("data.txt")).unwrap(),
                    "keep me",
                    "O_TRUNC without PATH_FILESTAT_SET_SIZE must not truncate"
                )
            },
            unix_only: false,
        },
    },
];

/// Run the `WasiCheck::Standalone` cases of `kind` (stdio, args/env, clock/random): convert standalone, run, check stdout + exit code.
/// Works for every backend; no glue.
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

/// Run the `WasiCheck::Fs` cases: create a scratch dir, run the host-side setup, convert in library mode, append `template` with its `{guest}`/`{host}` placeholders filled (guest `/`, host the preopened dir), run, then apply the case's stdout check and host-state assertions.
/// Every non-containment Fs case shares the one template (preopen the host dir at `/`, run `_start`, surface a `proc_exit` code as a trailing decimal line); the containment probe is a separate case (see [`run_wasi_containment`]).
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
            &lang.module_name("prog"),
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

/// Exercise the standalone runtime interface's `--dir` end to end: convert `wasi_standalone_dir.wat` in *standalone* mode, run it with a `--dir HOST::GUEST` mount of a fresh scratch dir at guest `/`, and require the guest to round-trip a file through it: the echoed stdout and the host file the guest wrote must both be correct.
/// Shared by every backend and re-run under wasmtime as ground truth (its `run_standalone_dir` override consumes `--dir` as a host flag).
/// No glue: standalone needs none.
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

/// Run the deep-recursion standalone case (`deep_recursion_e2e!`): convert `deep_recursion.wat` (whose `_start` recurses 5000 wasm frames, far past e.g. CPython's default ~1000-frame recursion limit) in *standalone* mode and run it with no arguments.
/// The generated entrypoint must survive the recursion (Python: a raised recursion limit plus a big-stack guest thread) and still surface the guest's `proc_exit(42)` as the process exit code.
/// Like `run_standalone_dir`, this exercises the emitted entrypoint itself, so no glue.
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

/// Run the root-preopen containment probe (`wasi_root_containment_e2e!`): a preopen whose realpath is the filesystem root must not reject every path (the containment check would otherwise build the prefix "//" and never match).
/// This exercises a WASI-model *internal* (the path-resolution helper) rather than a guest fixture (no host files are touched), so `glue` probes the resolver directly with a `"/" => "/"` preopen instead of running the converted module's `_start`.
/// `wasi_path_open_roundtrip.wat` is converted only to bring the runtime's WASI class into scope; `glue` normalizes the outcome to `contained`.
/// Unix-only: a realpath of `/` is a unix notion, so it is a no-op elsewhere.
pub fn run_wasi_containment(lang: &dyn BackendUnderTest, glue: &str) {
    if !cfg!(unix) {
        return;
    }
    let src = convert(
        lang.backend(),
        &examples_dir().join("wasi_path_open_roundtrip.wat"),
        Mode::Library,
        &lang.module_name("prog"),
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
