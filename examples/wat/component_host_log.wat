;; A component importing a custom host interface: exercises canon lower
;; with memory+realloc (string lifting) and canon lift (u32 result).
(component
  (type $log_ty (func (param "msg" string) (result u32)))
  (import "example:host/log@0.2.0" (instance $host
    (export "log" (func (type $log_ty)))
  ))
  (alias export $host "log" (func $log))
  (core module $mem_mod
    (memory (export "memory") 1)
    (global $next (mut i32) (i32.const 16))
    (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
      (local $p i32)
      (local.set $p (global.get $next))
      (global.set $next (i32.add (global.get $next) (local.get 3)))
      (local.get $p))
  )
  (core instance $mem_i (instantiate $mem_mod))
  (alias core export $mem_i "memory" (core memory $mem))
  (alias core export $mem_i "cabi_realloc" (core func $realloc))
  (core func $lowered (canon lower (func $log)
    (memory $mem) (realloc $realloc) string-encoding=utf8))
  (core module $m
    (import "example:host/log@0.2.0" "log" (func $log (param i32 i32) (result i32)))
    (import "env" "memory" (memory 1))
    (data (i32.const 8) "hi p2")
    (func (export "run") (result i32) (call $log (i32.const 8) (i32.const 5)))
  )
  (core instance $mi (instantiate $m
    (with "example:host/log@0.2.0" (instance
      (export "log" (func $lowered))))
    (with "env" (instance
      (export "memory" (memory $mem))))))
  (func (export "run") (result u32) (canon lift (core func $mi "run")))
)
