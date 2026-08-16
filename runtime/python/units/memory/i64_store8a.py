def i64_store8a(self, a, b, v):
    a = (a + b) & 0xFFFFFFFF
    self.check(a, 1)
    self.data[a] = v & 0xFF
