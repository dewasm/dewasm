def i32_load16_u(self, a):
    a &= 0xFFFFFFFF
    self.check(a, 2)
    return int.from_bytes(self.data[a:a + 2], "little")
