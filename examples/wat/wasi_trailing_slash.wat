;; Trailing-slash pathname resolution (POSIX; issue #42): "name/" may only
;; resolve to a directory, and the shared runtimes must enforce that even
;; where their path plumbing (File.join, filepath.Join, Paths.get.normalize)
;; would silently drop the slash before the host syscall could see it. Every
;; probe prints "<tag><errno as two decimal digits>\n" so the expected stdout
;; pins each errno exactly. Host layout (from the case's setup): a regular
;; file "file", a second file "file2", a directory "dir".
;;   a — path_rename with a slash-suffixed regular-file *source* (54 ENOTDIR),
;;   b — path_rename with a slash-suffixed existing-file *destination* (54),
;;   c — path_unlink_file of "file/" (54),
;;   d — path_unlink_file of "missing/" (44 ENOENT),
;;   e/f — path_filestat_get of "file/" with/without SYMLINK_FOLLOW (54),
;;   g — path_open of "file/" read-only (54),
;;   h — path_remove_directory of "file/" (54),
;;   i — path_link with a slash-suffixed regular-file source (54),
;;   j — path_rename with a missing slash-suffixed source (44),
;;   k — path_rename of a file onto a *nonexistent* slash-suffixed
;;       destination (44 — normalized: macOS/POSIX ENOENT vs Linux ENOTDIR;
;;       every backend implements the check deterministically, and must not
;;       silently create a plain file at the name),
;;   l — path_open O_CREAT through a nonexistent slash-suffixed name (44 —
;;       normalized: macOS/POSIX ENOENT vs Linux EISDIR; must not create),
;;   m — path_create_directory of "file/" (20 EEXIST — normalized: macOS
;;       mkdir(2) says ENOTDIR, Linux EEXIST; the slash is stripped since
;;       mkdir names a directory by definition),
;;   n — path_create_directory of "newdir/" (00: the slash is legal here),
;;   o — path_remove_directory of "dir/" (00: ditto).
;; Deliberately untested (host-divergent symlink-through-slash arcana, kept
;; out per docs/testing.md): unlink/rename onto "symlink-to-dir/" (macOS
;; EPERM / Linux ENOTDIR — the same accepted split as unlinking a plain
;; directory) and O_NOFOLLOW opens of slash-suffixed symlinks.
;; Ground truth: wasmtime 47 agrees on a-j exactly. It diverges on the
;; normalized shapes — k succeeds by silently renaming through the slash
;; onto a plain *file* (the very bug class issue #29/#42 removes), l/o are
;; EINVAL — which is cap-std's slash stripping, not POSIX pathname
;; resolution (a slash-terminated nonexistent name is ENOENT; rmdir of
;; "dir/" resolves to the directory and succeeds), so the POSIX shapes are
;; pinned here, as ADR-40/PR #41 chose for rename.
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
  (data (i32.const 0) "file/")
  (data (i32.const 8) "renamed")
  (data (i32.const 16) "dir")
  (data (i32.const 20) "file2/")
  (data (i32.const 32) "missing/")
  (data (i32.const 40) "x")
  (data (i32.const 48) "lnk")
  (data (i32.const 56) "newd/")
  (data (i32.const 64) "newf/")
  (data (i32.const 72) "newdir/")
  (data (i32.const 80) "dir/")
  (data (i32.const 88) "file")
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
        (i32.const 3) (i32.const 20) (i32.const 6)))
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
    ;; f: filestat_get("file/") without SYMLINK_FOLLOW.
    (call $report (i32.const 102)
      (call $path_filestat_get (i32.const 3) (i32.const 0)
        (i32.const 0) (i32.const 5) (i32.const 128)))
    ;; g: open("file/") read-only (rights FD_READ).
    (call $report (i32.const 103)
      (call $path_open (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 5)
        (i32.const 0) (i64.const 0x2) (i64.const 0) (i32.const 0) (i32.const 96)))
    ;; h: remove_directory("file/").
    (call $report (i32.const 104)
      (call $path_remove_directory (i32.const 3) (i32.const 0) (i32.const 5)))
    ;; i: link("file/", "lnk") — slash-suffixed hard-link source.
    (call $report (i32.const 105)
      (call $path_link (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 5)
        (i32.const 3) (i32.const 48) (i32.const 3)))
    ;; j: rename("missing/", "x").
    (call $report (i32.const 106)
      (call $path_rename (i32.const 3) (i32.const 32) (i32.const 8)
        (i32.const 3) (i32.const 40) (i32.const 1)))
    ;; k: rename("file", "newd/") — nonexistent slash-suffixed destination,
    ;; non-directory source (normalized ENOENT).
    (call $report (i32.const 107)
      (call $path_rename (i32.const 3) (i32.const 88) (i32.const 4)
        (i32.const 3) (i32.const 56) (i32.const 5)))
    ;; l: open("newf/", O_CREAT, rights FD_READ|FD_WRITE) — must not create.
    (call $report (i32.const 108)
      (call $path_open (i32.const 3) (i32.const 0) (i32.const 64) (i32.const 5)
        (i32.const 0x1) (i64.const 0x42) (i64.const 0) (i32.const 0) (i32.const 96)))
    ;; m: create_directory("file/") — EEXIST, uniformly.
    (call $report (i32.const 109)
      (call $path_create_directory (i32.const 3) (i32.const 0) (i32.const 5)))
    ;; n: create_directory("newdir/") — the slash is legal on mkdir.
    (call $report (i32.const 110)
      (call $path_create_directory (i32.const 3) (i32.const 72) (i32.const 7)))
    ;; o: remove_directory("dir/") — the slash is legal on a directory.
    (call $report (i32.const 111)
      (call $path_remove_directory (i32.const 3) (i32.const 80) (i32.const 4)))))
