# requires: memory/i32_store, memory/i32_store8, memory/init
def write_string_list(self, strings, list_ptr, buf_ptr):
    for i, s in enumerate(strings):
        self.memory.i32_store(list_ptr + i * 4, buf_ptr)
        self.memory.init(buf_ptr, s, 0, len(s))
        self.memory.i32_store8(buf_ptr + len(s), 0)
        buf_ptr += len(s) + 1
    return self.ERRNO_SUCCESS
