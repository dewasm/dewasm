# requires: memory/i32_store
def wasi_args_sizes_get(self, argc_ptr, buf_size_ptr):
    self.memory.i32_store(argc_ptr, len(self.args))
    self.memory.i32_store(buf_size_ptr, sum(len(a) + 1 for a in self.args))
    return self.ERRNO_SUCCESS
