def i64_load16_ua(self, a, b):
    a = (a + b) & 0xFFFFFFFF
    self.check(a, 2)
    return int.from_bytes(self.data[a:a + 2], "little")
