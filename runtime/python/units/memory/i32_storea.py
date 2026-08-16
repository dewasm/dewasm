def i32_storea(self, a, b, v):
    a = (a + b) & 0xFFFFFFFF
    self.check(a, 4)
    self.data[a:a + 4] = (v & 0xFFFFFFFF).to_bytes(4, "little")
