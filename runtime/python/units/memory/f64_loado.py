def f64_loado(self, a, off):
    a += off
    self.check(a, 8)
    return struct.unpack("<d", self.data[a:a + 8])[0]
