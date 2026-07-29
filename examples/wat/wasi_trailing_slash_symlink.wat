;; Trailing slashes on the symlink-flavored syscalls (issue #42, ADR-49:
;; follow wasmtime — every expected errno measured against wasmtime 47 on
;; both macOS and Linux, where they agree). Host layout (from the case's
;; setup): a regular file "file", a symlink "linkfile" -> "file", and a
;; directory "sd1". Probes print "<tag><errno>\n" like
;; wasi_trailing_slash.wat:
;;   t — path_readlink of "linkfile/": the slash forces following, and the
;;       target is a file (54 ENOTDIR) — never the link content,
;;   u — path_symlink onto "sd1/" (existing directory): 20 EEXIST,
;;   v — path_symlink onto "file/" (existing non-directory): 54 ENOTDIR —
;;       wasmtime's resolution order, where a raw Linux symlinkat(2) would
;;       say EEXIST,
;;   w — path_symlink onto "dang/" (nothing there): 44 ENOENT.
;; Deliberately untested: path_filestat_get of "linkfile/" without
;; SYMLINK_FOLLOW — wasmtime-on-Linux strips the slash internally and stats
;; the link itself (00) where wasmtime-on-macOS (and a raw lstat(2) on
;; either host) says ENOTDIR, an artifact with no portable expectation
;; (ADR-49's "copy wasmtime, don't invent" rule).
(module
  (import "wasi_snapshot_preview1" "path_readlink"
    (func $path_readlink (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_symlink"
    (func $path_symlink (param i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "linkfile/")
  (data (i32.const 16) "s")
  (data (i32.const 24) "sd1/")
  (data (i32.const 32) "file/")
  (data (i32.const 40) "dang/")
  ;; 64..95: readlink buffer, 100: bufused cell, 200: 4-byte message,
  ;; 208: message iovec, 216: message nwritten cell.
  (func $report (param $tag i32) (param $errno i32)
    (i32.store8 (i32.const 200) (local.get $tag))
    (i32.store8 (i32.const 201)
      (i32.add (i32.const 48) (i32.div_u (local.get $errno) (i32.const 10))))
    (i32.store8 (i32.const 202)
      (i32.add (i32.const 48) (i32.rem_u (local.get $errno) (i32.const 10))))
    (i32.store8 (i32.const 203) (i32.const 10))
    (i32.store (i32.const 208) (i32.const 200))
    (i32.store (i32.const 212) (i32.const 4))
    (drop (call $fd_write (i32.const 1) (i32.const 208) (i32.const 1) (i32.const 216))))
  (func (export "_start")
    ;; t: readlink("linkfile/").
    (call $report (i32.const 116)
      (call $path_readlink (i32.const 3) (i32.const 0) (i32.const 9)
        (i32.const 64) (i32.const 32) (i32.const 100)))
    ;; u: symlink("s", "sd1/") — existing directory behind the slash.
    (call $report (i32.const 117)
      (call $path_symlink (i32.const 16) (i32.const 1)
        (i32.const 3) (i32.const 24) (i32.const 4)))
    ;; v: symlink("s", "file/") — existing non-directory behind the slash.
    (call $report (i32.const 118)
      (call $path_symlink (i32.const 16) (i32.const 1)
        (i32.const 3) (i32.const 32) (i32.const 5)))
    ;; w: symlink("s", "dang/") — nothing behind the slash.
    (call $report (i32.const 119)
      (call $path_symlink (i32.const 16) (i32.const 1)
        (i32.const 3) (i32.const 40) (i32.const 5)))))
