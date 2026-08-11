;; Two shapes in which a folded operand can be overwritten by the reuse of the temp slot it reads: a call result taking the slot, and a spill writing it.
;; Both are pure arithmetic, so any wrong result is a folding bug. _start reports via proc_exit: 42 when both check out, 1 or 2 for the failing one.
(module
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  ;; Exported so wasmtime can run this fixture as ground truth: its WASI host functions require the guest memory to be exported.
  (memory (export "memory") 1)
  (func $one (result i32) i32.const 1)
  (func $two (result i32) i32.const 2)
  (func $big (result i32) i32.const 100)

  ;; (T0 + T1) is still pending at depth 0 when $big's result takes depth 1.
  (func $call_result (result i32)
    call $one
    call $two
    i32.add
    call $big
    i32.add)

  ;; The store spills the memory-reading pending at depth 1 into T1, which
  ;; (T0 + T1), still pending at depth 0, reads.
  (func $spilled (result i32)
    call $one
    call $two
    i32.add
    i32.const 0
    i32.load
    i32.const 4
    i32.const 7
    i32.store
    i32.add)

  (func (export "_start")
    (if (i32.ne (call $call_result) (i32.const 103))
      (then (call $proc_exit (i32.const 1))))
    (if (i32.ne (call $spilled) (i32.const 3))
      (then (call $proc_exit (i32.const 2))))
    (call $proc_exit (i32.const 42))))
