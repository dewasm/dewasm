(module
  (import "wasi_snapshot_preview1" "args_sizes_get"
    (func $asg (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $pe (param i32)))
  (memory 1)
  (func (export "_start")
    (drop (call $asg (i32.const 0) (i32.const 4)))
    (call $pe (i32.load (i32.const 0)))))
