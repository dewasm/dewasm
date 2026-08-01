;; f64_alu -- f64 add / mul / sqrt in a tight loop.
;;
;; Every operation here is correctly rounded double arithmetic on every
;; runtime, so the accumulated sum is bit-identical everywhere and the printed
;; integer is a genuine oracle. The result is scaled by 1000 and truncated:
;; printing the double itself would compare Ruby/Python/Perl/Go/Java float
;; formatting rather than the arithmetic. The scaled sum stays inside i64 for
;; any iteration count below roughly 1e12, well past anything runnable.
;;
;; ---------------------------------------------------------------------------
;; Shared preamble. Duplicated verbatim in every hand-written kernel so each
;; .wat stays a standalone module that wat2wasm and dewasm can consume directly.
;;
;; A kernel is a WASI command module invoked as `<module> <iterations>`. It does
;; <iterations> units of work, writes exactly one line -- the decimal result
;; followed by a newline -- to stdout, and exits 0. <iterations> = 0 does no
;; work but still prints, which is how the harness measures startup in
;; isolation. Only args_sizes_get / args_get / fd_write / proc_exit are
;; imported and the bodies stick to i32/i64/f64, because the pure-Ruby and
;; pure-Python interpreters this suite compares cannot do more than that -- see
;; benchmarks/README.md.
;;
;; Memory map, shared by every kernel:
;;
;;   0x0000   4  argc                     (args_sizes_get out param)
;;   0x0004   4  argv buffer size         (args_sizes_get out param)
;;   0x0010      argv pointer array       (args_get out param)
;;   0x0100      argv string buffer       (args_get out param)
;;   0x0400   8  iovec { base, len }
;;   0x0408   4  fd_write nwritten
;;   0x0410  24  decimal scratch, filled backwards from 0x0428
;;   0x0800  29  usage message
;;   0x10000+    kernel working set, for the kernels that have one
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

  (data (i32.const 0x800) "usage: <module> <iterations>\n")

  ;; Every argv problem lands here. The harness always passes exactly one
  ;; argument, so anything else is a caller bug, not an input to guess at.
  (func $die
    (i32.store (i32.const 0x400) (i32.const 0x800))
    (i32.store (i32.const 0x404) (i32.const 29))
    (drop (call $fd_write
      (i32.const 2) (i32.const 0x400) (i32.const 1) (i32.const 0x408)))
    (call $proc_exit (i32.const 2))
    (unreachable))

  ;; argv[1] as an unsigned decimal. Hand-rolled atoi: ask for the sizes, ask
  ;; for the strings, then walk the bytes of argv[1].
  (func $iterations (result i32)
    (local $p i32) (local $start i32) (local $c i32) (local $n i32)
    (if (call $args_sizes_get (i32.const 0) (i32.const 4)) (then (call $die)))
    (if (i32.ne (i32.load (i32.const 0)) (i32.const 2)) (then (call $die)))
    (if (call $args_get (i32.const 0x10) (i32.const 0x100)) (then (call $die)))
    (local.set $p (i32.load (i32.const 0x14)))
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

  ;; Write `<v>\n` to stdout with v as an unsigned decimal. Digits come out
  ;; least significant first, so the scratch area is filled backwards.
  (func $print (param $v i64)
    (local $p i32)
    (local.set $p (i32.const 0x427))
    (i32.store8 (local.get $p) (i32.const 10))
    (loop $digit
      (local.set $p (i32.sub (local.get $p) (i32.const 1)))
      (i32.store8 (local.get $p)
        (i32.add (i32.const 48)
                 (i32.wrap_i64 (i64.rem_u (local.get $v) (i64.const 10)))))
      (local.set $v (i64.div_u (local.get $v) (i64.const 10)))
      (br_if $digit (i64.ne (local.get $v) (i64.const 0))))
    (i32.store (i32.const 0x400) (local.get $p))
    (i32.store (i32.const 0x404) (i32.sub (i32.const 0x428) (local.get $p)))
    (drop (call $fd_write
      (i32.const 1) (i32.const 0x400) (i32.const 1) (i32.const 0x408))))

  (func (export "_start")
    (local $n i32) (local $i i32) (local $a f64) (local $b f64)
    (local.set $n (call $iterations))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
        (local.set $a (f64.add (local.get $a) (f64.const 1)))
        (local.set $b
          (f64.add (local.get $b)
                   (f64.mul (f64.sqrt (local.get $a)) (f64.const 1.5))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next)))
    (call $print
      (i64.trunc_f64_s (f64.mul (local.get $b) (f64.const 1000)))))
)
