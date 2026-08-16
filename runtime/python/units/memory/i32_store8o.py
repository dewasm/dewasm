def i32_store8o(self, a, off, v):
    a += off
    self.check(a, 1)
    self.data[a] = v & 0xFF
