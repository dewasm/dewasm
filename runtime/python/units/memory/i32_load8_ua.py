def i32_load8_ua(self, a, b):
    a = (a + b) & 0xFFFFFFFF
    self.check(a, 1)
    return self.data[a]
