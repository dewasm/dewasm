;; Counterpart of `shared_table_a.wat`: imports its table and runs `call_indirect` through it.
;; It declares the shared type at a different index.
(module
  (type (func (result i32)))
  (import "a" "tab" (table 1 funcref))
  (func (export "call0") (result i32)
    i32.const 0
    call_indirect (type 0)))
