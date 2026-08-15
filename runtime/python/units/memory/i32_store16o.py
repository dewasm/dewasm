def i32_store16o(self, a, off, v):
    a = (a & 0xFFFFFFFF) + off
    self.check(a, 2)
    self.data[a:a + 2] = (v & 0xFFFF).to_bytes(2, "little")
