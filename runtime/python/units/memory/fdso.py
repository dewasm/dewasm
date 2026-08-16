def fdso(self, a, off, v):
    a = (a & 0xFFFFFFFF) + off
    self.check(a, 8)
    self.data[a:a + 8] = struct.pack("<d", v)
