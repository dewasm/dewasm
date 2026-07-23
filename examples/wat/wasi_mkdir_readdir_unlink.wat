(module
  (import "wasi_snapshot_preview1" "path_create_directory"
    (func $mkdir (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_open"
    (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_close"
    (func $fd_close (param i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_readdir"
    (func $fd_readdir (param i32 i32 i32 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_unlink_file"
    (func $unlink (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_remove_directory"
    (func $rmdir (param i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "sub")
  (data (i32.const 16) "sub/file.txt")
  (func (export "_start")
    (drop (call $mkdir (i32.const 3) (i32.const 0) (i32.const 3)))
    (drop (call $path_open (i32.const 3) (i32.const 0) (i32.const 16) (i32.const 12)
      (i32.const 9) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 100)))
    (drop (call $fd_close (i32.load (i32.const 100))))
    ;; list the root directory, echo the raw dirent bytes to stdout
    (drop (call $fd_readdir (i32.const 3) (i32.const 300) (i32.const 256) (i64.const 0) (i32.const 260)))
    (i32.store (i32.const 104) (i32.const 300))
    (i32.store (i32.const 108) (i32.load (i32.const 260)))
    (drop (call $fd_write (i32.const 1) (i32.const 104) (i32.const 1) (i32.const 116)))
    ;; clean up
    (drop (call $unlink (i32.const 3) (i32.const 16) (i32.const 12)))
    (drop (call $rmdir (i32.const 3) (i32.const 0) (i32.const 3)))))
