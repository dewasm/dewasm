;; Trailing-slash pathname resolution (issue #42, ADR-49): the WASI spec is
;; silent about trailing slashes, so dewasm follows wasmtime 47's observed
;; behavior — every expected errno below was measured against wasmtime on
;; both macOS and Linux (they agree except where noted). The runtimes must
;; enforce these shapes even where their path plumbing (File.join,
;; filepath.Join, Paths.get.normalize) would silently drop the slash before
;; the host syscall could see it. Every probe prints "<tag><errno as two
;; decimal digits>\n" so the expected stdout pins each errno exactly. Host
;; layout (from the case's setup): a regular file "file", a second file
;; "file2", a directory "dir".
;;   a — path_rename with a slash-suffixed regular-file *source* (54 ENOTDIR),
;;   b — path_rename with a slash-suffixed existing-file *destination* (54),
;;   c — path_unlink_file of "file/" (54),
;;   d — path_unlink_file of "missing/" (44 ENOENT),
;;   e — path_filestat_get of "file/" with SYMLINK_FOLLOW (54),
;;   f — path_open of "file/" read-only (54),
;;   g — path_remove_directory of "file/" (54),
;;   h — path_link with a slash-suffixed regular-file source (54),
;;   i — path_rename with a missing slash-suffixed source (44),
;;   j — path_rename of a file onto a *nonexistent* slash-suffixed
;;       destination (00: wasmtime's cap-std strips the slash and renames —
;;       "file2" becomes a plain file "newd"),
;;   k — path_open O_CREAT through a nonexistent slash-suffixed name
;;       (host-split, as wasmtime: 28 EINVAL on macOS from its manual path
;;       resolution, 31 EISDIR on Linux via host passthrough; must not
;;       create),
;;   l — path_create_directory of "file/" (20 EEXIST: the slash is stripped —
;;       mkdir names a directory by definition — matching wasmtime),
;;   m — path_create_directory of "newdir/" (00: the slash is legal here),
;;   n — path_remove_directory of "missing/" (44),
;;   o — path_remove_directory of "dir/" (28 EINVAL on both hosts —
;;       wasmtime/cap-std rejects removing a directory through a slash —
;;       and "dir" must survive),
;;   p — path_remove_directory of "dir" (00: the bare name still works).
;; Deliberately untested, per ADR-49's "copy wasmtime, don't invent" rule:
;; path_filestat_get of "file/" *without* SYMLINK_FOLLOW — wasmtime-on-Linux
;; strips the slash internally and succeeds (00) where wasmtime-on-macOS and
;; even a raw Linux lstat(2) say ENOTDIR, an artifact with no portable
;; expectation — and the unlink/rename-of-a-directory shapes whose errno
;; wasmtime inherits from the host split (macOS EPERM / Linux EISDIR),
;; which the upstream wasi-testsuite already pins per host under the
;; strict errno modes.
(module
  (import "wasi_snapshot_preview1" "path_rename"
    (func $path_rename (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_unlink_file"
    (func $path_unlink_file (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_filestat_get"
    (func $path_filestat_get (param i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_open"
    (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_remove_directory"
    (func $path_remove_directory (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_link"
    (func $path_link (param i32 i32 i32 i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "path_create_directory"
    (func $path_create_directory (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  ;; Prefix reuse: "file/" also serves "file" (len 4), "dir/" serves "dir"
  ;; (len 3), "file2/" serves "file2" (len 5).
  (data (i32.const 0) "file/")
  (data (i32.const 8) "renamed")
  (data (i32.const 16) "dir/")
  (data (i32.const 24) "file2/")
  (data (i32.const 32) "missing/")
  (data (i32.const 40) "x")
  (data (i32.const 48) "lnk")
  (data (i32.const 56) "newd/")
  (data (i32.const 64) "newf/")
  (data (i32.const 72) "newdir/")
  ;; 96: opened-fd cell, 128..191: filestat buffer, 200: 4-byte message,
  ;; 208: message iovec, 216: message nwritten cell.
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
    ;; a: rename("file/", "renamed") — slash-suffixed non-directory source.
    (call $report (i32.const 97)
      (call $path_rename (i32.const 3) (i32.const 0) (i32.const 5)
        (i32.const 3) (i32.const 8) (i32.const 7)))
    ;; b: rename("dir", "file2/") — slash-suffixed existing-file destination.
    (call $report (i32.const 98)
      (call $path_rename (i32.const 3) (i32.const 16) (i32.const 3)
        (i32.const 3) (i32.const 24) (i32.const 6)))
    ;; c: unlink("file/").
    (call $report (i32.const 99)
      (call $path_unlink_file (i32.const 3) (i32.const 0) (i32.const 5)))
    ;; d: unlink("missing/") — missing beats the slash: ENOENT.
    (call $report (i32.const 100)
      (call $path_unlink_file (i32.const 3) (i32.const 32) (i32.const 8)))
    ;; e: filestat_get("file/") with SYMLINK_FOLLOW.
    (call $report (i32.const 101)
      (call $path_filestat_get (i32.const 3) (i32.const 1)
        (i32.const 0) (i32.const 5) (i32.const 128)))
    ;; f: open("file/") read-only (rights FD_READ).
    (call $report (i32.const 102)
      (call $path_open (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 5)
        (i32.const 0) (i64.const 0x2) (i64.const 0) (i32.const 0) (i32.const 96)))
    ;; g: remove_directory("file/").
    (call $report (i32.const 103)
      (call $path_remove_directory (i32.const 3) (i32.const 0) (i32.const 5)))
    ;; h: link("file/", "lnk") — slash-suffixed hard-link source.
    (call $report (i32.const 104)
      (call $path_link (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 5)
        (i32.const 3) (i32.const 48) (i32.const 3)))
    ;; i: rename("missing/", "x").
    (call $report (i32.const 105)
      (call $path_rename (i32.const 3) (i32.const 32) (i32.const 8)
        (i32.const 3) (i32.const 40) (i32.const 1)))
    ;; j: rename("file2", "newd/") — nonexistent slash-suffixed destination:
    ;; the slash is stripped and the rename proceeds (wasmtime).
    (call $report (i32.const 106)
      (call $path_rename (i32.const 3) (i32.const 24) (i32.const 5)
        (i32.const 3) (i32.const 56) (i32.const 5)))
    ;; k: open("newf/", O_CREAT, rights FD_READ|FD_WRITE) — must not create;
    ;; EINVAL on macOS, EISDIR on Linux (wasmtime's own split).
    (call $report (i32.const 107)
      (call $path_open (i32.const 3) (i32.const 0) (i32.const 64) (i32.const 5)
        (i32.const 0x1) (i64.const 0x42) (i64.const 0) (i32.const 0) (i32.const 96)))
    ;; l: create_directory("file/") — EEXIST, uniformly.
    (call $report (i32.const 108)
      (call $path_create_directory (i32.const 3) (i32.const 0) (i32.const 5)))
    ;; m: create_directory("newdir/") — the slash is legal on mkdir.
    (call $report (i32.const 109)
      (call $path_create_directory (i32.const 3) (i32.const 72) (i32.const 7)))
    ;; n: remove_directory("missing/").
    (call $report (i32.const 110)
      (call $path_remove_directory (i32.const 3) (i32.const 32) (i32.const 8)))
    ;; o: remove_directory("dir/") — EINVAL, and "dir" survives.
    (call $report (i32.const 111)
      (call $path_remove_directory (i32.const 3) (i32.const 16) (i32.const 4)))
    ;; p: remove_directory("dir") — the bare name still removes it.
    (call $report (i32.const 112)
      (call $path_remove_directory (i32.const 3) (i32.const 16) (i32.const 3)))))
