# requires: memory/i32_store
def wasi_environ_sizes_get(count_ptr, buf_size_ptr)
  @memory.i32_store(count_ptr, @env.size)
  @memory.i32_store(buf_size_ptr, @env.sum { |e| e.bytesize + 1 })
  ERRNO_SUCCESS
end
