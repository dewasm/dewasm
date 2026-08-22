def uwlba(self, a, b):
    a = (a + b) & 0xFFFFFFFF
    self.check(a, 1)
    return self.data[a]
