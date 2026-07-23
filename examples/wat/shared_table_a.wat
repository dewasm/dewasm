;; Exports a table whose slot holds a (func (result i32)). That type sits
;; at index 1 here but at index 0 in shared_table_b.wat, so any
;; module-local type id in the call_indirect check breaks this pair.
(module
  (type (func (param f64)))
  (type (func (result i32)))
  (func $f42 (type 1) i32.const 42)
  (table (export "tab") 1 funcref)
  (elem (i32.const 0) $f42))
