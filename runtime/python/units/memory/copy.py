def copy(self, dst, src, length):
    self.check(dst, length)
    self.check(src, length)
    if length == 0:
        return
    self.data[dst:dst + length] = self.data[src:src + length]
