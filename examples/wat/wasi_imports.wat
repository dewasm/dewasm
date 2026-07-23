(module
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fdw (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "random_get"
    (func $rnd (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 8) "ok\n")
  (func (export "_start")
    (drop (call $rnd (i32.const 100) (i32.const 4)))
    (i32.store (i32.const 0) (i32.const 8))
    (i32.store (i32.const 4) (i32.const 3))
    (drop (call $fdw (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 20)))))
