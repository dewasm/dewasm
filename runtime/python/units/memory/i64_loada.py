def i64_loada(self, a, b):
    a = (a + b) & 0xFFFFFFFF
    self.check(a, 8)
    return int.from_bytes(self.data[a:a + 8], "little")
