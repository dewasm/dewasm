def fill(self, dst, val, length):
    self.check(dst, length)
    if length == 0:
        return
    self.data[dst:dst + length] = bytes([val & 0xFF]) * length
