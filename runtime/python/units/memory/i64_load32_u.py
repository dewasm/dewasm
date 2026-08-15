def i64_load32_u(self, a):
    a &= 0xFFFFFFFF
    self.check(a, 4)
    return int.from_bytes(self.data[a:a + 4], "little")
