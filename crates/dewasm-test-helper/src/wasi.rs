//! The project's own WASI preview-1 conformance suite.
//! WASI has no official testsuite, so the WASI-exercising fixtures are grouped by feature unit.
//! The units are stdio, arguments/environment, clock/random, and file system.
//! Each backend crate selects exactly the kinds it supports via `wasi_suite!`.
//!
//! Two execution shapes share one table:
//!
//! * `WasiCheck::Standalone`: a whole-program standalone run checked by `stdout` + exit code.
//!   It covers stdio, arguments/environment, and clock/random.
//!   No glue; every backend runs these.
//! * `WasiCheck::Fs`: a library-mode run against a preopened host scratch directory.
//!   Host-side `setup` runs before it and assertions after.
//!   It needs per-backend instantiation glue: a single template string per backend.
//!   The template is passed as `wasi_suite!(Lang, Fs, TEMPLATE)`.
//!   The runner fills its `{guest}`/`{host}` placeholders with the preopen pair.
//!
//! One case does not fit the template: the root-preopen containment probe.
//! It calls the WASI resolver directly rather than running a guest.
//! So it has its own `wasi_root_containment_e2e!` macro with its own glue constant.

use std::path::Path;

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::fixtures::{convert, examples_dir};
use crate::glue::fill;

/// The WASI p1 feature units a fixture exercises.
/// Public API of the helper crate, so unused variants are not dead code.
/// A backend selects the kinds it supports at `wasi_suite!` sites.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WasiKind {
    Stdio,
    ArgsEnv,
    ClockRandom,
    /// `poll_oneoff` kept a separate variant so a backend without it could run the other three.
    /// Every backend currently runs it.
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
    /// Whole-program standalone run: exact `stdout` + exit code.
    Standalone { stdout: &'static str, code: i32 },
    /// Library-mode file system run against a preopened scratch directory.
    Fs {
        /// Preopen a directory inside the scratch root instead of the root itself.
        /// So a file (`canary.txt`) can sit *outside* the sandbox (the escape test).
        /// `setup`/`assert_host`/glue all receive the preopened directory.
        /// Its `.parent()` reaches the scratch root.
        preopen_subdir: Option<&'static str>,
        /// Prepare the scratch layout before the run.
        /// For example, create a symbolic link or write `canary.txt`.
        /// Receives the preopened directory.
        setup: fn(&Path),
        /// Check the run's captured `stdout`.
        /// Exact match vs. `contains` is the closure's own business, the exact original assertion.
        check_stdout: fn(&str),
        /// Assert host file system state after the run.
        /// Receives the preopened directory.
        assert_host: fn(&Path),
        /// Restrict to Unix (the symbolic link fixture); skipped elsewhere.
        unix_only: bool,
    },
}

/// A fresh, empty scratch directory keyed by `name`.
/// So cases running in parallel never share host state.
fn scratch_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("dewasm-wasi-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub const WASI_CASES: &[WasiCase] = &[
    // Stdio: `fd_write` to `stdout`.
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
    // Arguments/environment: `argc` (program name + arguments) becomes the exit code.
    // It goes through `args_sizes_get` + `proc_exit`.
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
    // `poll_oneoff`: a clock subscription (relative 1 ms) plus an `fd_write` readiness one.
    // The readiness subscription is on `stdout`, which is always ready.
    // So the call reports at least one event with error 0.
    // The module then prints its success line without blocking on the timer.
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
    // File system.
    // `path_open` (create+write, then reopen+read) round-trips real file content.
    // The content goes through a preopened host directory.
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
    // `path_create_directory` + `fd_readdir` + `path_unlink_file` / `path_remove_directory`.
    // They are verified from both sides.
    // Those are the directory entries the guest listed, and the host file system after removal.
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
    // `fd_readdir` with a `dircookie` whose high bit is set (u64 2^63).
    // The cookie is an unsigned position past any snapshot's end.
    // So the call succeeds with `bufused` 0 (matching Wasmtime).
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
    // `path_open` with `oflags::DIRECTORY` on a missing path is ENOENT (44), not ENOTDIR (54).
    // Guests (for example `wasi-libc`'s `opendir`) branch on the difference.
    // The fixture exits with the `errno`.
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
    // `path_filestat_get` without SYMLINK_FOLLOW `stat`s the symbolic link itself (`filetype` 7).
    // It does not `stat` the target.
    // Resolution must not follow the final component.
    // The fixture exits with the reported `filetype`.
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
    // A `..`-escaping guest path must be rejected with ERRNO_NOTCAPABLE.
    // It must not escape to the host file system.
    // A `canary.txt` file just outside the preopened directory must stay unreadable and untouched.
    // The preopen is a `sandbox` directory inside scratch; `escape-canary/` sits beside it.
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
    // Trailing-slash shapes (issue #42): matched to Wasmtime 47 on both hosts.
    // Probe k is Wasmtime's own host split, asserted per host.
    // The fixture header lists the shapes left unasserted.
    // The Bash-only PR #41 checks are in `crates/dewasm-backend-bash/tests/wasi_fs_regressions.rs`.
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
                // The failing probes must have left the file system alone.
                // The succeeding ones (j, m, p) must have taken effect.
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
    // The symbolic link side: `readlink` through a slash follows to the target.
    // A slash-suffixed symbolic link destination's error depends on what sits behind the slash.
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
    // Rights enforcement per file descriptor.
    // A file descriptor narrowed by `fd_fdstat_set_rights` must refuse `fd_pread`/`fd_pwrite`.
    // It refuses them with NOTCAPABLE, as it does `fd_read`/`fd_write`.
    // A directory descriptor stripped of PATH_FILESTAT_SET_SIZE must refuse an O_TRUNC open.
    // The refusal must not touch the file.
    // A directory descriptor stripped of PATH_OPEN must refuse any open.
    // The fixture prints "<tag><errno>" per probe (76 = NOTCAPABLE).
    // So the expected `stdout` checks every probe.
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

/// Run the `WasiCheck::Standalone` cases of `kind` (stdio, arguments/environment, clock/random).
/// Convert standalone, run, and check `stdout` + exit code.
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

/// Run the `WasiCheck::Fs` cases.
/// Create a scratch directory, run the host-side `setup`, and convert in library mode.
/// Append `template` with its `{guest}`/`{host}` placeholders filled.
/// The guest side is `/`, and the host side is the preopened directory.
/// Run, then apply the case's `stdout` check and host-state assertions.
/// Every non-containment Fs case shares the one template.
/// It preopens the host directory at `/`, runs `_start`, and surfaces a `proc_exit` code.
/// That code is a trailing decimal line.
/// The containment probe is a separate case (see [`run_wasi_containment`]).
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

/// Exercise the standalone runtime interface's `--dir` end to end.
/// Convert `wasi_standalone_dir.wat` in *standalone* mode.
/// Run it with a `--dir HOST::GUEST` mount of a fresh scratch directory at guest `/`.
/// Require the guest to round-trip a file through it.
/// The echoed `stdout` and the host file the guest wrote must both be correct.
/// Shared by every backend and re-run under Wasmtime as ground truth.
/// Its `run_standalone_dir` override consumes `--dir` as a host flag.
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

/// Run the deep-recursion standalone case (`deep_recursion_e2e!`).
/// Convert `deep_recursion.wat` in *standalone* mode and run it with no arguments.
/// Its `_start` recurses 5000 wasm frames.
/// That is far past for example CPython's default ~1000-frame recursion limit.
/// The generated entrypoint must survive the recursion.
/// Python does so with a raised recursion limit plus a big-stack guest thread.
/// It must still surface the guest's `proc_exit(42)` as the process exit code.
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

/// Run the root-preopen containment probe (`wasi_root_containment_e2e!`).
/// A preopen whose `realpath` is the file system root must not reject every path.
/// The containment check would otherwise build the prefix "//" and never match.
/// This exercises a WASI-model *internal* (the path-resolution helper) rather than a guest fixture.
/// No host files are touched.
/// So `glue` probes the resolver directly with a `"/" => "/"` preopen.
/// It does not run the converted module's `_start`.
/// `wasi_path_open_roundtrip.wat` is converted only to bring the runtime's WASI class into scope.
/// `glue` normalizes the outcome to `contained`.
/// On Unix only: a `realpath` of `/` is a Unix notion, so the case is a no-op elsewhere.
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
