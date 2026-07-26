# requires: wasi/write_string_list
def wasi_environ_get(self, environ_ptr, buf_ptr):
    return self.write_string_list(self.env, environ_ptr, buf_ptr)
