# requires: memory/i64_store
def wasi_clock_time_get(id, _precision, out_ptr)
  ns =
    case id
    when 0 then Process.clock_gettime(Process::CLOCK_REALTIME, :nanosecond)
    when 1, 2, 3 then Process.clock_gettime(Process::CLOCK_MONOTONIC, :nanosecond)
    else return ERRNO_INVAL
    end
  @memory.i64_store(out_ptr, ns & Rt::M64)
  ERRNO_SUCCESS
end
