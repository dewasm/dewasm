# requires: memory/i64_store
def wasi_clock_res_get(self, id, out_ptr):
    self.memory.i64_store(out_ptr, 1)
    return self.ERRNO_SUCCESS
