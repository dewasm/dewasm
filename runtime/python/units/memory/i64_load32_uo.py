def i64_load32_uo(self, a, off):
    a = (a & 0xFFFFFFFF) + off
    self.check(a, 4)
    return int.from_bytes(self.data[a:a + 4], "little")
