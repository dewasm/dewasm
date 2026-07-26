# requires: memory/i64_store
def wasi_clock_time_get(self, id, precision, out_ptr):
    if id == 0:  # realtime
        ns = time.time_ns()
    elif id in (1, 2, 3):  # monotonic / process / thread cputime
        ns = time.monotonic_ns()
    else:
        return self.ERRNO_INVAL
    self.memory.i64_store(out_ptr, ns & Rt.M64)
    return self.ERRNO_SUCCESS
