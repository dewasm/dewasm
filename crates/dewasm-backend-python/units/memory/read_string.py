def read_string(self, ptr, length):
    self.check(ptr, length)
    return bytes(self.data[ptr:ptr + length])
