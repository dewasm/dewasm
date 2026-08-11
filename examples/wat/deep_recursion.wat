;; _start recurses 5000 wasm frames (far past e.g. CPython's default
;; ~1000-frame recursion limit), then reports the depth check via proc_exit
;; (42 on success), so a standalone entrypoint without a deep-recursion mitigation fails loudly instead of exiting 42.
(module
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  ;; f(n) = n, computed through n recursive frames.
  (func $f (param $n i32) (result i32)
    (if (i32.eqz (local.get $n))
      (then (return (i32.const 0))))
    (i32.add (i32.const 1) (call $f (i32.sub (local.get $n) (i32.const 1)))))
  (func (export "_start")
    (if (i32.eq (call $f (i32.const 5000)) (i32.const 5000))
      (then (call $proc_exit (i32.const 42))))
    (call $proc_exit (i32.const 1))))
