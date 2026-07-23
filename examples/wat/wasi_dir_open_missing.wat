;; path_open with oflags::DIRECTORY on a path that does not exist must
;; fail with ERRNO_NOENT (44), not ERRNO_NOTDIR (54); the errno becomes
;; the exit code.
(module
  (import "wasi_snapshot_preview1" "path_open"
    (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "nosuch")
  (func (export "_start")
    (call $proc_exit
      (call $path_open
        (i32.const 3)   ;; dirfd: the first preopen
        (i32.const 0)   ;; dirflags
        (i32.const 0)   ;; path ptr
        (i32.const 6)   ;; path len
        (i32.const 0x2) ;; oflags: DIRECTORY
        (i64.const 0)   ;; rights base
        (i64.const 0)   ;; rights inheriting
        (i32.const 0)   ;; fdflags
        (i32.const 64)))))
