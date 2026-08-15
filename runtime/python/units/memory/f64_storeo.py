def f64_storeo(self, a, off, v):
    a += off
    self.check(a, 8)
    self.data[a:a + 8] = struct.pack("<d", v)
