;; br_table: a 16-arm branch table.
;;
;; The index is derived from a pseudo-random sequence, so no runtime can predict the arm, and each arm applies a different ordered pair of operators with constants no other arm uses, so the table cannot be folded back into arithmetic.
;; Against i32_alu, whose body is straight-line arithmetic of comparable size, the difference is what the dispatch costs.
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
    (local $n i32) (local $i i32) (local $h i32) (local $k i32) (local $a i32)
    (local.set $n (call $iterations))
    (local.set $h (i32.const 0x12345678))
    (local.set $a (i32.const 1))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
        (local.set $h
          (i32.add (i32.mul (local.get $h) (i32.const 1664525))
                   (i32.const 1013904223)))
        (local.set $k
          (i32.and (i32.shr_u (local.get $h) (i32.const 12)) (i32.const 15)))
        (block $sw
          (block $c0 (block $c1 (block $c2 (block $c3
          (block $c4 (block $c5 (block $c6 (block $c7
          (block $c8 (block $c9 (block $c10 (block $c11
          (block $c12 (block $c13 (block $c14 (block $c15
            ;; $k is masked to 0..15, so the default target is never taken.
            (br_table $c0 $c1 $c2 $c3 $c4 $c5 $c6 $c7 $c8 $c9 $c10 $c11 $c12 $c13 $c14 $c15 $c0 (local.get $k)))
          ;; $c15
          (local.set $a (i32.mul (local.get $a) (i32.const 0x28b7bd67)))
          (local.set $a (i32.mul (local.get $a) (i32.const 0xbd794d61)))
          (br $sw))
          ;; $c14
          (local.set $a (i32.mul (local.get $a) (i32.const 0xec48c9f5)))
          (local.set $a (i32.sub (local.get $a) (i32.const 0xb1a1b88b)))
          (br $sw))
          ;; $c13
          (local.set $a (i32.mul (local.get $a) (i32.const 0xafd9d683)))
          (local.set $a (i32.xor (local.get $a) (i32.const 0xa5ca23b5)))
          (br $sw))
          ;; $c12
          (local.set $a (i32.mul (local.get $a) (i32.const 0x736ae311)))
          (local.set $a (i32.add (local.get $a) (i32.const 0x99f28edf)))
          (br $sw))
          ;; $c11
          (local.set $a (i32.sub (local.get $a) (i32.const 0x36fbef9f)))
          (local.set $a (i32.mul (local.get $a) (i32.const 0x8e1afa09)))
          (br $sw))
          ;; $c10
          (local.set $a (i32.sub (local.get $a) (i32.const 0xfa8cfc2d)))
          (local.set $a (i32.sub (local.get $a) (i32.const 0x82436533)))
          (br $sw))
          ;; $c9
          (local.set $a (i32.sub (local.get $a) (i32.const 0xbe1e08bb)))
          (local.set $a (i32.xor (local.get $a) (i32.const 0x766bd05d)))
          (br $sw))
          ;; $c8
          (local.set $a (i32.sub (local.get $a) (i32.const 0x81af1549)))
          (local.set $a (i32.add (local.get $a) (i32.const 0x6a943b87)))
          (br $sw))
          ;; $c7
          (local.set $a (i32.xor (local.get $a) (i32.const 0x454021d7)))
          (local.set $a (i32.mul (local.get $a) (i32.const 0x5ebca6b1)))
          (br $sw))
          ;; $c6
          (local.set $a (i32.xor (local.get $a) (i32.const 0x08d12e65)))
          (local.set $a (i32.sub (local.get $a) (i32.const 0x52e511db)))
          (br $sw))
          ;; $c5
          (local.set $a (i32.xor (local.get $a) (i32.const 0xcc623af3)))
          (local.set $a (i32.xor (local.get $a) (i32.const 0x470d7d05)))
          (br $sw))
          ;; $c4
          (local.set $a (i32.xor (local.get $a) (i32.const 0x8ff34781)))
          (local.set $a (i32.add (local.get $a) (i32.const 0x3b35e82f)))
          (br $sw))
          ;; $c3
          (local.set $a (i32.add (local.get $a) (i32.const 0x5384540f)))
          (local.set $a (i32.mul (local.get $a) (i32.const 0x2f5e5359)))
          (br $sw))
          ;; $c2
          (local.set $a (i32.add (local.get $a) (i32.const 0x1715609d)))
          (local.set $a (i32.sub (local.get $a) (i32.const 0x2386be83)))
          (br $sw))
          ;; $c1
          (local.set $a (i32.add (local.get $a) (i32.const 0xdaa66d2b)))
          (local.set $a (i32.xor (local.get $a) (i32.const 0x17af29ad)))
          (br $sw))
          ;; $c0
          (local.set $a (i32.add (local.get $a) (i32.const 0x9e3779b9)))
          (local.set $a (i32.add (local.get $a) (i32.const 0x0bd794d7)))
          (br $sw))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next)))
    (call $print (i64.extend_i32_u (local.get $a))))
)
