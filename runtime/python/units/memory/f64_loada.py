def f64_loada(self, a, b):
    a = (a + b) & 0xFFFFFFFF
    self.check(a, 8)
    return struct.unpack("<d", self.data[a:a + 8])[0]
