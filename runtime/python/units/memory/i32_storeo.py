def i32_storeo(self, a, off, v):
    a += off
    self.check(a, 4)
    self.data[a:a + 4] = (v & 0xFFFFFFFF).to_bytes(4, "little")
