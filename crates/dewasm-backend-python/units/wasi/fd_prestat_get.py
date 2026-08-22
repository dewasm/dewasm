# requires: memory/init
def wasi_fd_prestat_get(self, fd, out_ptr):
    entry = self.fds.get(fd)
    if not isinstance(entry, self.WasiDir) or entry.preopen_name is None:
        return self.ERRNO_BADF
    # prestat: tag (u8, 0 = dir) + 3 pad + pr_name_len (u32).
    self.memory.init(out_ptr, struct.pack("<BxxxI", 0, len(entry.preopen_name)), 0, 8)
    return self.ERRNO_SUCCESS
