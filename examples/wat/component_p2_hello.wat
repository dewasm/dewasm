;; A minimal wasi:cli-flavoured component: gets stdout, writes through
;; wasi:io/streams, and exports run. The declared WIT types are simpler
;; than upstream WASI's (result without payloads) — the bundled Rt::WASIP2
;; host resolves functions by name and its post-lift values fit both.
(component
  (import "wasi:cli/stdout@0.2.0" (instance $stdout
    (export "output-stream" (type $os (sub resource)))
    (export "get-stdout" (func (result (own $os))))
  ))
  (import "wasi:io/streams@0.2.0" (instance $streams
    (export "output-stream" (type $os2 (sub resource)))
    (export "[method]output-stream.blocking-write-and-flush"
      (func (param "self" (borrow $os2)) (param "contents" (list u8)) (result (result))))
  ))
  (core module $mem_mod
    (memory (export "memory") 1)
    (global $next (mut i32) (i32.const 64))
    (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
      (local $p i32)
      (local.set $p (global.get $next))
      (global.set $next (i32.add (global.get $next) (local.get 3)))
      (local.get $p))
  )
  (core instance $mem_i (instantiate $mem_mod))
  (alias core export $mem_i "memory" (core memory $mem))
  (alias core export $mem_i "cabi_realloc" (core func $realloc))
  (alias export $stdout "get-stdout" (func $get_stdout))
  (alias export $streams "[method]output-stream.blocking-write-and-flush" (func $write))
  (core func $get_stdout_l (canon lower (func $get_stdout)))
  (core func $write_l (canon lower (func $write)
    (memory $mem) (realloc $realloc) string-encoding=utf8))
  (core module $m
    (import "wasi:cli/stdout@0.2.0" "get-stdout" (func $get_stdout (result i32)))
    (import "wasi:io/streams@0.2.0" "[method]output-stream.blocking-write-and-flush"
      (func $write (param i32 i32 i32) (result i32)))
    (import "env" "memory" (memory 1))
    (data (i32.const 8) "hello from p2\0a")
    (func (export "run") (result i32)
      (drop (call $write (call $get_stdout) (i32.const 8) (i32.const 14)))
      (i32.const 0))
  )
  (core instance $mi (instantiate $m
    (with "wasi:cli/stdout@0.2.0" (instance
      (export "get-stdout" (func $get_stdout_l))))
    (with "wasi:io/streams@0.2.0" (instance
      (export "[method]output-stream.blocking-write-and-flush" (func $write_l))))
    (with "env" (instance
      (export "memory" (memory $mem))))))
  (func (export "run") (result u32) (canon lift (core func $mi "run")))
)
