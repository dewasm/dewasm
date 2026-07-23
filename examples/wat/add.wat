(module
  (func (export "add") (param i32 i32) (result i32)
    local.get 0
    local.get 1
    i32.add)
  (func (export "fib") (param i32) (result i32)
    (if (result i32) (i32.lt_u (local.get 0) (i32.const 2))
      (then (local.get 0))
      (else
        (i32.add
          (call 1 (i32.sub (local.get 0) (i32.const 1)))
          (call 1 (i32.sub (local.get 0) (i32.const 2)))))))
)
