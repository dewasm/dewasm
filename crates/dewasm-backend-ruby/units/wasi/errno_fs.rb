# `errno` codes only file system calls use, kept out of the always-bundled `wasi/_class` prelude.
# So a stdio-only WASI module (no `path_*` or file system `fd_*` imports) doesn't carry them.
ERRNO_ACCES = 2
ERRNO_EXIST = 20
ERRNO_ISDIR = 31
ERRNO_LOOP = 32
ERRNO_NAMETOOLONG = 37
ERRNO_NOENT = 44
ERRNO_NOTDIR = 54
ERRNO_NOTEMPTY = 55
ERRNO_PERM = 63
# ERRNO_NOTCAPABLE (76) lives in the always-bundled `wasi/_class` prelude.
# That is since the per-descriptor rights model raises it from stdio-core `fd_*` units too.

# One mapping from `SystemCallError` to WASI `errno`, shared by every file system call.
# So the same host error never maps to different codes depending on which system call raised it.
# It is a rescue dispatch rather than a lookup table.
# Only in a `rescue` clause does an `Errno` class survive ahead-of-time compilation of this source.
# An `Errno` class named as a value is a constant the compiler need not have resolved.
# A Hash key and a `case/when` operand name it as a value.
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
