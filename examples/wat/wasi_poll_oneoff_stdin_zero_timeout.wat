;; `poll_oneoff` with a zero-timeout clock plus an `fd_read` subscription on `stdin`.
;; An event loop polls this way while it still has work to do.
;; Whichever event the call reports, it must not consume or invent `stdin` bytes.
;; The module then reads `stdin` once and echoes what it got.
;; So `stdout` must equal `stdin` exactly.
;; Buffers are placed on 8-byte boundaries.
;; `poll_oneoff` reads/writes i64 fields, and hosts such as Wasmtime require 8-byte alignment.
(module
  (import "wasi_snapshot_preview1" "poll_oneoff"
    (func $poll_oneoff (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_read"
    (func $fd_read (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (func (export "_start")
    ;; --- subscriptions (48 bytes each) at offset 104 ---
    ;; sub[0] @104: clock, monotonic, relative 0 ns.
    (i64.store (i32.const 104) (i64.const 1))       ;; userdata
    (i32.store8 (i32.const 112) (i32.const 0))      ;; tag = clock
    (i32.store (i32.const 120) (i32.const 1))       ;; clock_id = monotonic
    ;; timeout @128, precision @136 and flags @144 stay zero (relative, already due).
    ;; sub[1] @152: `fd_read` readiness on `stdin` (`fd` 0).
    (i64.store (i32.const 152) (i64.const 2))       ;; userdata
    (i32.store8 (i32.const 160) (i32.const 1))      ;; tag = fd_read
    (i32.store (i32.const 168) (i32.const 0))       ;; fd = 0 (stdin)

    ;; `poll_oneoff(in=104, out=304, nsubscriptions=2, nevents=200)`
    (if (call $poll_oneoff (i32.const 104) (i32.const 304) (i32.const 2) (i32.const 200))
      (then (call $proc_exit (i32.const 1))))
    (if (i32.eqz (i32.load (i32.const 200)))
      (then (call $proc_exit (i32.const 2))))

    ;; Read up to 64 bytes of `stdin` into 512.
    (i32.store (i32.const 0) (i32.const 512)) ;; iovec ptr
    (i32.store (i32.const 4) (i32.const 64))  ;; iovec len
    (if (call $fd_read (i32.const 0) (i32.const 0) (i32.const 1) (i32.const 16))
      (then (call $proc_exit (i32.const 3))))

    ;; Echo exactly the bytes read.
    (i32.store (i32.const 4) (i32.load (i32.const 16)))
    (drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 20)))
    (call $proc_exit (i32.const 0)))
)
