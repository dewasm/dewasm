def f64_store(self, a, v):
    a &= 0xFFFFFFFF
    self.check(a, 8)
    self.data[a:a + 8] = struct.pack("<d", v)
