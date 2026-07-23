# requires: memory/i64_store
def wasi_clock_res_get(_id, out_ptr)
  @memory.i64_store(out_ptr, 1)
  ERRNO_SUCCESS
end
