# requires: wasi/write_string_list
def wasi_args_get(argv_ptr, buf_ptr)
  write_string_list(@args, argv_ptr, buf_ptr)
end
