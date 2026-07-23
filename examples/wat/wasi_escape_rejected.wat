(module
  (import "wasi_snapshot_preview1" "path_open"
    (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "../escape-canary/canary.txt")
  (data (i32.const 64) "ESCAPED\n")
  (data (i32.const 80) "BLOCKED\n")
  (func (export "_start")
    (local $errno i32)
    (local.set $errno (call $path_open (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 28)
      (i32.const 0) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 100)))
    (i32.store (i32.const 108) (i32.const 8))
    (if (i32.eqz (local.get $errno))
      (then (i32.store (i32.const 104) (i32.const 64)))
      (else (i32.store (i32.const 104) (i32.const 80))))
    (drop (call $fd_write (i32.const 1) (i32.const 104) (i32.const 1) (i32.const 116)))))
