def i64_store32(self, a, v):
    self.check(a, 4)
    self.data[a:a + 4] = (v & 0xFFFFFFFF).to_bytes(4, "little")
