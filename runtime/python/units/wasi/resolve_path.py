# requires: wasi/errno_fs
def within(self, base, path):
    prefix = base if base.endswith(os.sep) else base + os.sep
    return path == base or path.startswith(prefix)

# Resolves a guest-relative path against a directory fd to an absolute host
# path, confined to that directory fd's own (already-realpath'd) root. Every
# call re-validates against its own dirfd's root, so nested path_opens can't be
# used to launder an escape one level cheaper.
#
# `follow_last=False` resolves the parent but leaves the final component
# untouched (the AT_SYMLINK_NOFOLLOW shape), for syscalls that operate on a
# symlink itself (lstat, unlink, rename, rmdir, mkdir). A trailing "." or ".."
# is never a symlink, so those fall back to full resolution.
#
# Known limitation (ADR-14): this is a check-then-open, not an atomic
# openat(2)-beneath resolution — a TOCTOU race or a symlink planted inside the
# sandbox between the check and the actual filesystem call could in principle
# escape. Accepted for a single-process research/demo runtime, not a
# multi-tenant sandbox host.
def resolve_path(self, dirfd, rel, follow_last=True):
    entry = self.fds.get(dirfd)
    if not isinstance(entry, self.WasiDir):
        return (None, self.ERRNO_BADF)
    if "\x00" in rel:
        return (None, self.ERRNO_PERM)
    base = entry.host_path
    joined = os.path.join(base, rel)
    last = os.path.basename(joined)
    if not follow_last and last != "." and last != "..":
        parent = os.path.dirname(joined)
        if not os.path.exists(parent):
            return (None, self.ERRNO_NOENT)
        real_parent = os.path.realpath(parent)
        if not self.within(base, real_parent):
            return (None, self.ERRNO_NOTCAPABLE)
        return (os.path.join(real_parent, last), None)
    if os.path.lexists(joined):
        real = os.path.realpath(joined)
        if not self.within(base, real):
            return (None, self.ERRNO_NOTCAPABLE)
        return (real, None)
    # The final component is missing: resolve the parent and re-attach it, so
    # a create (path_open O_CREAT) still gets a sandboxed target path.
    parent = os.path.dirname(joined)
    if not os.path.exists(parent):
        return (None, self.ERRNO_NOENT)
    real_parent = os.path.realpath(parent)
    if not self.within(base, real_parent):
        return (None, self.ERRNO_NOTCAPABLE)
    return (os.path.join(real_parent, os.path.basename(joined)), None)
