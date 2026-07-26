def i64_load(self, a):
    self.check(a, 8)
    return int.from_bytes(self.data[a:a + 8], "little")
