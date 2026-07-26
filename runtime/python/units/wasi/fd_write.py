# requires: memory/i32_load, memory/i32_store, memory/read_string
def wasi_fd_write(self, fd, iovs_ptr, iovs_len, nwritten_ptr):
    io = self.fds.get(fd)
    if io is None:
        return self.ERRNO_BADF
    written = 0
    try:
        for i in range(iovs_len):
            ptr = self.memory.i32_load(iovs_ptr + i * 8)
            length = self.memory.i32_load(iovs_ptr + i * 8 + 4)
            written += io.write(self.memory.read_string(ptr, length))
        io.flush()
    except OSError:
        return self.ERRNO_IO
    self.memory.i32_store(nwritten_ptr, written)
    return self.ERRNO_SUCCESS
