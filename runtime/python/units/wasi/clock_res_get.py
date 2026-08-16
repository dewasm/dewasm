# requires: memory/ids
def wasi_clock_res_get(self, id, out_ptr):
    self.memory.ids(out_ptr, 1)
    return self.ERRNO_SUCCESS
