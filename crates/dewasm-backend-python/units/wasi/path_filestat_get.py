# requires: memory/read_string, memory/init, wasi/resolve_path, wasi/pack_filestat, wasi/errno_fs
def wasi_path_filestat_get(self, dirfd, flags, path_ptr, path_len, buf_ptr):
    rel = self.memory.read_string(path_ptr, path_len).decode("utf-8", "surrogateescape")
    symlink_follow = flags & 0x1 != 0  # lookupflags::SYMLINK_FOLLOW
    host_path, err = self.resolve_path(dirfd, rel, symlink_follow)
    if err is not None:
        return err
    try:
        st = os.stat(host_path) if symlink_follow else os.lstat(host_path)
    except OSError as e:
        return self.fs_errno(e)
    self.memory.init(buf_ptr, self.pack_filestat(st), 0, 64)
    return self.ERRNO_SUCCESS
