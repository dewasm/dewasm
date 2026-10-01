// requires: wasi/errno_fs
// within reports whether path is base itself or lies under it.
// It checks prefix containment against the base, which `realpath` has already resolved.
func (w *WASI) within(base, path string) bool {
    if path == base {
        return true
    }
    prefix := base
    if !strings.HasSuffix(prefix, string(os.PathSeparator)) {
        prefix += string(os.PathSeparator)
    }
    return strings.HasPrefix(path, prefix)
}

// Resolve a guest-relative path against a directory `fd` to an absolute host path.
// The result is confined to that directory `fd`'s own root (already resolved by `realpath`).
// Every call re-validates against its own `dirfd`'s root.
// So nested `path_open` calls can't be used to launder an escape one level cheaper.
// Returns (`hostPath`, `errno`); an `errno` of `wasiOk` means success.
//
// `followLast=false` resolves the parent but leaves the final component untouched.
// That is the AT_SYMLINK_NOFOLLOW shape, for system calls that operate on a symbolic link itself.
// Those are `lstat`, `unlink`, `rename`, `rmdir`, `mkdir`, `link`, `symlink`, and `readlink`.
// A trailing "." or ".." is never a symbolic link, so those fall back to full resolution.
//
// Known limitation: this is a check-then-open, not an atomic `openat(2)`-beneath resolution.
// A TOCTOU race could in principle escape.
// So could a symbolic link planted inside the sandbox between the check and the file system call.
// Accepted for a single-process research/example runtime.
func (w *WASI) resolve_path(dirfd uint32, rel string, followLast bool) (string, uint32) {
    raw, present := w.fds[dirfd]
    if !present {
        return "", wasiBadf
    }
    entry, ok := raw.(*wasiDir)
    if !ok {
        // A base `fd` that exists but is a file, not a directory: NOTDIR.
        // So a guest opening a path under a plain file gets the POSIX `errno` rather than BADF.
        return "", wasiNotdir
    }
    if strings.ContainsRune(rel, 0) {
        // A trailing NUL (or embedded NUL) is a malformed path: INVAL.
        // That matches what the guests treat as ERRNO_INVAL/ILSEQ.
        return "", wasiInval
    }
    // A leading slash is an absolute guest path, never capable against a preopen root.
    // It is rejected before any join could absorb it.
    if strings.HasPrefix(rel, "/") {
        return "", wasiNotcapable
    }
    // Strip a trailing slash before the handling of the final component below.
    // An empty last component silently degrades `followLast`.
    // Then re-check via `trailingDirCheck` (issue #42).
    trailing := strings.HasSuffix(rel, "/")
    if trailing {
        rel = strings.TrimRight(rel, "/")
    }
    base := entry.hostPath
    joined := filepath.Join(base, rel)
    // Lexical containment on the cleaned path, before touching the file system.
    // It catches a "../.." escape even when the (nonexistent) target would reach a NOENT branch.
    // The suite wants NOTCAPABLE there.
    if !w.within(base, filepath.Clean(joined)) {
        return "", wasiNotcapable
    }
    // The final component as the *guest* wrote it, not filepath.Base(joined).
    // filepath.Base Cleans "." / ".." away and would report the parent's own name.
    // Go's filepath.Join Cleans, unlike Python's os.path.join.
    // A trailing "." or ".." is never a symbolic link.
    // So those must fall through to full resolution.
    last := rel
    if idx := strings.LastIndexByte(rel, '/'); idx >= 0 {
        last = rel[idx+1:]
    }
    if !followLast && last != "." && last != ".." && last != "" {
        parent := filepath.Dir(joined)
        realParent, err := filepath.EvalSymlinks(parent)
        if err != nil {
            return "", wasiNoent
        }
        if !w.within(base, realParent) {
            return "", wasiNotcapable
        }
        host := filepath.Join(realParent, last)
        if e := w.trailingDirCheck(trailing, host); e != wasiOk {
            return "", e
        }
        return host, wasiOk
    }
    if _, err := os.Lstat(joined); err == nil {
        real, err := filepath.EvalSymlinks(joined)
        if err != nil {
            // A dangling symbolic link or a race: fall back to the literal path.
            // The containment check then still runs against it.
            real = joined
        }
        if !w.within(base, real) {
            return "", wasiNotcapable
        }
        if e := w.trailingDirCheck(trailing, real); e != wasiOk {
            return "", e
        }
        return real, wasiOk
    }
    // The final component is missing: resolve the parent and re-attach it.
    // So a create (`path_open` O_CREAT) still gets a sandboxed target path.
    parent := filepath.Dir(joined)
    realParent, err := filepath.EvalSymlinks(parent)
    if err != nil {
        return "", wasiNoent
    }
    if !w.within(base, realParent) {
        return "", wasiNotcapable
    }
    return filepath.Join(realParent, filepath.Base(joined)), wasiOk
}

// `trailingDirCheck`: a slash-suffixed name may only resolve to a directory.
// An existing non-directory is ENOTDIR (issue #42).
// `os.Stat` follows symbolic links, as the slash requires.
// A missing target is each caller's case.
func (w *WASI) trailingDirCheck(trailing bool, host string) uint32 {
    if !trailing {
        return wasiOk
    }
    if fi, err := os.Stat(host); err == nil && !fi.IsDir() {
        return wasiNotdir
    }
    return wasiOk
}
