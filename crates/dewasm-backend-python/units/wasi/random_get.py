# requires: memory/init
def wasi_random_get(self, buf_ptr, length):
    self.memory.init(buf_ptr, os.urandom(length), 0, length)
    return self.ERRNO_SUCCESS
