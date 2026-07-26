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
        return new Resolved(null, WASI_BADF);
    }
    Dir dir = (Dir) e;
    if (rel.indexOf('\0') >= 0) {
        return new Resolved(null, WASI_PERM);
    }
    java.nio.file.Path base = dir.hostPath;
    java.nio.file.Path joined = java.nio.file.Paths.get(base.toString() + "/" + rel).normalize();
    // The final component as the *guest* wrote it (not joined.getFileName(),
    // which has Cleaned "." / ".." away). A trailing "." or ".." is never a
    // symlink, so those fall through to full resolution.
    String last = rel;
    int idx = rel.lastIndexOf('/');
    if (idx >= 0) {
        last = rel.substring(idx + 1);
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
