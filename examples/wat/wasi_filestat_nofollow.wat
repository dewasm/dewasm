;; path_filestat_get without lookupflags::SYMLINK_FOLLOW on a symlink
;; must report the link itself (filetype 7, exposed as the exit code),
;; not its target.
(module
  (import "wasi_snapshot_preview1" "path_filestat_get"
    (func $filestat (param i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "link")
  (func (export "_start")
    (drop
      (call $filestat
        (i32.const 3)    ;; dirfd: the first preopen
        (i32.const 0)    ;; lookupflags: no SYMLINK_FOLLOW
        (i32.const 0)    ;; path ptr
        (i32.const 4)    ;; path len
        (i32.const 64))) ;; filestat buf
    ;; filestat.filetype is the u8 at offset 16.
    (call $proc_exit (i32.load8_u (i32.const 80)))))
