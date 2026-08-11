# requires: wasi/errno_fs
def within(self, base, path):
    prefix = base if base.endswith(os.sep) else base + os.sep
    return path == base or path.startswith(prefix)

# Resolves a guest-relative path against a directory fd to an absolute host path, confined to that directory fd's own (already-realpath'd) root.
# Every call re-validates against its own dirfd's root, so nested path_opens can't be used to launder an escape one level cheaper.
#
# A non-directory base fd is NOTDIR (a file used as a dirfd); an absent one is
# BADF.
# A leading "/" is NOTCAPABLE before any join (an absolute guest path escapes the preopen).
# A trailing slash is preserved on the returned host path so the underlying os.* call enforces the POSIX "must be a directory" rule.
#
# `follow_last=False` resolves the parent but leaves the final component untouched (the AT_SYMLINK_NOFOLLOW shape), for syscalls that operate on a symlink itself (lstat, unlink, rename, rmdir, mkdir, link, symlink, readlink).
# A trailing "." or ".." is never a symlink, so those fall back to full resolution.
#
# Known limitation: this is a check-then-open, not an atomic openat(2)-beneath resolution: a TOCTOU race or a symlink planted inside the sandbox between the check and the actual filesystem call could in principle escape.
# Accepted for a single-process research/demo runtime, not a multi-tenant sandbox host.
def resolve_path(self, dirfd, rel, follow_last=True):
    if dirfd not in self.fds:
        return (None, self.ERRNO_BADF)
    if not isinstance(self.fds[dirfd], self.WasiDir):
        return (None, self.ERRNO_NOTDIR)
    if "\x00" in rel:
        return (None, self.ERRNO_INVAL)
    if rel.startswith("/"):
        return (None, self.ERRNO_NOTCAPABLE)
    base = self.fds[dirfd].host_path
    trailing = len(rel) > 1 and rel.endswith("/")
    core = rel.rstrip("/")
    joined = os.path.join(base, core) if core else base
    last = os.path.basename(joined)
    if not follow_last and last != "." and last != "..":
        # Containment is checked before existence: a path whose "..s" escape the sandbox is NOTCAPABLE even when the escaped-to parent does not exist.
        real_parent = os.path.realpath(os.path.dirname(joined))
        if not self.within(base, real_parent):
            return (None, self.ERRNO_NOTCAPABLE)
        if not os.path.exists(real_parent):
            return (None, self.ERRNO_NOENT)
        host = os.path.join(real_parent, last)
        return (host + os.sep if trailing else host, None)
    # A final "." resolves to its parent, but os.path.join left "<parent>/.", which os.path.lexists reports missing (and os.open rejects with ENOTDIR)
    # once that parent is a non-directory preopen: wasi-libc rewrites a path that *is* a preopen, e.g. the zeroperl reactor's "/dev/null", to ".".
    # Ruby's File.realpath collapses it and Go's filepath.Join cleans it; do the same, after `last` has had its say above so the symlink rules are unchanged.
    if last == ".":
        joined = os.path.dirname(joined)
    if os.path.lexists(joined):
        real = os.path.realpath(joined)
        if not self.within(base, real):
            return (None, self.ERRNO_NOTCAPABLE)
        return (real + os.sep if trailing else real, None)
    # The final component is missing: resolve the parent and re-attach it, so a create (path_open O_CREAT) still gets a sandboxed target path.
    real_parent = os.path.realpath(os.path.dirname(joined))
    if not self.within(base, real_parent):
        return (None, self.ERRNO_NOTCAPABLE)
    if not os.path.exists(real_parent):
        return (None, self.ERRNO_NOENT)
    host = os.path.join(real_parent, os.path.basename(joined))
    return (host + os.sep if trailing else host, None)
