def i32_load(self, a):
    self.check(a, 4)
    return int.from_bytes(self.data[a:a + 4], "little")
