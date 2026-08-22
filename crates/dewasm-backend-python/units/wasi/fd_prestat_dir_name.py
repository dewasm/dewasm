# requires: memory/init, wasi/errno_fs
def wasi_fd_prestat_dir_name(self, fd, path_ptr, path_len):
    entry = self.fds.get(fd)
    if not isinstance(entry, self.WasiDir) or entry.preopen_name is None:
        return self.ERRNO_BADF
    name = entry.preopen_name
    if len(name) > path_len:
        return self.ERRNO_NAMETOOLONG
    self.memory.init(path_ptr, name, 0, len(name))
    return self.ERRNO_SUCCESS
