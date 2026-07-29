;; Per-fd rights enforcement (ADR-40): every probe prints "<tag><errno as two
;; decimal digits>\n" so the expected stdout pins each errno exactly.
;;   a/b/c/d — setup steps that must succeed (00),
;;   p/w     — fd_pread/fd_pwrite on a fd narrowed to no rights (76 NOTCAPABLE),
;;   t       — O_TRUNC open under a dirfd without PATH_FILESTAT_SET_SIZE (76,
;;             and the host file must stay untruncated),
;;   o       — any open under a dirfd without PATH_OPEN (76).
(module
  (import "wasi_snapshot_preview1" "path_open"
    (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_fdstat_set_rights"
    (func $set_rights (param i32 i64 i64) (result i32)))
  (import "wasi_snapshot_preview1" "fd_pread"
    (func $fd_pread (param i32 i32 i32 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_pwrite"
    (func $fd_pwrite (param i32 i32 i32 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "data.txt")
  ;; 16: opened-fd cell, 24: pread/pwrite iovec, 32: 4-byte data buffer,
  ;; 40: nread/nwritten cell, 48: 4-byte message, 56: message iovec,
  ;; 64: message nwritten cell.
  (func $report (param $tag i32) (param $errno i32)
    (i32.store8 (i32.const 48) (local.get $tag))
    (i32.store8 (i32.const 49)
      (i32.add (i32.const 48) (i32.div_u (local.get $errno) (i32.const 10))))
    (i32.store8 (i32.const 50)
      (i32.add (i32.const 48) (i32.rem_u (local.get $errno) (i32.const 10))))
    (i32.store8 (i32.const 51) (i32.const 10))
    (i32.store (i32.const 56) (i32.const 48))
    (i32.store (i32.const 60) (i32.const 4))
    (drop (call $fd_write (i32.const 1) (i32.const 56) (i32.const 1) (i32.const 64))))
  (func (export "_start")
    (local $fd i32)
    ;; The pread/pwrite iovec: 4 bytes at 32.
    (i32.store (i32.const 24) (i32.const 32))
    (i32.store (i32.const 28) (i32.const 4))
    ;; a: open data.txt with rights FD_READ|FD_WRITE (0x42).
    (call $report (i32.const 97)
      (call $path_open (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 8)
        (i32.const 0) (i64.const 0x42) (i64.const 0) (i32.const 0) (i32.const 16)))
    (local.set $fd (i32.load (i32.const 16)))
    ;; b: narrow the fd to no rights at all (fd_fdstat_set_rights narrows only).
    (call $report (i32.const 98)
      (call $set_rights (local.get $fd) (i64.const 0) (i64.const 0)))
    ;; p: fd_pread without FD_READ is NOTCAPABLE.
    (call $report (i32.const 112)
      (call $fd_pread (local.get $fd) (i32.const 24) (i32.const 1)
        (i64.const 0) (i32.const 40)))
    ;; w: fd_pwrite without FD_WRITE is NOTCAPABLE.
    (call $report (i32.const 119)
      (call $fd_pwrite (local.get $fd) (i32.const 24) (i32.const 1)
        (i64.const 0) (i32.const 40)))
    ;; c: narrow the preopen dirfd to PATH_OPEN only (0x2000), inheriting rw.
    (call $report (i32.const 99)
      (call $set_rights (i32.const 3) (i64.const 0x2000) (i64.const 0x42)))
    ;; t: O_TRUNC (oflags 0x8) without PATH_FILESTAT_SET_SIZE is NOTCAPABLE —
    ;; and must not truncate the host file.
    (call $report (i32.const 116)
      (call $path_open (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 8)
        (i32.const 0x8) (i64.const 0x40) (i64.const 0) (i32.const 0) (i32.const 16)))
    ;; d: drop PATH_OPEN too.
    (call $report (i32.const 100)
      (call $set_rights (i32.const 3) (i64.const 0) (i64.const 0)))
    ;; o: any open through the narrowed dirfd is NOTCAPABLE.
    (call $report (i32.const 111)
      (call $path_open (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 8)
        (i32.const 0) (i64.const 0x2) (i64.const 0) (i32.const 0) (i32.const 16)))))
