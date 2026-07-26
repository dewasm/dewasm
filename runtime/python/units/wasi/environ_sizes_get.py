# requires: memory/i32_store
def wasi_environ_sizes_get(self, count_ptr, buf_size_ptr):
    self.memory.i32_store(count_ptr, len(self.env))
    self.memory.i32_store(buf_size_ptr, sum(len(e) + 1 for e in self.env))
    return self.ERRNO_SUCCESS
