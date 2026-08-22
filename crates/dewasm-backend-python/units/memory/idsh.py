def idsh(self, a, v):
    a &= 0xFFFFFFFF
    self.check(a, 2)
    self.data[a:a + 2] = (v & 0xFFFF).to_bytes(2, "little")
