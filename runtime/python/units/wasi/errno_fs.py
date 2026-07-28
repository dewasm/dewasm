# Filesystem-only errno codes (ADR-14): kept out of the always-bundled
# wasi/_class prelude so a stdio-only WASI module (no path_* / fs-only
# fd_* imports) doesn't carry them.
ERRNO_ACCES = 2
ERRNO_EXIST = 20
ERRNO_ISDIR = 31
ERRNO_LOOP = 32
ERRNO_NAMETOOLONG = 37
ERRNO_NOENT = 44
ERRNO_NOTDIR = 54
ERRNO_NOTEMPTY = 55
ERRNO_PERM = 63
# ERRNO_NOTCAPABLE (76) lives in the wasi/_class prelude: rights enforcement in
# fd_read/fd_write/etc. needs it even when errno_fs is not otherwise bundled.

# One OSError.errno-to-WASI-errno table shared by every filesystem syscall,
# so the same host error never maps to different codes depending on which
# syscall raised it.
FS_ERRNO = {
    errno.EACCES: ERRNO_ACCES,
    errno.EBADF: ERRNO_BADF,
    errno.EEXIST: ERRNO_EXIST,
    errno.EINVAL: ERRNO_INVAL,
    errno.EISDIR: ERRNO_ISDIR,
    errno.ELOOP: ERRNO_LOOP,
    errno.ENAMETOOLONG: ERRNO_NAMETOOLONG,
    errno.ENOENT: ERRNO_NOENT,
    errno.ENOTDIR: ERRNO_NOTDIR,
    errno.ENOTEMPTY: ERRNO_NOTEMPTY,
    errno.EPERM: ERRNO_PERM,
    errno.ESPIPE: ERRNO_SPIPE,
}

def fs_errno(self, e):
    return self.FS_ERRNO.get(e.errno, self.ERRNO_IO)
