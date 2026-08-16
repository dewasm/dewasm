def f64_storea(self, a, b, v):
    a = (a + b) & 0xFFFFFFFF
    self.check(a, 8)
    self.data[a:a + 8] = struct.pack("<d", v)
