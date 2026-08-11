;; Trailing slashes through symlinks (issue #42): unspecified by WASI, so
;; pinned to wasmtime 47 as measured on both hosts. Setup: file "file",
;; symlink "linkfile" -> "file", directory "sd1". Probes print
;; "<tag><errno>\n" and are commented at each call; expectations live in the
;; shared WASI_CASES entry. Left unpinned: nofollow filestat of "linkfile/"
;; (wasmtime's two hosts disagree).
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
