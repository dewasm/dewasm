# Filesystem-only errno codes: kept out of the always-bundled
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
# ERRNO_NOTCAPABLE (76) lives in the always-bundled wasi/_class prelude,
# since the per-fd rights model raises it from stdio-core fd_* units too.

# One SystemCallError-to-WASI-errno table shared by every filesystem
# syscall, so the same host error never maps to different codes
# depending on which syscall raised it.
FS_ERRNO = {
  Errno::EACCES => ERRNO_ACCES,
  Errno::EBADF => ERRNO_BADF,
  Errno::EEXIST => ERRNO_EXIST,
  Errno::EINVAL => ERRNO_INVAL,
  Errno::EISDIR => ERRNO_ISDIR,
  Errno::ELOOP => ERRNO_LOOP,
  Errno::ENAMETOOLONG => ERRNO_NAMETOOLONG,
  Errno::ENOENT => ERRNO_NOENT,
  Errno::ENOTDIR => ERRNO_NOTDIR,
  Errno::ENOTEMPTY => ERRNO_NOTEMPTY,
  Errno::EPERM => ERRNO_PERM,
  Errno::ESPIPE => ERRNO_SPIPE,
}.freeze

def fs_errno(e)
  FS_ERRNO.fetch(e.class, ERRNO_IO)
end
private :fs_errno
