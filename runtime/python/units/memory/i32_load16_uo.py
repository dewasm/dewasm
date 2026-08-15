def i32_load16_uo(self, a, off):
    a = (a & 0xFFFFFFFF) + off
    self.check(a, 2)
    return int.from_bytes(self.data[a:a + 2], "little")
