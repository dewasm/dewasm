# requires: memory/ids
def wasi_clock_res_get(_id, out_ptr)
  @memory.ids(out_ptr, 1)
  ERRNO_SUCCESS
end
