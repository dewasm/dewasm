def i64_storeo(self, a, off, v):
    a += off
    self.check(a, 8)
    self.data[a:a + 8] = (v & 0xFFFFFFFFFFFFFFFF).to_bytes(8, "little")
