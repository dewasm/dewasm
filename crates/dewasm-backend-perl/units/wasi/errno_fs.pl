# File system `errno` codes, kept out of the always-bundled `wasi/_package` prelude.
# A stdio-only WASI module (no `path_*` or file system `fd_*` imports) then doesn't carry them.
# ERRNO_NOTCAPABLE (76) lives in the prelude.
# That is because rights checks in `fd_read`, `fd_write`, etc. need it.
# That holds even when `errno_fs` is not otherwise bundled.
use Errno ();

use constant {
    ERRNO_ACCES => 2,
    ERRNO_EXIST => 20,
    ERRNO_ISDIR => 31,
    ERRNO_LOOP => 32,
    ERRNO_NAMETOOLONG => 37,
    ERRNO_NOENT => 44,
    ERRNO_NOTDIR => 54,
    ERRNO_NOTEMPTY => 55,
    ERRNO_PERM => 63,
};

# One table from host `errno` to WASI `errno`, shared by every file system call.
# The same host error then never maps to different codes depending on which system call raised it.
# Callers pass `0 + $!` right after the failed call, since the next call can clobber `$!`.
our %FS_ERRNO = (
    Errno::EACCES() => ERRNO_ACCES,
    Errno::EBADF() => ERRNO_BADF,
    Errno::EEXIST() => ERRNO_EXIST,
    Errno::EINVAL() => ERRNO_INVAL,
    Errno::EISDIR() => ERRNO_ISDIR,
    Errno::ELOOP() => ERRNO_LOOP,
    Errno::ENAMETOOLONG() => ERRNO_NAMETOOLONG,
    Errno::ENOENT() => ERRNO_NOENT,
    Errno::ENOTDIR() => ERRNO_NOTDIR,
    Errno::ENOTEMPTY() => ERRNO_NOTEMPTY,
    Errno::EPERM() => ERRNO_PERM,
    Errno::ESPIPE() => ERRNO_SPIPE,
);

sub fs_errno {
    my ($self, $e) = @_;
    return $FS_ERRNO{$e} // ERRNO_IO;
}
