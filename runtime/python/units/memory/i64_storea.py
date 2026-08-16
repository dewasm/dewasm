def i64_storea(self, a, b, v):
    a = (a + b) & 0xFFFFFFFF
    self.check(a, 8)
    self.data[a:a + 8] = (v if 0 <= v <= 0xFFFFFFFFFFFFFFFF else v & 0xFFFFFFFFFFFFFFFF).to_bytes(8, "little")
