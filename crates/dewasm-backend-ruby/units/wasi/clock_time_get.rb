# requires: memory/ids, rt/m64
def wasi_clock_time_get(id, _precision, out_ptr)
  ns =
    case id
    when 0 then Process.clock_gettime(Process::CLOCK_REALTIME, :nanosecond)
    when 1, 2, 3 then Process.clock_gettime(Process::CLOCK_MONOTONIC, :nanosecond)
    else return ERRNO_INVAL
    end
  @memory.ids(out_ptr, Rt.m64(ns))
  ERRNO_SUCCESS
end
