;; Trailing-slash shapes across the path_* family (issue #42): unspecified by
;; WASI, pinned to wasmtime 47 as measured on macOS and Linux.
;; Each probe prints "<tag><errno as two decimal digits>\n"; per-probe intent is commented at each call, expectations live in the shared WASI_CASES entry.
;; Setup provides a file "file", a file "file2", and a directory "dir".
;; Probe k is wasmtime's own host split: EINVAL on macOS, EISDIR on Linux.
;; Left unpinned: nofollow filestat of "file/" (wasmtime's two hosts disagree) and unlink/rename-of-directory errnos (host split, covered per host by the strict wasi-testsuite).
(module
  (import "wasi_snapshot_preview1" "path_rename"
    (func $path_rename (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_unlink_file"
    (func $path_unlink_file (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_filestat_get"
    (func $path_filestat_get (param i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_open"
    (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_remove_directory"
    (func $path_remove_directory (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_link"
    (func $path_link (param i32 i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_create_directory"
    (func $path_create_directory (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  ;; Prefix reuse: "file/" also serves "file" (len 4), "dir/" serves "dir"
  ;; (len 3), "file2/" serves "file2" (len 5).
  (data (i32.const 0) "file/")
  (data (i32.const 8) "renamed")
  (data (i32.const 16) "dir/")
  (data (i32.const 24) "file2/")
  (data (i32.const 32) "missing/")
  (data (i32.const 40) "x")
  (data (i32.const 48) "lnk")
  (data (i32.const 56) "newd/")
  (data (i32.const 64) "newf/")
  (data (i32.const 72) "newdir/")
  ;; 96: opened-fd cell, 128..191: filestat buffer, 200: 4-byte message,
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
    ;; a: rename("file/", "renamed").
    ;; Slash-suffixed non-directory source.
    (call $report (i32.const 97)
      (call $path_rename (i32.const 3) (i32.const 0) (i32.const 5)
        (i32.const 3) (i32.const 8) (i32.const 7)))
    ;; b: rename("dir", "file2/").
    ;; Slash-suffixed existing-file destination.
    (call $report (i32.const 98)
      (call $path_rename (i32.const 3) (i32.const 16) (i32.const 3)
        (i32.const 3) (i32.const 24) (i32.const 6)))
    ;; c: unlink("file/").
    (call $report (i32.const 99)
      (call $path_unlink_file (i32.const 3) (i32.const 0) (i32.const 5)))
    ;; d: unlink("missing/").
    ;; Missing beats the slash: ENOENT.
    (call $report (i32.const 100)
      (call $path_unlink_file (i32.const 3) (i32.const 32) (i32.const 8)))
    ;; e: filestat_get("file/") with SYMLINK_FOLLOW.
    (call $report (i32.const 101)
      (call $path_filestat_get (i32.const 3) (i32.const 1)
        (i32.const 0) (i32.const 5) (i32.const 128)))
    ;; f: open("file/") read-only (rights FD_READ).
    (call $report (i32.const 102)
      (call $path_open (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 5)
        (i32.const 0) (i64.const 0x2) (i64.const 0) (i32.const 0) (i32.const 96)))
    ;; g: remove_directory("file/").
    (call $report (i32.const 103)
      (call $path_remove_directory (i32.const 3) (i32.const 0) (i32.const 5)))
    ;; h: link("file/", "lnk").
    ;; Slash-suffixed hard-link source.
    (call $report (i32.const 104)
      (call $path_link (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 5)
        (i32.const 3) (i32.const 48) (i32.const 3)))
    ;; i: rename("missing/", "x").
    (call $report (i32.const 105)
      (call $path_rename (i32.const 3) (i32.const 32) (i32.const 8)
        (i32.const 3) (i32.const 40) (i32.const 1)))
    ;; j: rename("file2", "newd/").
    ;; A nonexistent slash-suffixed destination:
    ;; the slash is stripped and the rename proceeds (wasmtime).
    (call $report (i32.const 106)
      (call $path_rename (i32.const 3) (i32.const 24) (i32.const 5)
        (i32.const 3) (i32.const 56) (i32.const 5)))
    ;; k: open("newf/", O_CREAT, rights FD_READ|FD_WRITE).
    ;; Must not create;
    ;; EINVAL on macOS, EISDIR on Linux (wasmtime's own split).
    (call $report (i32.const 107)
      (call $path_open (i32.const 3) (i32.const 0) (i32.const 64) (i32.const 5)
        (i32.const 0x1) (i64.const 0x42) (i64.const 0) (i32.const 0) (i32.const 96)))
    ;; l: create_directory("file/").
    ;; EEXIST, uniformly.
    (call $report (i32.const 108)
      (call $path_create_directory (i32.const 3) (i32.const 0) (i32.const 5)))
    ;; m: create_directory("newdir/").
    ;; The slash is legal on mkdir.
    (call $report (i32.const 109)
      (call $path_create_directory (i32.const 3) (i32.const 72) (i32.const 7)))
    ;; n: remove_directory("missing/").
    (call $report (i32.const 110)
      (call $path_remove_directory (i32.const 3) (i32.const 32) (i32.const 8)))
    ;; o: remove_directory("dir/").
    ;; EINVAL, and "dir" survives.
    (call $report (i32.const 111)
      (call $path_remove_directory (i32.const 3) (i32.const 16) (i32.const 4)))
    ;; p: remove_directory("dir").
    ;; The bare name still removes it.
    (call $report (i32.const 112)
      (call $path_remove_directory (i32.const 3) (i32.const 16) (i32.const 3)))))
