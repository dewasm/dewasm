# requires: memory/i32_store
def wasi_args_sizes_get(argc_ptr, buf_size_ptr)
  @memory.i32_store(argc_ptr, @args.size)
  @memory.i32_store(buf_size_ptr, @args.sum { |a| a.bytesize + 1 })
  ERRNO_SUCCESS
end
