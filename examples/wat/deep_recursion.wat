;; `_start` recurses 5000 wasm frames.
;; That is far past, for example, CPython's default recursion limit of about 1000 frames.
;; `_start` then reports the depth check through `proc_exit` (42 on success).
;; So a standalone entrypoint that does nothing against deep recursion fails with an error.
;; It does not exit 42.
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
