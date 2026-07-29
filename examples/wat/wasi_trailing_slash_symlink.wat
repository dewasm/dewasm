;; Trailing-slash resolution through a symlink (POSIX; issue #42): the slash
;; forces the final symlink to be *followed*, even for syscalls that otherwise
;; operate on the link itself — so a slash-suffixed symlink-to-a-file is
;; ENOTDIR everywhere the target's non-directory-ness is reached. Host layout
;; (from the case's setup): a regular file "file" and a symlink
;; "linkfile" -> "file". Probes print "<tag><errno>\n" like
;; wasi_trailing_slash.wat:
;;   s — path_filestat_get of "linkfile/" *without* SYMLINK_FOLLOW: the slash
;;       still forces following, and the target is a file (54 ENOTDIR),
;;   t — path_readlink of "linkfile/": ditto (54), never the link content.
;; The symlink-to-a-*directory* shapes stay untested: hosts diverge on them
;; (see wasi_trailing_slash.wat's exclusion note).
(module
  (import "wasi_snapshot_preview1" "path_filestat_get"
    (func $path_filestat_get (param i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_readlink"
    (func $path_readlink (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "linkfile/")
  ;; 64..95: readlink buffer, 100: bufused cell, 128..191: filestat buffer,
  ;; 200: 4-byte message, 208: message iovec, 216: message nwritten cell.
  (func $report (param $tag i32) (param $errno i32)
    (i32.store8 (i32.const 200) (local.get $tag))
    (i32.store8 (i32.const 201)
      (i32.add (i32.const 48) (i32.div_u (local.get $errno) (i32.const 10))))
    (i32.store8 (i32.const 202)
      (i32.add (i32.const 48) (i32.rem_u (local.get $errno) (i32.const 10))))
    (i32.store8 (i32.const 203) (i32.const 10))
    (i32.store (i32.const 208) (i32.const 200))
    (i32.store (i32.const 212) (i32.const 4))
    (drop (call $fd_write (i32.const 1) (i32.const 208) (i32.const 1) (i32.const 216))))
  (func (export "_start")
    ;; s: filestat_get("linkfile/") without SYMLINK_FOLLOW.
    (call $report (i32.const 115)
      (call $path_filestat_get (i32.const 3) (i32.const 0)
        (i32.const 0) (i32.const 9) (i32.const 128)))
    ;; t: readlink("linkfile/").
    (call $report (i32.const 116)
      (call $path_readlink (i32.const 3) (i32.const 0) (i32.const 9)
        (i32.const 64) (i32.const 32) (i32.const 100)))))
