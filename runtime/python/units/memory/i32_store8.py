def i32_store8(self, a, v):
    a &= 0xFFFFFFFF
    self.check(a, 1)
    self.data[a] = v & 0xFF
