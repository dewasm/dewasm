# requires: memory/iwl, memory/iws, memory/read_string
def wasi_fd_write(self, fd, iovs_ptr, iovs_len, nwritten_ptr):
    io = self.fds.get(fd)
    if io is None or isinstance(io, self.WasiDir):
        return self.ERRNO_BADF
    if not (self.fd_meta[fd][0] & self.RIGHTS_FD_WRITE):
        return self.ERRNO_NOTCAPABLE
    written = 0
    try:
        # fdflags::APPEND is honoured here (not via O_APPEND on the OS handle) so fd_fdstat_set_flags(0) can turn it back off.
        if self.fd_meta[fd][2] & 0x1 and io not in self.std_ios:
            io.seek(0, os.SEEK_END)
        for i in range(iovs_len):
            ptr = self.memory.iwl(iovs_ptr + i * 8)
            length = self.memory.iwl(iovs_ptr + i * 8 + 4)
            written += io.write(self.memory.read_string(ptr, length))
        io.flush()
    except OSError:
        return self.ERRNO_IO
    self.memory.iws(nwritten_ptr, written)
    return self.ERRNO_SUCCESS
