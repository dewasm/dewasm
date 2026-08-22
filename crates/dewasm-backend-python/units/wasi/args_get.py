# requires: wasi/write_string_list
def wasi_args_get(self, argv_ptr, buf_ptr):
    return self.write_string_list(self.args, argv_ptr, buf_ptr)
