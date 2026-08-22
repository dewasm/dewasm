def fdl(self, a):
    a &= 0xFFFFFFFF
    self.check(a, 8)
    return struct.unpack("<d", self.data[a:a + 8])[0]
