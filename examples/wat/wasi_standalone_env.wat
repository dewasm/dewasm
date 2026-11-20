(module
  ;; Standalone `--env`: writes the guest's environment to `stdout`, one `NAME=VAL` per line.
  ;; `environ_get` ends each entry with a zero byte, and each of those becomes a newline.
  ;; The buffer starts at 8192, so the whole environment must fit in the 57344 bytes after it.
  (import "wasi_snapshot_preview1" "environ_sizes_get"
    (func $environ_sizes_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "environ_get"
    (func $environ_get (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "_start") (local $i i32) (local $size i32)
    (drop (call $environ_sizes_get (i32.const 0) (i32.const 4)))
    (drop (call $environ_get (i32.const 1024) (i32.const 8192)))
    (local.set $size (i32.load (i32.const 4)))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $i) (local.get $size)))
        (if (i32.eqz (i32.load8_u (i32.add (i32.const 8192) (local.get $i))))
          (then (i32.store8 (i32.add (i32.const 8192) (local.get $i)) (i32.const 10))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next)))
    (i32.store (i32.const 16) (i32.const 8192))
    (i32.store (i32.const 20) (local.get $size))
    (drop (call $fd_write (i32.const 1) (i32.const 16) (i32.const 1) (i32.const 24)))))
