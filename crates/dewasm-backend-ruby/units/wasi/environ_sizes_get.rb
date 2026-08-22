# requires: memory/iws
def wasi_environ_sizes_get(count_ptr, buf_size_ptr)
  @memory.iws(count_ptr, @env.size)
  @memory.iws(buf_size_ptr, @env.sum { |e| e.bytesize + 1 })
  ERRNO_SUCCESS
end
