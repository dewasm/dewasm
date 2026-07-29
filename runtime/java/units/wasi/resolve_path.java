// requires: wasi/errno_fs
// within reports whether path is base itself or lies under it (component-wise
// containment against the already-realpath'd base — Path.startsWith is safer
// than a raw string prefix, so /base does not "contain" /baseball).
private static boolean within(java.nio.file.Path base, java.nio.file.Path path) {
    return path.equals(base) || path.startsWith(base);
}

// Resolve a guest-relative path against a directory fd to an absolute host
// path, confined to that directory fd's own (already-realpath'd) root. Every
// call re-validates against its own dirfd's root, so nested path_opens can't be
// used to launder an escape one level cheaper. Returns a Resolved; an errno of
// WASI_OK means success.
//
// The join mirrors Go's filepath.Join: an absolute `rel` is still confined
// under base (base + "/" + rel, then normalize), so a guest cannot escape by
// passing a leading slash. normalize() collapses "." / ".." lexically; the
// realpath + within() check below then rejects anything that still escapes.
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
Resolved resolve_path(int dirfd, String rel, boolean followLast) {
    Object e = fds.get(dirfd);
    if (!(e instanceof Dir)) {
        // A non-directory fd (a regular file, or stdio) used as a dirfd is
        // ENOTDIR; only an fd absent from the table is EBADF (ADR-40).
        return new Resolved(null, fds.containsKey(dirfd) ? WASI_NOTDIR : WASI_BADF);
    }
    Dir dir = (Dir) e;
    if (rel.indexOf('\0') >= 0) {
        return new Resolved(null, WASI_INVAL);
    }
    // A leading slash is an absolute path: WASI confines the guest to the
    // preopen tree, so it is rejected outright rather than reinterpreted
    // relative to the dirfd (the wasmtime behaviour interesting_paths asserts).
    if (rel.startsWith("/")) {
        return new Resolved(null, WASI_NOTCAPABLE);
    }
    java.nio.file.Path base = dir.hostPath;
    java.nio.file.Path joined = java.nio.file.Paths.get(base.toString() + "/" + rel).normalize();
    // Lexical containment gate: enough ".." components can normalize to a path
    // above base even though every prefix exists, so reject it here (NOTCAPABLE)
    // before the filesystem is consulted — otherwise a nonexistent escaped
    // target would surface as NOENT rather than the capability error the guest
    // expects. The realpath + within() checks below still catch symlink escapes.
    if (!within(base, joined)) {
        return new Resolved(null, WASI_NOTCAPABLE);
    }
    // A slash-suffixed name may only resolve to a directory (issue #42):
    // Paths.get has normalized the slash away, so enforce it here — the
    // probes follow symlinks as the slash requires; a missing target is each
    // syscall's case. The slash is also stripped from `last` below, which
    // would otherwise be "" and silently degrade followLast.
    boolean trailing = rel.endsWith("/");
    String core = rel;
    while (core.endsWith("/")) {
        core = core.substring(0, core.length() - 1);
    }
    if (trailing && java.nio.file.Files.exists(joined)
        && !java.nio.file.Files.isDirectory(joined)) {
        return new Resolved(null, WASI_NOTDIR);
    }
    // The final component as the *guest* wrote it (not joined.getFileName(),
    // which has Cleaned "." / ".." away). A trailing "." or ".." is never a
    // symlink, so those fall through to full resolution.
    String last = core;
    int idx = core.lastIndexOf('/');
    if (idx >= 0) {
        last = core.substring(idx + 1);
    }
    if (!followLast && !last.equals(".") && !last.equals("..") && !last.isEmpty()) {
        java.nio.file.Path parent = joined.getParent();
        java.nio.file.Path realParent;
        try {
            realParent = parent.toRealPath();
        } catch (java.io.IOException ex) {
            return new Resolved(null, WASI_NOENT);
        }
        if (!within(base, realParent)) {
            return new Resolved(null, WASI_NOTCAPABLE);
        }
        return new Resolved(realParent.resolve(last).toString(), WASI_OK);
    }
    if (java.nio.file.Files.exists(joined, java.nio.file.LinkOption.NOFOLLOW_LINKS)) {
        java.nio.file.Path real;
        try {
            real = joined.toRealPath();
        } catch (java.io.IOException ex) {
            // A dangling symlink or a race: fall back to the literal path so
            // the containment check still runs against it.
            real = joined;
        }
        if (!within(base, real)) {
            return new Resolved(null, WASI_NOTCAPABLE);
        }
        return new Resolved(real.toString(), WASI_OK);
    }
    // The final component is missing: resolve the parent and re-attach it, so a
    // create (path_open O_CREAT) still gets a sandboxed target path.
    java.nio.file.Path parent = joined.getParent();
    java.nio.file.Path realParent;
    try {
        realParent = parent.toRealPath();
    } catch (java.io.IOException ex) {
        return new Resolved(null, WASI_NOENT);
    }
    if (!within(base, realParent)) {
        return new Resolved(null, WASI_NOTCAPABLE);
    }
    return new Resolved(realParent.resolve(joined.getFileName()).toString(), WASI_OK);
}
