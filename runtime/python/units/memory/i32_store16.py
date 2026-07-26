def i32_store16(self, a, v):
    self.check(a, 2)
    self.data[a:a + 2] = (v & 0xFFFF).to_bytes(2, "little")
