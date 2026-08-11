;; fd_readdir with a dircookie whose high bit is set: the u64 cookie is an
;; unsigned position far past any snapshot's end, so each call must succeed
;; with zero dirent bytes (matching wasmtime): never crash in a runtime that
;; holds the cookie in a signed type, and never return entries. Two shapes:
;; 0x8000000000000000 truncates to index 0 (the silently-wrong-entries shape)
;; and 0x8000000000000063's low bits land past the snapshot end (the
;; out-of-bounds shape). A nonzero errno becomes the exit code; a nonzero
;; bufused exits 200/201.
(module
  (import "wasi_snapshot_preview1" "fd_readdir"
    (func $fd_readdir (param i32 i32 i32 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "past-end ok\n")
  (func $check (param $cookie i64) (param $failcode i32)
    (local $e i32)
    (local.set $e (call $fd_readdir
      (i32.const 3)   ;; dirfd: the first preopen
      (i32.const 300) ;; buf ptr
      (i32.const 256) ;; buf len
      (local.get $cookie)
      (i32.const 260)))
    (if (local.get $e) (then (call $proc_exit (local.get $e))))
    (if (i32.load (i32.const 260)) (then (call $proc_exit (local.get $failcode)))))
  (func (export "_start")
    (call $check (i64.const 0x8000000000000000) (i32.const 200))
    (call $check (i64.const 0x8000000000000063) (i32.const 201))
    (i32.store (i32.const 104) (i32.const 0))
    (i32.store (i32.const 108) (i32.const 12))
    (drop (call $fd_write (i32.const 1) (i32.const 104) (i32.const 1) (i32.const 116)))))
