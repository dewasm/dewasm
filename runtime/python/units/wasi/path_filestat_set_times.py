# requires: memory/read_string, wasi/resolve_path, wasi/fst_times, wasi/errno_fs
def wasi_path_filestat_set_times(self, fd, flags, path_ptr, path_len, atim, mtim, fst_flags):
    rel = self.memory.read_string(path_ptr, path_len).decode("utf-8", "surrogateescape")
    follow = flags & 0x1 != 0  # lookupflags::SYMLINK_FOLLOW
    host, err = self.resolve_path(fd, rel, follow)
    if err is not None:
        return err
    try:
        st = os.stat(host) if follow else os.lstat(host)
    except OSError as e:
        return self.fs_errno(e)
    times, err = self.fst_times(st, atim, mtim, fst_flags)
    if err is not None:
        return err
    try:
        os.utime(host, ns=times, follow_symlinks=follow)
    except OSError as e:
        return self.fs_errno(e)
    return self.ERRNO_SUCCESS
