def i64_store(self, a, v):
    self.check(a, 8)
    self.data[a:a + 8] = (v & 0xFFFFFFFFFFFFFFFF).to_bytes(8, "little")
