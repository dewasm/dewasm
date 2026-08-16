# requires: memory/iws
def wasi_args_sizes_get(self, argc_ptr, buf_size_ptr):
    self.memory.iws(argc_ptr, len(self.args))
    self.memory.iws(buf_size_ptr, sum(len(a) + 1 for a in self.args))
    return self.ERRNO_SUCCESS
