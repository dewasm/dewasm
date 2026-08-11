//! Bash-only WASI filesystem regression pins (the issue-29 fixes, plus the issue-143 single-file preopen): drive a converted library-mode module plus its bundled units directly under bash, the same direct-drive shape as `softfloat.rs`. These cases do not join the shared `WASI_CASES` conformance table because the other backends inherit the probed errnos from the host OS (which differs between Linux and macOS on some of them) while the bash units implement each choice deterministically; the exact codes pinned here are bash's own contract (`runtime/bash/units/wasi/path_rename.sh` / `fd_close.sh` / `fd_allocate.sh` / `init_preopens.sh`).
//!
//! The permission-based cases (a read-only parent to fail `rmdir`, a read-only file to fail the close-time flush) assume a non-root test user: root ignores permission bits and would see the operations succeed.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

use dewasm_backend::Mode;
use dewasm_backend_bash::{find_bash5, BashBackend};

/// A fresh, empty scratch directory keyed by `name`, so cases running in parallel never share host state (the `wasi.rs` helper's shape).
fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dewasm-bash-wasi-reg-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Convert `wat_src` in library mode (module name `prog`, embedded runtime, bundled WASI), append `glue`, run the script under bash >= 5, and return its stdout. The script itself must exit 0 (the glue ends in `exit 0`).
fn run_module(name: &str, wat_src: &str, glue: &str) -> String {
    let bash = find_bash5().expect("bash >= 5 not found: see docs/testing.md");
    let bytes = wat::parse_str(wat_src).expect("parse wat");
    let src = dewasm_test_helper::convert_bytes(&BashBackend, &bytes, Mode::Library, "prog");
    let script_path = std::env::temp_dir().join(format!(
        "dewasm-bash-wasi-reg-{name}-{}.sh",
        std::process::id()
    ));
    std::fs::write(&script_path, format!("{src}\n{glue}")).unwrap();
    let output = Command::new(&bash)
        .arg(&script_path)
        .output()
        .expect("run bash");
    assert!(
        output.status.success(),
        "{name}: bash failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The standard `_start` glue (`e2e.rs`'s `BASH_FS_GLUE` with the preopen inlined): preopen `host` at guest `/`, run `_start`, surface a `proc_exit` code (`invoke` returns status 133 with the code in `$EXIT_CODE`) as a trailing decimal line.
fn start_glue(host: &Path) -> String {
    format!(
        r#"WASI_DIRS=('{}::/')
prog_init || {{ echo "init failed" >&2; exit 1; }}
prog_invoke '_start'
status=$?
if [[ $status -eq 133 ]]; then
  echo "$EXIT_CODE"
fi
exit 0
"#,
        host.display()
    )
}

/// A module whose `_start` calls `path_rename(3, old, 3, new)` against the first preopen and exits with the errno.
fn rename_wat(old: &str, new: &str) -> String {
    format!(
        r#"(module
  (import "wasi_snapshot_preview1" "path_rename"
    (func $path_rename (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "{old}")
  (data (i32.const 128) "{new}")
  (func (export "_start")
    (call $proc_exit
      (call $path_rename
        (i32.const 3) (i32.const 0) (i32.const {old_len})
        (i32.const 3) (i32.const 128) (i32.const {new_len})))))"#,
        old_len = old.len(),
        new_len = new.len(),
    )
}

/// A module whose `_start` opens (CREAT) the file `f` with FD_READ|FD_WRITE|FD_ALLOCATE rights, calls `fd_allocate(fd, offset, len)`, and exits with the errno.
fn allocate_wat(offset: i64, len: i64) -> String {
    format!(
        r#"(module
  (import "wasi_snapshot_preview1" "path_open"
    (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_allocate"
    (func $fd_allocate (param i32 i64 i64) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "f")
  (func (export "_start")
    (local $err i32)
    (local.set $err
      (call $path_open
        (i32.const 3)   ;; dirfd: the first preopen
        (i32.const 0)   ;; dirflags
        (i32.const 0)   ;; path ptr
        (i32.const 1)   ;; path len
        (i32.const 0x1) ;; oflags: CREAT
        (i64.const 0x142) ;; rights: FD_READ|FD_WRITE|FD_ALLOCATE
        (i64.const 0)   ;; rights inheriting
        (i32.const 0)   ;; fdflags
        (i32.const 64)))
    (if (local.get $err) (then (call $proc_exit (local.get $err))))
    (call $proc_exit
      (call $fd_allocate
        (i32.load (i32.const 64))
        (i64.const {offset})
        (i64.const {len})))))"#
    )
}

/// A trailing slash on a source that resolves to a regular file is ENOTDIR (54), and nothing moves.
/// Before the fix the slash was silently stripped and the rename succeeded.
#[test]
fn rename_trailing_slash_on_file_source_is_enotdir() {
    let dir = scratch_dir("slash-src-file");
    std::fs::write(dir.join("file"), "x").unwrap();
    let out = run_module(
        "slash-src-file",
        &rename_wat("file/", "renamed"),
        &start_glue(&dir),
    );
    assert_eq!(out, "54\n");
    assert!(dir.join("file").exists(), "source must survive");
    assert!(!dir.join("renamed").exists(), "nothing may be created");
}

/// A trailing slash on a nonexistent source is the ordinary missing-source ENOENT (44).
/// POSIX: rename("nonexistent/", "x") fails pathname resolution, not ENOTDIR.
#[test]
fn rename_trailing_slash_missing_source_is_enoent() {
    let dir = scratch_dir("slash-src-missing");
    let out = run_module(
        "slash-src-missing",
        &rename_wat("nosuch/", "x"),
        &start_glue(&dir),
    );
    assert_eq!(out, "44\n");
}

/// A trailing slash on a destination that resolves to an existing regular file is ENOTDIR (54).
/// Without the slash this would be a legal replace.
#[test]
fn rename_trailing_slash_on_file_destination_is_enotdir() {
    let dir = scratch_dir("slash-dst-file");
    std::fs::write(dir.join("src"), "s").unwrap();
    std::fs::write(dir.join("dst"), "d").unwrap();
    let out = run_module(
        "slash-dst-file",
        &rename_wat("src", "dst/"),
        &start_glue(&dir),
    );
    assert_eq!(out, "54\n");
    assert_eq!(std::fs::read_to_string(dir.join("src")).unwrap(), "s");
    assert_eq!(std::fs::read_to_string(dir.join("dst")).unwrap(), "d");
}

/// A trailing slash on a *nonexistent* destination is stripped and the rename proceeds onto the bare name, which is wasmtime's behavior. The raw hosts diverge here (macOS ENOENT / Linux ENOTDIR), hence the pin.
#[test]
fn rename_trailing_slash_missing_destination_strips_and_renames() {
    let dir = scratch_dir("slash-dst-missing");
    std::fs::write(dir.join("src"), "s").unwrap();
    let out = run_module(
        "slash-dst-missing",
        &rename_wat("src", "newdir/"),
        &start_glue(&dir),
    );
    assert_eq!(out, "0\n");
    assert!(!dir.join("src").exists(), "source must move");
    assert_eq!(
        std::fs::read_to_string(dir.join("newdir")).unwrap(),
        "s",
        "the bare name receives the file"
    );
}

/// Trailing slashes on directories keep working: rename("d1/", "d2/") with a directory source and a nonexistent destination succeeds.
#[test]
fn rename_trailing_slash_on_directories_still_renames() {
    let dir = scratch_dir("slash-dirs-ok");
    std::fs::create_dir(dir.join("d1")).unwrap();
    let out = run_module(
        "slash-dirs-ok",
        &rename_wat("d1/", "d2/"),
        &start_glue(&dir),
    );
    assert_eq!(out, "0\n");
    assert!(!dir.join("d1").exists(), "source must be gone");
    assert!(dir.join("d2").is_dir(), "destination must be the directory");
}

/// When the `rmdir` clearing an empty destination directory fails (here: its parent is read-only), path_rename must report the failure.
/// Before the fix the unchecked `rmdir` was followed by an unconditional `mv`, which nested the source *inside* the surviving destination and reported success. The destination is still empty afterwards, so the errno is the unit's default EIO (29); a destination that gained entries would be ENOTEMPTY (55), but that race cannot be staged deterministically here.
#[test]
fn rename_rmdir_failure_is_reported_and_moves_nothing() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch_dir("rmdir-failure");
    std::fs::create_dir(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("ro/dst")).unwrap();
    std::fs::set_permissions(dir.join("ro"), std::fs::Permissions::from_mode(0o555)).unwrap();
    let out = run_module(
        "rmdir-failure",
        &rename_wat("src", "ro/dst"),
        &start_glue(&dir),
    );
    // Restore before asserting so a later scratch cleanup can delete the tree.
    std::fs::set_permissions(dir.join("ro"), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(out, "29\n");
    assert!(dir.join("src").is_dir(), "source must survive");
    assert!(dir.join("ro/dst").is_dir(), "destination must survive");
    assert!(
        !dir.join("ro/dst/src").exists(),
        "the source must not be nested inside the surviving destination"
    );
}

/// offset+len past i64::MAX must not wrap bash's signed-64 arithmetic into a silent no-op success: it is EIO (29), the code Ruby/Python surface when the host refuses an unsatisfiable size (their unmapped EFBIG falls through to ERRNO_IO).
#[test]
fn fd_allocate_overflowing_size_is_eio() {
    let dir = scratch_dir("alloc-overflow");
    let out = run_module(
        "alloc-overflow",
        &allocate_wat(0x4000_0000_0000_0000, 0x4000_0000_0000_0000),
        &start_glue(&dir),
    );
    assert_eq!(out, "29\n");
}

/// A negative (u64 >= 2^63) len keeps its existing EINVAL (28): the overflow guard must not shadow the sign check.
#[test]
fn fd_allocate_negative_len_is_einval() {
    let dir = scratch_dir("alloc-negative");
    let out = run_module("alloc-negative", &allocate_wat(0, -1), &start_glue(&dir));
    assert_eq!(out, "28\n");
}

/// A flush failure at close (the buffered file was made unwritable between the write and the close) must surface its errno.
/// Before the fix fd_close unconditionally reported 0 and the guest never learned its writes were lost. The fd state must still be released: a second close is EBADF (8).
#[test]
fn fd_close_flush_failure_propagates_errno_and_releases_fd() {
    let dir = scratch_dir("close-flush-failure");
    let wat = r#"(module
  (import "wasi_snapshot_preview1" "path_open"
    (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_close"
    (func $fd_close (param i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "f")
  (data (i32.const 8) "hi")
  ;; Open (CREAT) /f for writing, buffer two bytes (marking it dirty), and
  ;; return the fd. The file itself is created eagerly at open time.
  (func (export "openwrite") (result i32)
    (local $fd i32)
    (drop (call $path_open
      (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 1)
      (i32.const 0x1)  ;; oflags: CREAT
      (i64.const 0x42) ;; rights: FD_READ|FD_WRITE
      (i64.const 0) (i32.const 0) (i32.const 64)))
    (local.set $fd (i32.load (i32.const 64)))
    (i32.store (i32.const 32) (i32.const 8)) ;; iovec: ptr
    (i32.store (i32.const 36) (i32.const 2)) ;; iovec: len
    (drop (call $fd_write (local.get $fd) (i32.const 32) (i32.const 1) (i32.const 40)))
    (local.get $fd))
  (func (export "closefd") (param i32) (result i32)
    (call $fd_close (local.get 0))))"#;
    // Glue: open+write, then make the file unwritable *behind the buffer* so the close-time flush fails (file_flush's truncate-open reports EIO, 29), then close twice: the second close must see a released fd (EBADF, 8).
    let glue = format!(
        r#"WASI_DIRS=('{host}::/')
prog_init || {{ echo "init failed" >&2; exit 1; }}
prog_invoke openwrite
fd=$R0
command chmod 444 '{host}/f'
prog_invoke closefd "$fd"
echo "$R0"
prog_invoke closefd "$fd"
echo "$R0"
exit 0
"#,
        host = dir.display()
    );
    let out = run_module("close-flush-failure", wat, &glue);
    assert_eq!(out, "29\n8\n");
}

/// A preopen whose host path is a *file*, not a directory, is accepted, and the `"."` wasi-libc addresses that preopen with resolves back to the file itself: `path_filestat_get(3, ".")` is 0, not an errno. This is the shape the zeroperl reactor's mandatory `/dev/null` preopen takes (issue #143); before the fix `cd -P` could not enter a non-directory, so init failed outright and the `"."` was ENOENT. Ruby and Perl inherit this from `File.realpath`/`Cwd::realpath`, which resolve `"/dev/null/."` to `/dev/null` already; bash's own `cd -P` resolution is why it needed pinning here.
#[test]
fn single_file_preopen_resolves_dot_to_itself() {
    let dir = scratch_dir("file-preopen");
    let file = dir.join("only");
    std::fs::write(&file, "x").unwrap();
    let wat = r#"(module
  (import "wasi_snapshot_preview1" "path_filestat_get"
    (func $path_filestat_get (param i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) ".")
  (func (export "_start")
    (call $proc_exit
      (call $path_filestat_get
        (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 1) (i32.const 64)))))"#;
    let out = run_module("file-preopen", wat, &start_glue(&file));
    assert_eq!(out, "0\n");
}
