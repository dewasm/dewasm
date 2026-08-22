# requires: memory/init, wasi/pack_filestat
def wasi_fd_filestat_get(self, fd, buf_ptr):
    entry = self.fds.get(fd)
    if entry is None:
        return self.ERRNO_BADF
    try:
        if isinstance(entry, self.WasiDir):
            st = os.stat(entry.host_path)
        else:
            st = os.fstat(entry.fileno())
    except OSError:
        return self.ERRNO_IO
    self.memory.init(buf_ptr, self.pack_filestat(st), 0, 64)
    return self.ERRNO_SUCCESS
