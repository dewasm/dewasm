# requires: wasi/write_string_list
def wasi_environ_get(environ_ptr, buf_ptr)
  write_string_list(@env, environ_ptr, buf_ptr)
end
