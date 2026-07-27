(module
  ;; Standalone `--dir` round-trip (ADR-31): opens "hello.txt" under the first
  ;; preopen (fd 3), writes a line, reopens it read-only, reads it back, and
  ;; echoes it to stdout. Uses a valid WASI rights mask (0x3FFFFFFF) rather than
  ;; -1 so the same fixture is accepted by wasmtime as the ground-truth engine.
  (import "wasi_snapshot_preview1" "path_open"
    (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_read"
    (func $fd_read (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_close"
    (func $fd_close (param i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "hello.txt")
  (data (i32.const 16) "hello, wasi fs!")
  (func (export "_start")
    ;; create + write
    (drop (call $path_open (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 9)
      (i32.const 9) (i64.const 0x3FFFFFFF) (i64.const 0x3FFFFFFF) (i32.const 0) (i32.const 100)))
    (i32.store (i32.const 104) (i32.const 16))
    (i32.store (i32.const 108) (i32.const 15))
    (drop (call $fd_write (i32.load (i32.const 100)) (i32.const 104) (i32.const 1) (i32.const 116)))
    (drop (call $fd_close (i32.load (i32.const 100))))
    ;; reopen read-only, read back
    (drop (call $path_open (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 9)
      (i32.const 0) (i64.const 0x3FFFFFFF) (i64.const 0x3FFFFFFF) (i32.const 0) (i32.const 120)))
    (i32.store (i32.const 240) (i32.const 200))
    (i32.store (i32.const 244) (i32.const 32))
    (drop (call $fd_read (i32.load (i32.const 120)) (i32.const 240) (i32.const 1) (i32.const 252)))
    (drop (call $fd_close (i32.load (i32.const 120))))
    ;; echo what was read back to stdout
    (i32.store (i32.const 104) (i32.const 200))
    (i32.store (i32.const 108) (i32.load (i32.const 252)))
    (drop (call $fd_write (i32.const 1) (i32.const 104) (i32.const 1) (i32.const 116)))))
