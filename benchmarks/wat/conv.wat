;; conv: conversions between f64, i32 and i64.
;;
;; One iteration truncates a double to i32 and to i64, converts integers back to double in both signednesses, and moves a value through i64.reinterpret_f64 and f64.reinterpret_i64.
;; The backends implement these as runtime helpers rather than as host operators, which neither the arithmetic nor the memory cases exercise.
;;
;; Two constraints hold every value in range: each truncation input is built from a 24-bit integer, because an out-of-range truncation traps, and the only bits ever reinterpreted into f64 are mantissa bits of a value that came out of finite f64 arithmetic, because a NaN would not be comparable across runtimes (its payload bits differ).
;;
;; ---------------------------------------------------------------------------
;; Shared preamble.
;; Duplicated verbatim in every hand-written microbenchmark so each
;; .wat stays a standalone module that wat2wasm and dewasm can consume directly.
;;
;; A microbenchmark is a WASI command module invoked as `<module> <iterations>`.
;; It does
;; <iterations> units of work, writes exactly one line (the decimal result followed by a newline) to stdout, and exits 0. <iterations> = 0 does no work but still prints, which is how the harness measures startup in isolation.
;; Only args_sizes_get / args_get / fd_write / proc_exit are imported, and a body stays inside i32/i64/f64 except for the one axis its case exists to measure: f32 in f32_alu, exception handling in eh_throw and eh_try.
;; That keeps every other case within reach of the pure-Ruby and pure-Python interpreters this suite compares, and a runner that cannot execute a case's axis is excluded for that case in the harness workload table, with the reason stated there.
;;
;; Memory map, shared by every microbenchmark.
;; It starts at 0x1000 rather than at 0 because wasm3 traps with "out of bounds memory access" whenever a WASI out param is written to linear-memory address 0: address 0 is perfectly valid linear memory and every other runtime in the matrix accepts it, so the whole block is simply moved up out of wasm3's way:
;;
;; 0x1000   4  argc                     (args_sizes_get out param)
;; 0x1004   4  argv buffer size         (args_sizes_get out param)
;; 0x1010      argv pointer array       (args_get out param)
;; 0x1100      argv string buffer       (args_get out param)
;; 0x1400   8  iovec { base, len }
;; 0x1408   4  fd_write nwritten
;; 0x1410  24  decimal scratch, filled backwards from 0x1428
;; 0x1800  29  usage message
;; 0x10000+    working set, for the microbenchmarks that have one
;; ---------------------------------------------------------------------------
(module
  (import "wasi_snapshot_preview1" "args_sizes_get"
    (func $args_sizes_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "args_get"
    (func $args_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit"
    (func $proc_exit (param i32)))

  (memory (export "memory") 2)

  (data (i32.const 0x1800) "usage: <module> <iterations>\n")

  ;; Every argv problem lands here.
  ;; The harness always passes exactly one argument, so anything else is a caller bug, not an input to guess at.
  (func $die
    (i32.store (i32.const 0x1400) (i32.const 0x1800))
    (i32.store (i32.const 0x1404) (i32.const 29))
    (drop (call $fd_write
      (i32.const 2) (i32.const 0x1400) (i32.const 1) (i32.const 0x1408)))
    (call $proc_exit (i32.const 2))
    (unreachable))

  ;; argv[1] as an unsigned decimal.
  ;; Hand-rolled atoi: ask for the sizes, ask for the strings, then walk the bytes of argv[1].
  (func $iterations (result i32)
    (local $p i32) (local $start i32) (local $c i32) (local $n i32)
    (if (call $args_sizes_get (i32.const 0x1000) (i32.const 0x1004))
      (then (call $die)))
    (if (i32.ne (i32.load (i32.const 0x1000)) (i32.const 2))
      (then (call $die)))
    (if (call $args_get (i32.const 0x1010) (i32.const 0x1100))
      (then (call $die)))
    (local.set $p (i32.load (i32.const 0x1014)))
    (local.set $start (local.get $p))
    (block $end
      (loop $byte
        (local.set $c (i32.load8_u (local.get $p)))
        (br_if $end (i32.eqz (local.get $c)))
        ;; Unsigned wraparound turns every non-digit into a large value.
        (if (i32.gt_u (i32.sub (local.get $c) (i32.const 48)) (i32.const 9))
          (then (call $die)))
        (local.set $n
          (i32.add (i32.mul (local.get $n) (i32.const 10))
                   (i32.sub (local.get $c) (i32.const 48))))
        (local.set $p (i32.add (local.get $p) (i32.const 1)))
        (br $byte)))
    (if (i32.eq (local.get $p) (local.get $start)) (then (call $die)))
    (local.get $n))

  ;; Write `<v>\n` to stdout with v as an unsigned decimal.
  ;; Digits come out least significant first, so the scratch area is filled backwards.
  (func $print (param $v i64)
    (local $p i32)
    (local.set $p (i32.const 0x1427))
    (i32.store8 (local.get $p) (i32.const 10))
    (loop $digit
      (local.set $p (i32.sub (local.get $p) (i32.const 1)))
      (i32.store8 (local.get $p)
        (i32.add (i32.const 48)
                 (i32.wrap_i64 (i64.rem_u (local.get $v) (i64.const 10)))))
      (local.set $v (i64.div_u (local.get $v) (i64.const 10)))
      (br_if $digit (i64.ne (local.get $v) (i64.const 0))))
    (i32.store (i32.const 0x1400) (local.get $p))
    (i32.store (i32.const 0x1404) (i32.sub (i32.const 0x1428) (local.get $p)))
    (drop (call $fd_write
      (i32.const 1) (i32.const 0x1400) (i32.const 1) (i32.const 0x1408))))

  (func (export "_start")
    (local $n i32) (local $i i32) (local $h i32)
    (local $x f64) (local $y f64) (local $bits i64) (local $a i64)
    (local.set $n (call $iterations))
    (local.set $h (i32.const 0x12345678))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
        (local.set $h
          (i32.add (i32.mul (local.get $h) (i32.const 1664525))
                   (i32.const 1013904223)))
        (local.set $x
          (f64.add (f64.convert_i32_u (i32.shr_u (local.get $h) (i32.const 8)))
                   (f64.const 1.5)))
        (local.set $bits (i64.reinterpret_f64 (local.get $x)))
        ;; The exponent field is left alone, so the value stays finite.
        (local.set $bits (i64.xor (local.get $bits) (i64.const 0x3ff)))
        (local.set $x (f64.reinterpret_i64 (local.get $bits)))
        (local.set $a
          (i64.add (local.get $a) (i64.trunc_f64_s (local.get $x))))
        (local.set $a
          (i64.xor (local.get $a)
                   (i64.extend_i32_u
                     (i32.trunc_f64_s
                       (f64.mul (local.get $x) (f64.const -0.25))))))
        (local.set $y
          (f64.convert_i64_s
            (i64.sub (i64.and (local.get $a) (i64.const 0xffffff))
                     (i64.const 0x800000))))
        (local.set $a
          (i64.add (local.get $a)
                   (i64.trunc_f64_s (f64.mul (local.get $y) (f64.const 1.25)))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next)))
    (call $print (local.get $a)))
)
