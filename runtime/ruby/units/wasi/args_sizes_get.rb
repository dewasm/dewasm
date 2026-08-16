# requires: memory/iws
def wasi_args_sizes_get(argc_ptr, buf_size_ptr)
  @memory.iws(argc_ptr, @args.size)
  @memory.iws(buf_size_ptr, @args.sum { |a| a.bytesize + 1 })
  ERRNO_SUCCESS
end
