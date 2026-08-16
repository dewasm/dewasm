def i32_loado(self, a, off):
    a += off
    self.check(a, 4)
    return int.from_bytes(self.data[a:a + 4], "little")
