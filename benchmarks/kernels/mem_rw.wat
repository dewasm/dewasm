;; mem_rw -- i32 store/load over linear memory.
;;
;; One iteration reads one i32 and writes one i32 at a pseudo-random offset in
;; a 256 KiB window, so the address stream defeats any locality the backend's
;; memory representation might otherwise get for free. Against i32_alu this
;; separates memory-access cost from arithmetic cost.
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

  (memory (export "memory") 8)

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
    (local $n i32) (local $i i32) (local $h i32) (local $a i32) (local $p i32)
    (local.set $n (call $iterations))
    (local.set $h (i32.const 0x12345678))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
        (local.set $h
          (i32.add (i32.mul (local.get $h) (i32.const 1664525))
                   (i32.const 1013904223)))
        ;; 0x3fffc keeps the offset i32-aligned inside the 256 KiB window that
        ;; starts one page in, clear of the preamble's scratch area.
        (local.set $p
          (i32.add (i32.const 0x10000)
                   (i32.and (local.get $h) (i32.const 0x3fffc))))
        (local.set $a (i32.add (local.get $a) (i32.load (local.get $p))))
        (i32.store (local.get $p) (i32.xor (local.get $a) (local.get $i)))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next)))
    (call $print (i64.extend_i32_u (local.get $a))))
)
