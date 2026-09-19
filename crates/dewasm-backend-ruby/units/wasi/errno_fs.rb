# Filesystem-only errno codes: kept out of the always-bundled wasi/_class prelude so a stdio-only WASI module (no path_* / fs-only fd_* imports) doesn't carry them.
ERRNO_ACCES = 2
ERRNO_EXIST = 20
ERRNO_ISDIR = 31
ERRNO_LOOP = 32
ERRNO_NAMETOOLONG = 37
ERRNO_NOENT = 44
ERRNO_NOTDIR = 54
ERRNO_NOTEMPTY = 55
ERRNO_PERM = 63
# ERRNO_NOTCAPABLE (76) lives in the always-bundled wasi/_class prelude, since the per-fd rights model raises it from stdio-core fd_* units too.

# One SystemCallError-to-WASI-errno mapping shared by every filesystem syscall, so the same host error never maps to different codes depending on which syscall raised it.
# It is a rescue dispatch rather than a lookup table because a `rescue` clause is the one place an `Errno` class survives ahead-of-time compilation of this source; named as a value (a Hash key, a `case/when` operand) it is a constant the compiler need not have resolved.
# Every caller rescues SystemCallError, so the last clause is total.
def fs_errno(e)
  raise e
rescue Errno::EACCES then ERRNO_ACCES
rescue Errno::EBADF then ERRNO_BADF
rescue Errno::EEXIST then ERRNO_EXIST
rescue Errno::EINVAL then ERRNO_INVAL
rescue Errno::EISDIR then ERRNO_ISDIR
rescue Errno::ELOOP then ERRNO_LOOP
rescue Errno::ENAMETOOLONG then ERRNO_NAMETOOLONG
rescue Errno::ENOENT then ERRNO_NOENT
rescue Errno::ENOTDIR then ERRNO_NOTDIR
rescue Errno::ENOTEMPTY then ERRNO_NOTEMPTY
rescue Errno::EPERM then ERRNO_PERM
rescue Errno::ESPIPE then ERRNO_SPIPE
rescue SystemCallError then ERRNO_IO
end
private :fs_errno
