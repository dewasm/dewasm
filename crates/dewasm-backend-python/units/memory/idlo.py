def idlo(self, a, off):
    a = (a & 0xFFFFFFFF) + off
    self.check(a, 8)
    return int.from_bytes(self.data[a:a + 8], "little")
