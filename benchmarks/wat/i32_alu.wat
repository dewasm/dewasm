;; i32_alu -- chained i32 arithmetic in a tight loop.
;;
;; One iteration is a dependency chain of mul / add / xor / shift / rotl on i32, so the measurement is integer-op dispatch cost and nothing else: no memory traffic, no calls, one branch per iteration.
;;
;; ---------------------------------------------------------------------------
;; Shared preamble.
;; Duplicated verbatim in every hand-written microbenchmark so each
;; .wat stays a standalone module that wat2wasm and dewasm can consume directly.
;;
;; A microbenchmark is a WASI command module invoked as `<module> <iterations>`.
;; It does
;; <iterations> units of work, writes exactly one line -- the decimal result followed by a newline -- to stdout, and exits 0. <iterations> = 0 does no work but still prints, which is how the harness measures startup in isolation.
;; Only args_sizes_get / args_get / fd_write / proc_exit are imported and the bodies stick to i32/i64/f64, because the pure-Ruby and pure-Python interpreters this suite compares cannot do more than that.
;;
;; Memory map, shared by every microbenchmark.
;; It starts at 0x1000 rather than at 0 because wasm3 traps with "out of bounds memory access" whenever a WASI out param is written to linear-memory address 0 -- address 0 is perfectly valid linear memory and every other runtime in the matrix accepts it, so the whole block is simply moved up out of wasm3's way:
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
    (local $n i32) (local $i i32) (local $a i32) (local $b i32)
    (local.set $n (call $iterations))
    (local.set $a (i32.const 1))
    (local.set $b (i32.const 0x9e3779b9))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
        (local.set $a
          (i32.add (i32.mul (local.get $a) (i32.const 1664525))
                   (i32.const 1013904223)))
        (local.set $a
          (i32.xor (local.get $a) (i32.shr_u (local.get $a) (i32.const 15))))
        (local.set $a
          (i32.add (local.get $a) (i32.shl (local.get $a) (i32.const 7))))
        (local.set $b
          (i32.rotl (i32.xor (local.get $b) (local.get $a)) (i32.const 11)))
        (local.set $b
          (i32.sub (local.get $b) (i32.shr_s (local.get $a) (i32.const 3))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next)))
    (call $print (i64.extend_i32_u (local.get $b))))
)
