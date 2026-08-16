def i64_loado(self, a, off):
    a += off
    self.check(a, 8)
    return int.from_bytes(self.data[a:a + 8], "little")
