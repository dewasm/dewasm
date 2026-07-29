// requires: wasi/errno_fs
// within reports whether path is base itself or lies under it (prefix
// containment against the already-realpath'd base).
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

// Resolve a guest-relative path against a directory fd to an absolute host
// path, confined to that directory fd's own (already-realpath'd) root. Every
// call re-validates against its own dirfd's root, so nested path_opens can't be
// used to launder an escape one level cheaper. Returns (hostPath, errno); an
// errno of wasiOk means success.
//
// followLast=false resolves the parent but leaves the final component
// untouched (the AT_SYMLINK_NOFOLLOW shape), for syscalls that operate on a
// symlink itself (lstat, unlink, rename, rmdir, mkdir). A trailing "." or ".."
// is never a symlink, so those fall back to full resolution.
//
// Known limitation (ADR-14): this is a check-then-open, not an atomic
// openat(2)-beneath resolution — a TOCTOU race or a symlink planted inside the
// sandbox between the check and the actual filesystem call could in principle
// escape. Accepted for a single-process research/demo runtime.
func (w *WASI) resolve_path(dirfd uint32, rel string, followLast bool) (string, uint32) {
    raw, present := w.fds[dirfd]
    if !present {
        return "", wasiBadf
    }
    entry, ok := raw.(*wasiDir)
    if !ok {
        // A base fd that exists but is a file, not a directory: NOTDIR, so a
        // guest opening a path underneath a plain file gets the POSIX errno
        // rather than BADF (ADR-40).
        return "", wasiNotdir
    }
    if strings.ContainsRune(rel, 0) {
        // A trailing NUL (or embedded NUL) is a malformed path: INVAL, matching
        // what the guests treat as ERRNO_INVAL/ILSEQ.
        return "", wasiInval
    }
    // A leading slash is an absolute guest path — never capable against a
    // preopen root (rejected before any join could absorb it; ADR-40).
    if strings.HasPrefix(rel, "/") {
        return "", wasiNotcapable
    }
    // A trailing slash constrains the final component to a directory (POSIX
    // pathname resolution; issue #42). filepath.Join Cleans it away *after*
    // the final-component bookkeeping below would have misread "" as the last
    // component (silently degrading followLast), so strip it up front and
    // re-check the constraint against the resolved target before returning.
    trailing := strings.HasSuffix(rel, "/")
    if trailing {
        rel = strings.TrimRight(rel, "/")
    }
    base := entry.hostPath
    joined := filepath.Join(base, rel)
    // Lexical containment on the cleaned path, before touching the filesystem:
    // catches a "../.." escape even when the (nonexistent) target would
    // otherwise fall through to a NOENT branch (the suite wants NOTCAPABLE).
    if !w.within(base, filepath.Clean(joined)) {
        return "", wasiNotcapable
    }
    // The final component as the *guest* wrote it — not filepath.Base(joined),
    // which Cleans "." / ".." away and would report the parent's own name (Go's
    // filepath.Join Cleans, unlike Python's os.path.join). A trailing "." or
    // ".." is never a symlink, so those must fall through to full resolution.
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
        if e := w.trailingDirGate(trailing, host); e != wasiOk {
            return "", e
        }
        return host, wasiOk
    }
    if _, err := os.Lstat(joined); err == nil {
        real, err := filepath.EvalSymlinks(joined)
        if err != nil {
            // A dangling symlink or a race: fall back to the literal path so
            // the containment check still runs against it.
            real = joined
        }
        if !w.within(base, real) {
            return "", wasiNotcapable
        }
        if e := w.trailingDirGate(trailing, real); e != wasiOk {
            return "", e
        }
        return real, wasiOk
    }
    // The final component is missing: resolve the parent and re-attach it, so a
    // create (path_open O_CREAT) still gets a sandboxed target path.
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

// trailingDirGate enforces the trailing-slash constraint on a resolved host
// path: a slash-suffixed guest name may only resolve to a directory, so an
// existing non-directory target is ENOTDIR (issue #42). os.Stat follows
// symlinks — the slash forces following, so "link-to-file/" is ENOTDIR while
// "link-to-dir/" passes. A missing target passes too: which errno (or
// success, for a create) that becomes is each syscall's own business.
func (w *WASI) trailingDirGate(trailing bool, host string) uint32 {
    if !trailing {
        return wasiOk
    }
    if fi, err := os.Stat(host); err == nil && !fi.IsDir() {
        return wasiNotdir
    }
    return wasiOk
}
