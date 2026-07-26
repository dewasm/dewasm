def i32_store8(self, a, v):
    self.check(a, 1)
    self.data[a] = v & 0xFF
