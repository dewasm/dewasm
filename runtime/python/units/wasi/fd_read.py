# requires: memory/i32_load, memory/i32_store, memory/init
def wasi_fd_read(self, fd, iovs_ptr, iovs_len, nread_ptr):
    io = self.fds.get(fd)
    if io is None:
        return self.ERRNO_BADF
    nread = 0
    try:
        for i in range(iovs_len):
            ptr = self.memory.i32_load(iovs_ptr + i * 8)
            length = self.memory.i32_load(iovs_ptr + i * 8 + 4)
            if length == 0:
                continue
            chunk = io.read(length)
            if not chunk:
                break
            self.memory.init(ptr, chunk, 0, len(chunk))
            nread += len(chunk)
            if len(chunk) < length:
                break
    except OSError:
        return self.ERRNO_IO
    self.memory.i32_store(nread_ptr, nread)
    return self.ERRNO_SUCCESS
