;; poll_oneoff smoke test: one clock subscription (relative 1 ms) plus an fd_write-readiness subscription on stdout. stdout is always writable, so the call must report at least one event with error 0 without waiting out the timer.
;; On unexpected results the module exits non-zero so failures are loud.
;; Buffers are placed on 8-byte boundaries (poll_oneoff reads/writes i64 fields and hosts such as wasmtime require 8-byte alignment).
(module
  (import "wasi_snapshot_preview1" "poll_oneoff"
    (func $poll_oneoff (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 8) "poll ok\n")
  (func (export "_start")
    ;; --- subscriptions (48 bytes each) at offset 104 --- sub[0] @104: clock, monotonic, relative 1 ms.
    (i64.store (i32.const 104) (i64.const 1))       ;; userdata
    (i32.store8 (i32.const 112) (i32.const 0))      ;; tag = clock
    (i32.store (i32.const 120) (i32.const 1))       ;; clock_id = monotonic
    (i64.store (i32.const 128) (i64.const 1000000)) ;; timeout = 1 ms
    ;; precision @136 and flags @144 stay zero (relative deadline).
    ;; sub[1] @152: fd_write readiness on stdout (fd 1).
    (i64.store (i32.const 152) (i64.const 2))       ;; userdata
    (i32.store8 (i32.const 160) (i32.const 2))      ;; tag = fd_write
    (i32.store (i32.const 168) (i32.const 1))       ;; fd = 1 (stdout)

    ;; poll_oneoff(in=104, out=304, nsubscriptions=2, nevents=200)
    (drop (call $poll_oneoff (i32.const 104) (i32.const 304) (i32.const 2) (i32.const 200)))

    ;; Require at least one event.
    (if (i32.eqz (i32.load (i32.const 200)))
      (then (call $proc_exit (i32.const 1))))
    ;; Require the first event's error field (u16 at out+8 = 312) to be 0.
    (if (i32.load16_u (i32.const 312))
      (then (call $proc_exit (i32.const 2))))

    ;; Deterministic success line.
    (i32.store (i32.const 0) (i32.const 8)) ;; iovec ptr
    (i32.store (i32.const 4) (i32.const 8)) ;; iovec len ("poll ok\n")
    (drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 20)))
    (call $proc_exit (i32.const 0)))
)
